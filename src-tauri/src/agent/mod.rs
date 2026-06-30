//! 하위 에이전트 레이어(설계 §9). 기존 pstmacro 앱에 **추가만** 되는 모듈 — 기존 로그인·큐·계정
//! 코드는 한 줄도 바꾸지 않고, 그 함수/스토어를 호출만 한다(adb.rs의 상태신호는 "추가").
//!
//! 하는 일:
//! ① 서버에 SSE로 연결해 명령 수신(distribute/login/delete)
//! ② 받은 계정을 기존 계정 스토어에 등록 + 기존 종토 선택로그인 경로로 자동 전체 로그인 enqueue
//! ③ 로그인 끝나면 결과 4분류(성공/보류/대기초과/실패) + 누적을 서버에 보고 + 실패 자동삭제(§10-4)
//! ④ 하트비트(현재 IP)·ROTATING 상태 보고(§4) + 끊기면 백오프 재연결(§4-1)

mod config;
mod net;

pub use config::AgentConfig;

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::mpsc;

use crate::ipc::accounts::{Account, AccountStatus, PlatformId};
use crate::ipc::log_batches::LogBatch;
use crate::ipc::posts::ModeValue;
use crate::ipc::queue::{
    apply_priority_order, as_fresh_now_item, LoginTarget, PublishPlan, QueueLocation, QueueNowItem,
    QueueState,
};
use crate::ipc::queue_runner::{start_if_idle, NowQueueRunner};
use crate::store::JsonStore;

/// 서버가 SSE로 내려보내는 명령.
#[derive(Deserialize)]
struct Command {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "commandId", default)]
    command_id: Option<String>,
    #[serde(default)]
    accounts: Vec<AccountIn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountIn {
    login_id: String,
    pw: String,
}

/// 에이전트 상태(하위 등록 화면 표시용).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub configured: bool,
    pub server_url: String,
    pub device_name: String,
}

/// dispatch가 즉시 응답 후 백그라운드로 이어갈 후속 작업(로그인 결과 보고).
struct Followup {
    queue_id: String,
    login_ids: Vec<String>,
    // 이 분배에서 새로 등록된 계정 수와, 그중 로그인 엔진이 볼 수 있는 수(§10-1 등록 확인).
    registered: usize,
    registered_visible: usize,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// IP 회전 등 상태신호를 adb.rs(AppHandle 없는 곳)에서 에이전트로 보내는 전역 채널.
static STATE_TX: OnceLock<mpsc::UnboundedSender<(String, Option<String>)>> = OnceLock::new();

/// 기존 로그인/회전 흐름에서 호출하는 상태신호(추가 전용). 미등록·미기동이면 no-op.
/// `state`="rotating"|"online", online이면 `ip`=바뀐 공인 IP(§4-1).
pub fn report_state_change(state: &str, ip: Option<String>) {
    if let Some(tx) = STATE_TX.get() {
        let _ = tx.send((state.to_string(), ip));
    }
}

/// 앱 시작 시 호출(setup, 추가 1줄). 명령 수신·하트비트·상태보고 루프를 백그라운드로 띄운다.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    let (tx, rx) = mpsc::unbounded_channel::<(String, Option<String>)>();
    let _ = STATE_TX.set(tx);

    let cmd_app = app.clone();
    let post_app = app.clone();
    tauri::async_runtime::spawn(async move { command_loop(cmd_app).await });
    tauri::async_runtime::spawn(async move { heartbeat_loop().await });
    tauri::async_runtime::spawn(async move { state_report_loop(rx).await });
    tauri::async_runtime::spawn(async move { post_report_loop(post_app).await });
}

// ───────────────────────── 명령 수신 루프 ─────────────────────────

