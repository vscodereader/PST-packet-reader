//! 하위 에이전트 레이어(설계 §9). 기존 pstmacro 앱에 **추가만** 되는 모듈 — 기존 로그인·큐·계정
//! 코드는 한 줄도 바꾸지 않고, 그 함수/스토어를 호출만 한다.
//!
//! 하는 일: ① 서버에 SSE로 연결해 명령 수신 ② 받은 계정을 기존 계정 스토어에 등록 +
//! 기존 로그인 큐로 자동 전체 로그인 enqueue ③ 하트비트(현재 IP)·결과를 서버에 POST
//! ④ 연결 끊기면 백오프 재연결(§4-1).

mod config;
mod net;

pub use config::AgentConfig;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::ipc::accounts::{Account, AccountStatus, PlatformId};
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

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// 앱 시작 시 호출(setup, 추가 1줄). 명령 수신 루프 + 하트비트 루프를 백그라운드로 띄운다.
/// 설정이 없으면 두 루프는 대기만 한다(등록 전까지 무동작).
pub fn start<R: Runtime>(app: AppHandle<R>) {
    let cmd_app = app.clone();
    tauri::async_runtime::spawn(async move { command_loop(cmd_app).await });
    tauri::async_runtime::spawn(async move { heartbeat_loop(app).await });
}

/// 명령 수신 루프: 설정 있으면 SSE 연결 → 명령 디스패치, 끊기면 백오프 재연결(§4-1).
async fn command_loop<R: Runtime>(app: AppHandle<R>) {
    let client = reqwest::Client::new();
    let mut backoff = 1u64;
    loop {
        let Some(cfg) = config::load() else {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
                        Ok(None) => break, // 스트림 종료 → 재연결
                        Err(e) => {
                            tracing::warn!("[AGENT] 스트림 끊김: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => tracing::warn!("[AGENT] 연결 실패: {e}"),
        }
        // 백오프(1→2→4→…→30s) 재시도(§4-1 (4)).
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
        backoff = (backoff * 2).min(30);
    }
}

/// 버퍼에서 완성된 SSE `data:` 줄을 꺼내 명령으로 처리한다.
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
            continue; // 주석(keep-alive)·빈 줄 등 무시
        };
        let data = data.trim();
        let Ok(cmd) = serde_json::from_str::<Command>(data) else {
            continue;
        };
        let cid = cmd.command_id.clone().unwrap_or_else(|| format!("c-{}", now_ms()));
        // ★ 상태 접근은 동기로 끝내고(아래 dispatch), 그 결과 메시지만 await POST한다.
        let (level, msg) = dispatch(app, &cmd);
        let _ = net::post_result(client, &cfg.server_url, &cfg.device_token, &cid, level, &msg).await;
    }
}

/// 명령 디스패치(동기 — 기존 스토어/큐 함수 호출만). State 가드를 await 너머로 들지 않게 한다.
fn dispatch<R: Runtime>(app: &AppHandle<R>, cmd: &Command) -> (&'static str, String) {
    match cmd.kind.as_str() {
        // 분배: 받은 계정을 기존 계정 스토어에 등록 + 기존 로그인 큐로 자동 전체 로그인 enqueue.
        "distribute_accounts" => {
            let added = add_accounts(app, &cmd.accounts);
            let login_ids: Vec<String> =
                cmd.accounts.iter().map(|a| a.login_id.clone()).collect();
            enqueue_login(app, &login_ids);
            (
                "ok",
                format!("계정 {added}건 등록 + 자동 로그인 시작(종토)"),
            )
        }
        // 전체 로그인: 현재 스토어의 모든 계정을 선택 로그인 큐로.
        "import_then_login_all" => {
            let ids = all_login_ids(app);
            let n = ids.len();
            enqueue_login(app, &ids);
            ("ok", format!("전체 로그인 시작 — {n}건"))
        }
        // 계정 삭제: 받은 loginId들을 기존 계정 스토어에서 제거.
        "delete_accounts" => {
            let ids: Vec<String> = cmd.accounts.iter().map(|a| a.login_id.clone()).collect();
            let removed = delete_by_login_ids(app, &ids);
            ("info", format!("계정 {removed}건 삭제"))
        }
        other => ("fail", format!("알 수 없는 명령: {other}")),
    }
}

/// 받은 계정을 기존 계정 스토어에 추가(login_id 중복은 건너뜀). 종토(forum) 가정(§10-2).
fn add_accounts<R: Runtime>(app: &AppHandle<R>, accounts: &[AccountIn]) -> usize {
    let store = app.state::<JsonStore<Account>>();
    let mut added = 0usize;
    store.mutate(|mut list| {
        for a in accounts {
            if list.iter().any(|x| x.login_id == a.login_id) {
                continue; // 중복 건너뜀(import_accounts 검증과 동일 취지)
            }
            list.push(Account {
                id: a.login_id.clone(),
                platform: PlatformId::Forum,
                login_id: a.login_id.clone(),
                pw: a.pw.clone(),
                status: AccountStatus::New,
                status_msg: None,
                last: "—".into(),
                tags: vec![],
            });
            added += 1;
        }
        list
    });
    added
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

/// 선택 로그인(종토) 큐 아이템 1개를 만들어 기존 now 큐에 적재 + 러너 기동.
/// 프론트 `buildLoginNowItem`과 동일 페이로드(plan.login만 채움, platform=naver, useAdb·force).
fn enqueue_login<R: Runtime>(app: &AppHandle<R>, login_ids: &[String]) {
    if login_ids.is_empty() {
        return;
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
        .map(|id| QueueLocation {
            p: PlatformId::Forum,
            name: id.clone(),
            code: None,
        })
        .collect();
    let title = format!("계정 로그인 {}건", login_ids.len());
    let item = QueueNowItem {
        id: format!("agent-login-{}", now_ms()),
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
}

/// 하트비트 루프: 30초마다 현재 공인 IP + online 상태 보고(§4-1).
async fn heartbeat_loop<R: Runtime>(_app: AppHandle<R>) {
    let client = reqwest::Client::new();
    loop {
        if let Some(cfg) = config::load() {
            let ip = crate::auth::fetch_external_ip().await;
            let ip_opt = if ip.starts_with('(') { None } else { Some(ip.as_str()) };
            let _ = net::heartbeat(&client, &cfg.server_url, &cfg.device_token, ip_opt, "online").await;
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

// ===================== Tauri 명령(하위 등록 화면 §6-2) =====================

/// 하위 앱 등록: 서버주소 + 기기코드 → 등록 → 토큰 저장. 성공 시 루프가 자동 연결.
#[tauri::command]
pub async fn agent_register(server_url: String, code: String) -> Result<AgentStatus, String> {
    let base = server_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() || code.trim().is_empty() {
        return Err("서버 주소와 기기코드를 입력하세요".into());
    }
    let client = reqwest::Client::new();
    let resp = net::register(&client, &base, code.trim(), None).await?;
    let device_name = format!("하위-{}", &resp.device_id.chars().take(4).collect::<String>());
    config::save(&AgentConfig {
        server_url: base.clone(),
        device_token: resp.device_token,
        device_name: device_name.clone(),
    })?;
    Ok(AgentStatus {
        configured: true,
        server_url: base,
        device_name,
    })
}

/// 현재 등록 상태 조회.
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

/// 등록 해제(설정 삭제). 서버 쪽 기기 삭제는 Admin이 별도로 수행(§6-4).
#[tauri::command]
pub fn agent_unregister() -> Result<(), String> {
    config::clear()
}