async fn command_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut backoff = 1u64;
    loop {
        let Some(cfg) = config::load() else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };
        match net::open_stream(&client, &cfg.server_url, &cfg.device_token).await {
            Ok(mut resp) => {
                backoff = 1;
                tracing::info!("[AGENT] SSE 연결됨 → {}", cfg.server_url);
                let mut buf = String::new();
                loop {
                    match resp.chunk().await {
                        Ok(Some(bytes)) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            drain_events(&app, &client, &cfg, &mut buf).await;
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::warn!("[AGENT] 스트림 끊김: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("[AGENT] 연결 실패: {e}"),
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

async fn drain_events<R: Runtime>(
    app: &AppHandle<R>,
    client: &reqwest::Client,
    cfg: &AgentConfig,
    buf: &mut String,
) {
    while let Some(nl) = buf.find('\n') {
        let line = buf[..nl].trim().to_string();
        buf.drain(..=nl);
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        let Ok(cmd) = serde_json::from_str::<Command>(data) else {
            continue;
        };
        let cid = cmd.command_id.clone().unwrap_or_else(|| format!("c-{}", now_ms()));
        // 동기 디스패치(기존 스토어/큐 호출) → 즉시 응답.
        let (level, msg, followup) = dispatch(app, &cmd);
        let _ = net::post_result(client, &cfg.server_url, &cfg.device_token, &cid, level, &msg).await;
        // 로그인이 걸렸으면 끝날 때까지 지켜보고 §10-4 결과를 같은 commandId로 보고(백그라운드).
        if let Some(f) = followup {
            let (app2, client2, cfg2, cid2) =
                (app.clone(), client.clone(), cfg.clone(), cid.clone());
            tauri::async_runtime::spawn(async move {
                report_login_results(app2, client2, cfg2, cid2, f).await;
            });
        }
    }
}

/// 명령 디스패치(동기). 반환: (level, 즉시 메시지, 로그인 결과 후속).
fn dispatch<R: Runtime>(app: &AppHandle<R>, cmd: &Command) -> (&'static str, String, Option<Followup>) {
    match cmd.kind.as_str() {
        "distribute_accounts" => {
            let (added, visible) = add_accounts(app, &cmd.accounts);
            let login_ids: Vec<String> = cmd.accounts.iter().map(|a| a.login_id.clone()).collect();
            let queue_id = enqueue_login(app, &login_ids);
            (
                "ok",
                format!("계정 {added}건 등록(로그인 대상 {visible}건) + 자동 로그인 시작(종토)"),
                queue_id.map(|q| Followup {
                    queue_id: q,
                    login_ids,
                    registered: added,
                    registered_visible: visible,
                }),
            )
        }
        "import_then_login_all" => {
            let ids = all_login_ids(app);
            let n = ids.len();
            let queue_id = enqueue_login(app, &ids);
            (
                "ok",
                format!("전체 로그인 시작 — {n}건"),
                queue_id.map(|q| Followup {
                    queue_id: q,
                    login_ids: ids,
                    registered: 0,
                    registered_visible: 0,
                }),
            )
        }
        "delete_accounts" => {
            let ids: Vec<String> = cmd.accounts.iter().map(|a| a.login_id.clone()).collect();
            let removed = delete_by_login_ids(app, &ids);
            ("info", format!("계정 {removed}건 삭제"), None)
        }
        other => ("fail", format!("알 수 없는 명령: {other}"), None),
    }
}

/// 반환: (IPC 스토어에 새로 추가된 수, 그중 로그인 엔진(accounts.json)이 볼 수 있는 수).
fn add_accounts<R: Runtime>(app: &AppHandle<R>, accounts: &[AccountIn]) -> (usize, usize) {
    let store = app.state::<JsonStore<Account>>();
    let mut added = 0usize;
    store.mutate(|mut list| {
        for a in accounts {
            if list.iter().any(|x| x.login_id == a.login_id) {
                continue;
            }
            list.push(Account {
                id: a.login_id.clone(),
                platform: PlatformId::Forum,
                login_id: a.login_id.clone(),
                pw: a.pw.clone(),
                status: AccountStatus::New,
                status_msg: None,
                status_trace: None,
                last: "—".into(),
                tags: vec![],
            });
            added += 1;
        }
        list
    });

    // ⚠️ 로그인 엔진은 위 IPC 스토어가 아니라 *별도 파일* accounts.json(auth::Account)을 읽는다
    // (auth::load_accounts_file). 분배된 계정을 거기에 안 쓰면 자동 로그인이 "account not found"로
    // 떨어지고, 그 실패가 자동삭제까지 이어져 멀쩡한 계정이 파괴된다. 그래서 프론트 save_accounts와
    // 동일하게 같은 계정을 accounts.json에도 기록한다(save_accounts_file이 id로 병합: 기존 갱신·신규 추가).
    let auth_accounts: Vec<crate::auth::Account> = accounts
        .iter()
        .map(|a| crate::auth::Account {
            id: a.login_id.clone(),
            password: a.pw.clone(),
            label: a.login_id.clone(),
        })
        .collect();
    let visible = match crate::auth::save_accounts_file(&auth_accounts) {
        Ok(merged) => {
            // 진단: 방금 등록한 계정이 로그인 엔진이 읽는 파일에서 실제로 보이는지 확인.
            let visible = auth_accounts
                .iter()
                .filter(|a| merged.iter().any(|m| m.id == a.id))
                .count();
            tracing::info!(
                ipc_added = added,
                login_visible = visible,
                total = auth_accounts.len(),
                "[AGENT] 분배 계정 등록 — IPC 스토어 + accounts.json(로그인 엔진) 양쪽 기록"
            );
            visible
        }
        Err(error) => {
            tracing::warn!(
                "[AGENT] accounts.json 기록 실패 — 자동 로그인이 'account not found'로 실패할 수 있음: {error}"
            );
            0
        }
    };

    (added, visible)
}

fn all_login_ids<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    app.state::<JsonStore<Account>>()
        .snapshot()
        .into_iter()
        .map(|a| a.login_id)
        .collect()
}

fn delete_by_login_ids<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) -> usize {
    let store = app.state::<JsonStore<Account>>();
    let mut removed = 0usize;
    store.mutate(|list| {
        let before = list.len();
        let kept: Vec<Account> = list
            .into_iter()
            .filter(|a| !login_ids.contains(&a.login_id))
            .collect();
        removed = before - kept.len();
        kept
    });
    removed
}

/// 선택 로그인(종토) 큐 아이템 1개를 만들어 기존 now 큐에 적재 + 러너 기동. 큐 아이템 id 반환.
fn enqueue_login<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) -> Option<String> {
    if login_ids.is_empty() {
        return None;
    }
    let login: Vec<LoginTarget> = login_ids
        .iter()
        .map(|id| LoginTarget {
            account_id: id.clone(),
            platform: PlatformId::Naver,
            headless: false,
            use_adb: true,
            force: true,
        })
        .collect();
    let locs: Vec<QueueLocation> = login_ids
        .iter()
        .map(|id| QueueLocation { p: PlatformId::Forum, name: id.clone(), code: None })
        .collect();
    let title = format!("계정 로그인 {}건", login_ids.len());
    let id = format!("agent-login-{}", now_ms());
    let item = QueueNowItem {
        id: id.clone(),
        title: title.clone(),
        kind: ModeValue::Post,
        state: QueueState::Waiting,
        batch_id: None,
        progress: None,
        locs,
        plan: Some(PublishPlan {
            post_id: String::new(),
            kind: ModeValue::Post,
            title,
            body_text: String::new(),
            comments: vec![],
            link_override: String::new(),
            naver: vec![],
            forum: vec![],
            band: vec![],
            blog: vec![],
            clip: vec![],
            login: Some(login),
        }),
        items: vec![],
    };
    let now = app.state::<JsonStore<QueueNowItem>>();
    now.mutate(|mut items| {
        items.push(as_fresh_now_item(item));
        apply_priority_order(items)
    });
    let runner = app.state::<NowQueueRunner>();
    start_if_idle(runner.inner(), app.clone());
    Some(id)
}

// ───────────────────────── §10-4 로그인 결과 보고 + 실패 자동삭제 ─────────────────────────

/// 분류 결과.
struct Tally {
    success: usize,
    onhold: Vec<(String, String, String)>, // (loginId, pw, 보류사유)
    timedout: Vec<(String, String)>,        // (loginId, pw)
    // (loginId, pw, 실패사유, trace) — trace는 "자세히 보기"용 백트레이스(없으면 None).
    failed: Vec<(String, String, String, Option<String>)>,
}

/// 큐 아이템이 끝날(Done) 때까지 기다렸다가 계정 상태로 §10-4 분류 → 보고 + 실패 자동삭제 + 누적 갱신.
async fn report_login_results<R: Runtime>(
    app: AppHandle<R>,
    client: reqwest::Client,
    cfg: AgentConfig,
    command_id: String,
    f: Followup,
) {
    // 큐 아이템이 Done 될 때까지 폴링(최대 30분 안전장치). 사라지면(치워짐) 완료로 간주.
    let deadline = std::time::Instant::now() + Duration::from_secs(30 * 60);
    loop {
        let done = {
            let items = app.state::<JsonStore<QueueNowItem>>().snapshot();
            match items.iter().find(|i| i.id == f.queue_id) {
                Some(i) => matches!(i.state, QueueState::Done),
                None => true, // 큐에서 제거됨 → 완료로 봄
            }
        };
        if done || std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    // 완료된 계정 상태를 §10-4 4분류로(동기 스냅샷).
    let tally = classify_accounts(&app, &f.login_ids);
    let received = f.login_ids.len();
    let cum = ledger_add(received, &tally);
    let report = format_report(&tally, received, &cum);
    let _ = net::post_result(&client, &cfg.server_url, &cfg.device_token, &command_id, "ok", &report).await;
    // 구조화 로그인 결과도 보고(§10-4-1) → 결과보고 '로그인 결과' 탭이 실데이터로 렌더.
    let body = login_report_body(&command_id, &tally, &cum, f.registered, f.registered_visible);
    let _ = net::post_login_report(&client, &cfg.server_url, &cfg.device_token, &body).await;

    // 실패 계정 자동삭제(§10-1 (4)) — 단, *비밀번호 오류(BadCredentials)*처럼 계정 자체가 무효인
    // 경우만 삭제한다. account not found·네트워크·타임아웃·추가인증·차단 같은 일시적·인프라성
    // 실패까지 삭제하면 멀쩡한 계정이 사라진다(사용자 지시 2026-06-30). 삭제 대상 status를 지금
    // 스냅샷에서 다시 확인해 BadCredentials만 고르고, 나머지 실패는 보존하고 로그로 남긴다.
    if !tally.failed.is_empty() {
        let snapshot = app.state::<JsonStore<Account>>().snapshot();
        let failed_ids: Vec<String> = tally.failed.iter().map(|(id, _, _, _)| id.clone()).collect();
        let (delete_ids, retained_ids) = partition_auto_delete(&failed_ids, |id| {
            snapshot
                .iter()
                .find(|a| a.login_id == id)
                .map(|a| a.status.clone())
        });

        if !retained_ids.is_empty() {
            tracing::info!(
                retained = ?retained_ids,
                "[AGENT] 실패했지만 보존 — 일시적·인프라성 실패(비번오류 아님)는 자동삭제하지 않음"
            );
        }
        if !delete_ids.is_empty() {
            let removed = delete_by_login_ids(&app, &delete_ids);
            let del_msg = format!(
                "delete_accounts(계정 삭제) {removed}건(비밀번호 오류만) → {}",
                tally
                    .failed
                    .iter()
                    .filter(|(id, _, _, _)| delete_ids.contains(id))
                    .map(|(id, pw, why, _)| format!("{id}/{pw} (사유: {why})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let _ = net::post_result(&client, &cfg.server_url, &cfg.device_token, &command_id, "info", &del_msg).await;
        }
    }
}

/// 실패 계정 중 *자동삭제 대상*(비밀번호 오류 = 계정 자체가 무효)과 *보존 대상*(나머지: account
/// not found·네트워크·타임아웃·추가인증·차단 같은 일시적·인프라성 실패)을 가른다. 순수 함수.
/// 반환 = (삭제할 id, 보존할 id). status_of가 None(스토어에 없음)이면 보존한다(account not found
/// 류는 일시적이라 삭제하지 않는다 — 사용자 지시 2026-06-30).
fn partition_auto_delete(
    failed_ids: &[String],
    status_of: impl Fn(&str) -> Option<AccountStatus>,
) -> (Vec<String>, Vec<String>) {
    failed_ids
        .iter()
        .cloned()
        .partition(|id| matches!(status_of(id), Some(AccountStatus::BadCredentials)))
}

fn classify_accounts<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) -> Tally {
    let snapshot = app.state::<JsonStore<Account>>().snapshot();
    let mut t = Tally { success: 0, onhold: vec![], timedout: vec![], failed: vec![] };
    for id in login_ids {
        let Some(acct) = snapshot.iter().find(|a| &a.login_id == id) else {
            // 보낸 계정이 스토어에 없음(중복 스킵·삭제 등). 조용히 빼면 "보낸 N개"와 "보고된 N개"가
            // 어긋난다(사용자: 2개 보냈는데 1개만 나옴). 빼지 말고 실패로 명시해 누락 0을 보장한다.
            t.failed.push((
                id.clone(),
                String::new(),
                "계정이 스토어에 없음(중복/삭제 추정)".to_string(),
                None,
            ));
            continue;
        };
        let pw = acct.pw.clone();
        let why = acct.status_msg.clone().unwrap_or_default();
        match acct.status {
            AccountStatus::Active => t.success += 1,
            AccountStatus::OnHold => {
                let reason = if why.is_empty() { "보류".to_string() } else { why };
                t.onhold.push((id.clone(), pw, reason));
            }
            AccountStatus::TimedOut => t.timedout.push((id.clone(), pw)),
            // 미시도/게시쿨다운 등 로그인 결과 아님 — 삭제·집계 제외.
            AccountStatus::New | AccountStatus::Waiting => {}
            // 비번오류·추가인증·차단·에러 = 실패(§10-4)
            AccountStatus::BadCredentials
            | AccountStatus::Challenge
            | AccountStatus::Blocked
            | AccountStatus::Error => {
                let reason = if why.is_empty() {
                    format!("{:?}", acct.status)
                } else {
                    why
                };
                t.failed.push((id.clone(), pw, reason, acct.status_trace.clone()));
            }
        }
    }
    t
}

/// §10-4 보고 본문(통신 로그에 ID/PW 평문 — §10-5). 성공은 개수만, 나머지는 ID/PW(+사유).
fn format_report(t: &Tally, received: usize, cum: &Cumulative) -> String {
    let mut s = format!(
        "성공 {} / 보류 {} / 대기초과 {} / 실패 {}",
        t.success,
        t.onhold.len(),
        t.timedout.len(),
        t.failed.len()
    );
    for (id, pw, why) in &t.onhold {
        s.push_str(&format!("\n  보류  {id} / {pw}  사유: {why}"));
    }
    for (id, pw) in &t.timedout {
        s.push_str(&format!("\n  대기초과  {id} / {pw}"));
    }
    for (id, pw, why, _trace) in &t.failed {
        // 통신로그 텍스트엔 사유 한 줄만(백트레이스는 구조화 보고의 trace로 가서 "자세히 보기"에 노출).
        s.push_str(&format!("\n  실패  {id} / {pw}  사유: {why}"));
    }
    s.push_str(&format!(
        "\n총 받은 계정 {} · 성공 {} / 보류 {} / 대기초과 {} / 실패 {}",
        cum.received, cum.success, cum.onhold, cum.timedout, cum.failed
    ));
    let _ = received;
    s
}

/// §10-4-1 구조화 로그인 결과 본문(서버 `LoginReportReq` 모양). 성공은 개수만, 보류/실패는
/// ID/PW+사유, 대기초과는 ID/PW만. 누적 합계 동봉. 순수함수(테스트 대상).
fn login_report_body(
    command_id: &str,
    t: &Tally,
    cum: &Cumulative,
    registered: usize,
    registered_visible: usize,
) -> serde_json::Value {
    let line3 = |v: &[(String, String, String)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(id, pw, why)| serde_json::json!({ "loginId": id, "pw": pw, "reason": why }))
            .collect()
    };
    let line2 = |v: &[(String, String)]| -> Vec<serde_json::Value> {
        v.iter()
            .map(|(id, pw)| serde_json::json!({ "loginId": id, "pw": pw }))
            .collect()
    };
    // 실패 줄은 사유(reason) + 백트레이스(trace, 게시 결과와 동일하게 "자세히 보기"용)를 함께 싣는다.
    let failed: Vec<serde_json::Value> = t
        .failed
        .iter()
        .map(|(id, pw, why, trace)| {
            serde_json::json!({ "loginId": id, "pw": pw, "reason": why, "trace": trace })
        })
        .collect();
    serde_json::json!({
        "commandId": command_id,
        "registered": registered,
        "registeredVisible": registered_visible,
        "batch": {
            "success": t.success,
            "onhold": line3(&t.onhold),
            "timedout": line2(&t.timedout),
            "failed": failed,
        },
        "cumulative": {
            "received": cum.received,
            "success": cum.success,
            "onhold": cum.onhold,
            "timedout": cum.timedout,
            "failed": cum.failed,
        }
    })
}

// ── 누적 ledger(§10-4) — 작은 json으로 영속화 ──
#[derive(Default, Serialize, Deserialize, Clone)]
struct Cumulative {
    received: usize,
    success: usize,
    onhold: usize,
    timedout: usize,
    failed: usize,
}

fn ledger_path() -> Option<std::path::PathBuf> {
    crate::auth::app_data_root().ok().map(|r| r.join("agent-ledger.json"))
}

fn ledger_add(received: usize, t: &Tally) -> Cumulative {
    let mut c: Cumulative = ledger_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    c.received += received;
    c.success += t.success;
    c.onhold += t.onhold.len();
    c.timedout += t.timedout.len();
    c.failed += t.failed.len();
    if let Some(p) = ledger_path() {
        if let Ok(s) = serde_json::to_string_pretty(&c) {
            let _ = std::fs::write(p, s);
        }
    }
    c
}

// ───────────────────────── §10-4-2 게시 결과 보고 루프 ─────────────────────────
//
// 하위는 게시가 끝날 때마다 자기 로컬 게시 완료 로그(`LogBatch`)를 이미 만들어 둔다
// (데스크톱 앱과 동일, `queue_runner.rs::store_log_batch`). 에이전트는 그 스토어를 폴링해
// **아직 안 올린 완료 배치**를 그대로 서버에 보고한다 → Admin '게시 결과' 탭이 같은 모델로 렌더.
// 로그인 결과(§10-4)와 달리 commandId·명령에 묶이지 않는 별도 흐름이다(게시는 로컬 큐가 돌림).

/// 보고 완료한 배치 id(중복 방지). 스토어는 최대 MAX_LOG_BATCHES(500)건만 유지하므로 이 목록은
/// 그보다 넉넉히만 들고 있으면 된다(스토어에서 빠진 배치는 다시 안 보임).
const MAX_REPORTED_IDS: usize = 2000;

fn reported_path() -> Option<std::path::PathBuf> {
    crate::auth::app_data_root()
        .ok()
        .map(|r| r.join("agent-reported-batches.json"))
}

fn load_reported() -> Vec<String> {
    reported_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_reported(ids: &[String]) {
    if let Some(p) = reported_path() {
        if let Ok(s) = serde_json::to_string(ids) {
            let _ = std::fs::write(p, s);
        }
    }
}

/// 스토어 배치 중 **아직 안 올린 완료 배치**를 오래된 것부터(시간순) 고른다. 스토어는 최신순
/// (insert(0))이라 뒤집고, 진행 중(state=running)은 제외, 이미 보고한 id는 제외(순수함수).
fn unreported_oldest_first(batches: &[LogBatch], reported: &[String]) -> Vec<LogBatch> {
    batches
        .iter()
        .rev()
        .filter(|b| b.state.is_none() && !reported.contains(&b.id))
        .cloned()
        .collect()
}

/// 보고 완료 id를 누적하되 상한(MAX_REPORTED_IDS)을 넘으면 오래된 것부터 버린다(순수함수).
fn push_reported(reported: &mut Vec<String>, id: String) {
    reported.push(id);
    if reported.len() > MAX_REPORTED_IDS {
        let drop = reported.len() - MAX_REPORTED_IDS;
        reported.drain(0..drop);
    }
}

/// 게시 완료 로그(`LogBatch`) 스토어를 폴링해 미보고 완료 배치를 서버에 올린다(§10-4-2).
async fn post_report_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut reported: Vec<String> = load_reported();
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let Some(cfg) = config::load() else {
            continue; // 미등록이면 보고 안 함(단독 동작 무영향)
        };
        let batches = app.state::<JsonStore<LogBatch>>().snapshot();
        for batch in unreported_oldest_first(&batches, &reported) {
            let Ok(body) = serde_json::to_value(&batch) else {
                continue;
            };
            match net::post_report(&client, &cfg.server_url, &cfg.device_token, &body).await {
                Ok(()) => {
                    push_reported(&mut reported, batch.id.clone());
                    save_reported(&reported);
                }
                Err(e) => {
                    // 서버 미연결 등 → 다음 틱에 재시도(보고 안 됨으로 남김).
                    tracing::warn!("[AGENT] 게시 결과 보고 실패(batch={}): {e}", batch.id);
                    break; // 연결 문제면 이번 틱 나머지도 어차피 실패 → 다음 틱에.
                }
            }
        }
    }
}

// ───────────────────────── 하트비트 + 상태 보고 루프 ─────────────────────────

async fn heartbeat_loop() {
    let client = reqwest::Client::new();
    loop {
        if let Some(cfg) = config::load() {
            let ip = crate::auth::fetch_external_ip().await;
            let ip_opt = if ip.starts_with('(') { None } else { Some(ip.as_str()) };
            let _ = net::heartbeat(&client, &cfg.server_url, &cfg.device_token, ip_opt, "online").await;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

/// adb.rs가 보낸 상태신호를 서버로 전달(§4). rotating=상태 전이, online=하트비트(바뀐 IP).
async fn state_report_loop(mut rx: mpsc::UnboundedReceiver<(String, Option<String>)>) {
    let client = reqwest::Client::new();
    while let Some((state, ip)) = rx.recv().await {
        let Some(cfg) = config::load() else { continue };
        if state == "online" {
            let _ = net::heartbeat(&client, &cfg.server_url, &cfg.device_token, ip.as_deref(), "online").await;
        } else {
            let _ = net::post_state(&client, &cfg.server_url, &cfg.device_token, &state).await;
        }
    }
}

// ===================== Tauri 명령(하위 등록 화면 §6-2) =====================

#[tauri::command]
pub async fn agent_register(server_url: String, code: String) -> Result<AgentStatus, String> {
    let base = server_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || code.trim().is_empty() {
        return Err("서버 주소와 기기코드를 입력하세요".into());
    }
    let client = reqwest::Client::new();
    let resp = net::register(&client, &base, code.trim(), None).await?;
    let device_name = format!("하위-{}", resp.device_id.chars().take(4).collect::<String>());
    config::save(&AgentConfig {
        server_url: base.clone(),
        device_token: resp.device_token,
        device_name: device_name.clone(),
    })?;
    Ok(AgentStatus { configured: true, server_url: base, device_name })
}

#[tauri::command]
pub fn agent_status() -> AgentStatus {
    match config::load() {
        Some(c) => AgentStatus {
            configured: true,
            server_url: c.server_url,
            device_name: c.device_name,
        },
        None => AgentStatus {
            configured: false,
            server_url: String::new(),
            device_name: String::new(),
        },
    }
}

#[tauri::command]
pub fn agent_unregister() -> Result<(), String> {
    config::clear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::log_batches::BatchState;

    fn batch(id: &str, running: bool) -> LogBatch {
        LogBatch {
            id: id.into(),
            title: "게시".into(),
            body: None,
            comment: None,
            kind: ModeValue::Post,
            at: 1,
            state: if running { Some(BatchState::Running) } else { None },
            items: vec![],
        }
    }

    #[test]
    fn unreported_skips_running_and_already_reported_oldest_first() {
        // 스토어는 최신순(insert(0)): [c(최신), b, a(오래된)]. b는 진행 중, a는 이미 보고됨.
        let batches = vec![batch("c", false), batch("b", true), batch("a", false)];
        let reported = vec!["a".to_string()];
        let picked: Vec<String> = unreported_oldest_first(&batches, &reported)
            .into_iter()
            .map(|b| b.id)
            .collect();
        // a=보고됨 제외, b=진행 중 제외 → c만, 그리고 오래된 것부터(여기선 c 하나).
        assert_eq!(picked, vec!["c".to_string()]);
    }

    #[test]
    fn unreported_returns_oldest_first_order() {
        // 완료·미보고 둘: 스토어 [y(최신), x(오래된)] → 시간순 [x, y]로 보고해야 함.
        let batches = vec![batch("y", false), batch("x", false)];
        let picked: Vec<String> = unreported_oldest_first(&batches, &[])
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(picked, vec!["x".to_string(), "y".to_string()]);
    }

    #[test]
    fn push_reported_caps_at_max_dropping_oldest() {
        let mut reported: Vec<String> = (0..MAX_REPORTED_IDS).map(|i| format!("b{i}")).collect();
        push_reported(&mut reported, "new".into());
        assert_eq!(reported.len(), MAX_REPORTED_IDS);
        assert_eq!(reported.last().unwrap(), "new"); // 새 id는 남고
        assert_eq!(reported.first().unwrap(), "b1"); // 가장 오래된 b0은 밀려남
    }

    #[test]
    fn login_report_body_shapes_batch_and_cumulative() {
        let t = Tally {
            success: 3,
            onhold: vec![("aaa".into(), "pw1".into(), "캡차".into())],
            timedout: vec![("bbb".into(), "pw2".into())],
            failed: vec![(
                "ccc".into(),
                "pw3".into(),
                "연결 실패".into(),
                Some("at x.rs:1:1\n\nframe0".into()),
            )],
        };
        let cum = Cumulative {
            received: 20,
            success: 6,
            onhold: 3,
            timedout: 5,
            failed: 6,
        };
        let v = login_report_body("c-1", &t, &cum, 2, 2);
        assert_eq!(v["commandId"], "c-1");
        assert_eq!(v["batch"]["success"], 3);
        // 보류·실패는 ID/PW+사유, 대기초과는 사유 없음.
        assert_eq!(v["batch"]["onhold"][0]["loginId"], "aaa");
        assert_eq!(v["batch"]["onhold"][0]["reason"], "캡차");
        assert!(v["batch"]["timedout"][0].get("reason").is_none());
        assert_eq!(v["batch"]["failed"][0]["reason"], "연결 실패");
        // 실패 줄은 "자세히 보기"용 trace를 함께 싣는다(게시 결과와 동일).
        assert_eq!(v["batch"]["failed"][0]["trace"], "at x.rs:1:1\n\nframe0");
        // 등록 정보(§10-1 등록 확인)도 동봉.
        assert_eq!(v["registered"], 2);
        assert_eq!(v["registeredVisible"], 2);
        // 누적 합계 동봉.
        assert_eq!(v["cumulative"]["received"], 20);
        assert_eq!(v["cumulative"]["failed"], 6);
    }

    #[test]
    fn partition_auto_delete_removes_only_bad_credentials() {
        use std::collections::HashMap;
        // 비번오류만 삭제 대상, 차단·에러·추가인증·"스토어에 없음(account not found)"은 보존.
        let status: HashMap<&str, AccountStatus> = HashMap::from([
            ("bad", AccountStatus::BadCredentials),
            ("blocked", AccountStatus::Blocked),
            ("error", AccountStatus::Error),
            ("challenge", AccountStatus::Challenge),
        ]);
        let failed = vec![
            "bad".to_string(),
            "blocked".to_string(),
            "error".to_string(),
            "challenge".to_string(),
            "gone".to_string(), // 스토어에 없음 → status_of None
        ];
        let (delete_ids, retained_ids) =
            partition_auto_delete(&failed, |id| status.get(id).cloned());
        assert_eq!(delete_ids, vec!["bad"], "비밀번호 오류만 삭제");
        assert_eq!(
            retained_ids,
            vec!["blocked", "error", "challenge", "gone"],
            "차단·에러·추가인증·account not found는 보존(삭제 금지)"
        );
    }
}
