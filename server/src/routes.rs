//! HTTP 라우트 — 설계 §10 API 구현. Admin 웹용 + 에이전트(하위)용.
use std::convert::Infallible;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures::Stream;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::state::AppState;
use crate::{crypto, jwt};

pub fn build_router(state: AppState) -> Router {
    use tower_http::cors::{Any, CorsLayer};
    // Admin 웹(브라우저)에서 호출하므로 CORS 허용(배포 시 출처 제한 가능).
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    // Admin UI(admin.html) 정적 서빙 — 하위 COM엔 Node가 없으니 서버 exe 가 UI까지 직접 서빙한다.
    // exe 옆 `dist/` 를 서빙하고, 알 수 없는 경로는 admin.html 로 폴백(SPA 라우팅). Edge 로
    // http://localhost:8080 을 열면 Admin 화면이 바로 뜬다. 정적 서빙은 fallback 이라 API 라우트가 우선.
    use tower_http::services::{ServeDir, ServeFile};
    let ui_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("dist")))
        .unwrap_or_else(|| std::path::PathBuf::from("dist"));
    // `/` 는 디렉토리 index(index.html=매크로 메인앱)로 잡지 말고, 무조건 admin.html 로 폴백시킨다
    // (하위 COM 운영자는 Admin 화면만 필요). 에셋(/assets/*)은 ServeDir 가 그대로 서빙한다.
    let ui_service = ServeDir::new(&ui_dir)
        .append_index_html_on_directories(false)
        .fallback(ServeFile::new(ui_dir.join("admin.html")));
    Router::new()
        // 헬스체크(기존 "/" 텍스트). "/" 는 이제 Admin UI(admin.html)가 차지한다.
        .route("/healthz", get(|| async { "pstmacro-server ok" }))
        // ── 인증(§5) ──
        .route("/auth/signup", post(signup))
        .route("/auth/login", post(login))
        .route("/auth/change-password", post(change_password))
        // ── 운영자 관리(§5) ──
        .route("/admin/operators", get(list_operators))
        .route("/admin/operators/:id/approve", post(approve_operator))
        .route("/admin/operators/:id/reject", post(reject_operator))
        .route("/admin/operators/:id", delete(delete_operator))
        .route("/admin/operators/:id/reset-password", post(reset_password))
        // ── 기기(§6) ──
        .route("/admin/device-codes", post(issue_device_code))
        .route("/devices", get(list_devices))
        .route("/devices/:id", delete(delete_device))
        .route("/devices/:id/commands", post(issue_command))
        .route("/admin/publish", post(issue_publish))
        .route("/admin/forum-stocks", get(forum_stocks))
        .route("/admin/scheduled", post(create_scheduled).get(list_scheduled))
        .route("/admin/scheduled/:id", delete(delete_scheduled))
        .route("/devices/:id/inventory", get(device_inventory))
        .route("/devices/:id/queue-state", get(device_queue_state))
        .route(
            "/devices/:id/nickname-remaining",
            post(issue_nickname_query).get(device_nickname_remaining),
        )
        .route("/admin/kill", post(issue_kill))
        .route("/admin/stop-reports", get(list_stop_reports))
        .route("/admin/daily-results", get(list_daily_results))
        // ── 계정 스테이징·분배(§7·§10-3) ──
        .route("/admin/accounts", get(list_accounts))
        .route("/admin/accounts/import", post(import_accounts))
        .route("/admin/accounts/distribute", post(distribute_accounts))
        .route("/admin/accounts/update-meta", post(update_account_meta))
        // ── 통신로그(§10-5) + Admin 실시간 스트림(§3) ──
        .route("/admin/audit-log", get(audit_log))
        .route("/admin/stream", get(admin_stream))
        // ── 에이전트(하위)용(§10) ──
        .route("/device/register", post(register_device))
        .route("/agent/stream", get(agent_stream))
        .route("/agent/heartbeat", post(agent_heartbeat))
        .route("/agent/state", post(agent_state))
        .route("/agent/log", post(agent_log))
        .route("/agent/inventory", post(agent_inventory))
        .route("/agent/nickname-remaining", post(agent_nickname_remaining))
        .route("/agent/queue-state", post(agent_queue_state))
        .route("/agent/stop-report", post(agent_stop_report))
        .route("/agent/commands/:command_id/result", post(command_result))
        .route("/agent/post-report", post(post_report))
        .route("/admin/post-reports", get(list_post_reports))
        .route("/agent/login-report", post(login_report))
        .route("/admin/login-reports", get(list_login_reports))
        .fallback_service(ui_service)
        .layer(cors)
        .with_state(state)
}

// ───────────────────────── 인증 ─────────────────────────

async fn signup(
    State(st): State<AppState>,
    Json(req): Json<SignupReq>,
) -> AppResult<Json<serde_json::Value>> {
    if req.login_id.trim().is_empty() || req.pw.is_empty() {
        return Err(AppError::BadRequest("아이디·비밀번호를 입력하세요".into()));
    }
    // 계정 열거 방지: 이미 있는 아이디여도 같은 응답(존재 여부를 외부에 노출하지 않음).
    // 새 아이디일 때만 실제로 생성한다.
    if st.repo.find_operator(&req.login_id).await?.is_none() {
        let hash = crypto::hash_password(&req.pw).map_err(AppError::Internal)?;
        st.repo
            .create_operator(Operator {
                login_id: req.login_id.clone(),
                pw_hash: hash,
                role: Role::Operator,
                approved: false, // 승인 전 로그인 불가(§5)
                must_change_password: false,
                token_version: 1,
            })
            .await?;
        st.audit("[REGISTER]", "운영자 → 서버", "", &format!("가입 신청: {} (승인 대기)", req.login_id), "info")
            .await;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn login(
    State(st): State<AppState>,
    Json(req): Json<LoginReq>,
) -> AppResult<Json<LoginResp>> {
    let unauthorized = || AppError::Unauthorized("아이디 또는 비밀번호가 올바르지 않습니다".into());
    let op = match st.repo.find_operator(&req.login_id).await? {
        Some(op) => op,
        None => {
            // 계정 열거 방지: 없는 아이디여도 argon2를 한 번 돌려 타이밍을 맞춘다(같은 에러).
            let _ = crypto::verify_password(&req.pw, &st.dummy_pw_hash);
            return Err(unauthorized());
        }
    };
    if !crypto::verify_password(&req.pw, &op.pw_hash) {
        return Err(unauthorized());
    }
    if !op.approved {
        return Err(AppError::Forbidden("승인 대기 중인 계정입니다".into()));
    }
    let token = jwt::issue_operator(
        &st.cfg.jwt_secret,
        &op.login_id,
        op.token_version,
        st.cfg.operator_token_ttl_secs,
    )
    .map_err(AppError::Internal)?;
    Ok(Json(LoginResp {
        token,
        login_id: op.login_id,
        role: op.role,
        must_change_password: op.must_change_password,
    }))
}

async fn change_password(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChangePwReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    if !crypto::verify_password(&req.current_pw, &op.pw_hash) {
        return Err(AppError::BadRequest("현재 비밀번호가 올바르지 않습니다".into()));
    }
    if req.new_pw.len() < 4 {
        return Err(AppError::BadRequest("새 비밀번호가 너무 짧습니다".into()));
    }
    let hash = crypto::hash_password(&req.new_pw).map_err(AppError::Internal)?;
    // must_change=false + 토큰버전 +1(재로그인 강제, §5).
    st.repo.set_operator_password(&op.login_id, &hash, false).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ───────────────────────── 운영자 관리 ─────────────────────────

async fn list_operators(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<OperatorsResp>> {
    st.auth_operator(&headers).await?;
    let all = st.repo.list_operators().await?;
    let operators = all
        .iter()
        .filter(|o| o.approved)
        .map(|o| OperatorDto { login_id: o.login_id.clone(), role: o.role })
        .collect();
    let pending = all
        .iter()
        .filter(|o| !o.approved)
        .map(|o| o.login_id.clone())
        .collect();
    Ok(Json(OperatorsResp { operators, pending }))
}

async fn approve_operator(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    st.auth_operator(&headers).await?; // 승인은 승인된 운영자 누구나(§5)
    st.repo.set_operator_approved(&id, true).await?;
    st.audit("[REGISTER]", "Admin → 서버", "", &format!("운영자 승인: {id}"), "ok").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn reject_operator(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    st.auth_operator(&headers).await?;
    // 거절 = 대기 중인 가입 신청 삭제(승인된 계정은 영향 없음).
    if let Some(op) = st.repo.find_operator(&id).await? {
        if !op.approved {
            st.repo.delete_operator(&id).await?;
        }
    }
    st.audit("[REGISTER]", "Admin → 서버", "", &format!("가입 거절: {id}"), "info").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn delete_operator(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    st.auth_super(&headers).await?; // SuperAdmin 전용(§5)
    let target = st
        .repo
        .find_operator(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 운영자".into()))?;
    if target.role == Role::Super {
        return Err(AppError::Forbidden("SuperAdmin 계정은 삭제할 수 없습니다(§5)".into()));
    }
    st.repo.delete_operator(&id).await?;
    st.audit("[REGISTER]", "Admin → 서버", "", &format!("운영자 삭제: {id}"), "warn").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn reset_password(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<ResetPwReq>,
) -> AppResult<Json<serde_json::Value>> {
    // 사수 확정(PR #324): 하위 운영자 비번은 SuperAdmin이 재설정. SuperAdmin 본인은 종이 보관.
    st.auth_super(&headers).await?;
    let target = st
        .repo
        .find_operator(&id)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 운영자".into()))?;
    if target.role == Role::Super {
        return Err(AppError::Forbidden(
            "SuperAdmin 비번은 재설정 대상이 아닙니다(본인이 직접 변경·종이 보관, §5)".into(),
        ));
    }
    if req.new_pw.len() < 4 {
        return Err(AppError::BadRequest("새 비밀번호가 너무 짧습니다".into()));
    }
    let hash = crypto::hash_password(&req.new_pw).map_err(AppError::Internal)?;
    // 토큰버전 +1 → 그 운영자 옛 토큰 즉시 무효(§5).
    st.repo.set_operator_password(&id, &hash, false).await?;
    st.audit("[REGISTER]", "Admin → 서버", "", &format!("비번 재설정(SuperAdmin): {id}"), "warn").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ───────────────────────── 기기 ─────────────────────────

async fn issue_device_code(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<DeviceCodeResp>> {
    st.auth_operator(&headers).await?;
    let code = crypto::gen_device_code();
    st.repo
        .create_device_code(DeviceCode { code: code.clone(), created_at: Utc::now(), used: false })
        .await?;
    st.audit("[REGISTER]", "Admin → 서버", "", &format!("기기코드 발급: {code} (10분·1회용)"), "info").await;
    Ok(Json(DeviceCodeResp {
        code,
        server_url: st.cfg.public_server_url.clone(), // 배포 전 결정 → None이면 프론트가 안내
        expires_in_secs: st.cfg.device_code_ttl_secs,
    }))
}

fn to_device_dto(d: &Device, timeout_secs: i64) -> DeviceDto {
    // 하트비트 타임아웃 초과면 표시상 offline(§4-1).
    let stale = (Utc::now() - d.last_seen).num_seconds() > timeout_secs;
    let state = if stale && d.state != DeviceState::Offline {
        DeviceState::Offline
    } else {
        d.state
    };
    DeviceDto {
        id: d.id.to_string(),
        name: d.name.clone(),
        connected: state == DeviceState::Online,
        ip: d.ip.clone(),
        last_seen: d.last_seen.to_rfc3339(),
        state,
    }
}

async fn list_devices(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<DeviceDto>>> {
    st.auth_operator(&headers).await?;
    let devices = st.repo.list_devices().await?;
    Ok(Json(
        devices.iter().map(|d| to_device_dto(d, st.cfg.heartbeat_timeout_secs)).collect(),
    ))
}

async fn delete_device(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    st.auth_operator(&headers).await?; // 기기 삭제는 누구나(§6-4)
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let existed = st.repo.delete_device(uid).await?;
    if !existed {
        return Err(AppError::NotFound("없는 기기".into()));
    }
    st.audit("[REGISTER]", "Admin → 서버", &id, &format!("기기 삭제(등록 해제): {id} — 옛 토큰 무효(§6-4)"), "warn").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandReq {
    #[serde(rename = "type")]
    kind: String,
    command_id: Option<String>,
    // 기타 명령(15-기타명령 §2) 페이로드 — 좋아요/싫어요=links×loginIds, 조회수=links×repeats.
    // 다른 명령은 비운다(그 명령들은 SSE에 etc를 안 싣는다).
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    login_ids: Vec<String>,
    #[serde(default)]
    repeats: u32,
}

fn cmd_label(kind: &str) -> &'static str {
    match kind {
        "import_then_login_all" => "전체로그인",
        "distribute_accounts" => "계정 분배",
        "publish_posts" => "게시 명령",
        "delete_accounts" => "계정 삭제",
        // 기타 명령(15-기타명령 §2).
        "like_posts" => "좋아요",
        "dislike_posts" => "싫어요",
        "boost_view" => "조회수",
        "rotate_ip" => "IP 변경",
        _ => "명령",
    }
}

/// 기타 명령(15-기타명령 §2-3)이면 SSE 페이로드에 etc(links/loginIds/repeats)를 싣는다. 나머지
/// 명령(게시·kill·계정 등 전용 경로가 따로 있는 것)은 type/commandId만 내려보낸다(기존 동작).
fn command_payload(req: &CommandReq, cid: &str) -> serde_json::Value {
    match req.kind.as_str() {
        "like_posts" | "dislike_posts" => serde_json::json!({
            "type": req.kind,
            "commandId": cid,
            "etc": { "links": req.links, "loginIds": req.login_ids },
        }),
        "boost_view" => serde_json::json!({
            "type": req.kind,
            "commandId": cid,
            "etc": { "links": req.links, "repeats": req.repeats },
        }),
        _ => serde_json::json!({ "type": req.kind, "commandId": cid }),
    }
}

async fn issue_command(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<CommandReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st.repo.find_device(uid).await?.ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let cid = req.command_id.clone().unwrap_or_else(|| format!("c-{}", Uuid::new_v4()));
    let label = cmd_label(&req.kind);
    // §4-2: online이 아니면 거부(409) + [REJECT] 로그.
    if !AppState::is_commandable(device.state) {
        let reason = match device.state {
            DeviceState::Rotating => "대상 컴퓨터 IP 변경 중(ROTATING·거부코드 409)",
            DeviceState::Reconnecting => "대상 컴퓨터 재연결 중(거부코드 409)",
            _ => "대상 컴퓨터 꺼짐(offline·거부코드 409)",
        };
        st.audit(
            "[REJECT]",
            &format!("Admin → {}", device.name),
            &id,
            &format!(
                "거부: device_name={} 명령={}({}) commandId={cid} 사유={reason} operator={} src=server/src/routes.rs:issue_command",
                device.name, req.kind, label, op.login_id
            ),
            "fail",
        )
        .await;
        return Err(AppError::Conflict(format!("{reason} — 재연결 후 다시 시도")));
    }
    // online → 명령 push(SSE) + [CMD] 로그.
    let payload = command_payload(&req, &cid);
    st.hub.device_push(uid, payload.to_string());
    st.audit(
        "[CMD]",
        &format!("Admin → {}", device.name),
        &id,
        &format!("{}({}) (commandId={cid}, operator={})", req.kind, label, op.login_id),
        "cmd",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "commandId": cid })))
}

// ───────────────────────── 게시 명령(publish_posts, 07-게시명령) ─────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishReq {
    device_id: String,
    #[serde(default)]
    command_id: Option<String>,
    #[serde(flatten)]
    spec: crate::scheduled::PublishSpec,
}

/// 게시 명령 발행(즉시) — **하위 1대당 1묶음**(대원칙 0-1). 서버가 확정한 계정×종목을 그 하위 SSE로
/// 내려보내고, **통신로그에 무엇을·어느 계정에·어느 종목으로 보내는지 원문 전체를 자르지 않고**
/// 남긴다(사용자 지시). 발송·로깅은 예약 게시와 **동일 함수**(`dispatch_publish`)를 재사용한다.
async fn issue_publish(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PublishReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&req.device_id)
        .map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st
        .repo
        .find_device(uid)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let cid = req
        .command_id
        .clone()
        .unwrap_or_else(|| format!("c-{}", Uuid::new_v4()));

    if !AppState::is_commandable(device.state) {
        let reason = match device.state {
            DeviceState::Rotating => "대상 컴퓨터 IP 변경 중(ROTATING·거부코드 409)",
            DeviceState::Reconnecting => "대상 컴퓨터 재연결 중(거부코드 409)",
            _ => "대상 컴퓨터 꺼짐(offline·거부코드 409)",
        };
        st.audit(
            "[REJECT]",
            &format!("Admin → {}", device.name),
            &req.device_id,
            &format!(
                "거부: publish_posts(게시 명령) commandId={cid} 사유={reason} operator={} 글=\"{}\"(postId={})",
                op.login_id, req.spec.post_title, req.spec.post_id
            ),
            "fail",
        )
        .await;
        return Err(AppError::Conflict(format!("{reason} — 재연결 후 다시 시도")));
    }

    crate::scheduled::dispatch_publish(
        &st,
        &device,
        &cid,
        &req.spec,
        &format!("operator={}", op.login_id),
    )
    .await;

    Ok(Json(serde_json::json!({ "ok": true, "commandId": cid })))
}

// ───────────────────────── 중지 명령(kill_publish, 설계서 08 §10) ─────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KillReq {
    device_id: String,
    #[serde(default)]
    command_id: Option<String>,
    /// 특정 큐 1개(Admin "중지 명령" 페이지의 큐 옆 [중지]).
    #[serde(default)]
    queue_id: Option<String>,
    /// 디바이스 전체(하위 옆 [중지]) — 그 하위의 실행/대기 큐 전부.
    #[serde(default)]
    all: bool,
    /// 계정 단위(선택).
    #[serde(default)]
    login_id: Option<String>,
}

/// 중지 명령 발행(설계서 08 §10) — Admin이 고른 대상(큐 1개 / 디바이스 전체 / 계정)을 그 하위
/// SSE로 내려보내 실행 중 게시큐를 완전 종료시킨다. **무엇을 중지하라 했는지 payload 원문 전체를
/// 자르지 않고** 통신로그·서버 로그에 남긴다(Stage5 무필터 로그). 게이트·[REJECT]는 게시 명령과 동일.
async fn issue_kill(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<KillReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&req.device_id)
        .map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st
        .repo
        .find_device(uid)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let cid = req
        .command_id
        .clone()
        .unwrap_or_else(|| format!("c-{}", Uuid::new_v4()));

    if !AppState::is_commandable(device.state) {
        let reason = match device.state {
            DeviceState::Rotating => "대상 컴퓨터 IP 변경 중(ROTATING·거부코드 409)",
            DeviceState::Reconnecting => "대상 컴퓨터 재연결 중(거부코드 409)",
            _ => "대상 컴퓨터 꺼짐(offline·거부코드 409)",
        };
        st.audit(
            "[REJECT]",
            &format!("Admin → {}", device.name),
            &req.device_id,
            &format!(
                "거부: kill_publish(중지) commandId={cid} 사유={reason} operator={} queueId={:?} all={} loginId={:?}",
                op.login_id, req.queue_id, req.all, req.login_id
            ),
            "fail",
        )
        .await;
        return Err(AppError::Conflict(format!("{reason} — 재연결 후 다시 시도")));
    }

    let payload = serde_json::json!({
        "type": "kill_publish",
        "commandId": cid,
        "kill": { "queueId": req.queue_id, "all": req.all, "loginId": req.login_id },
    });
    st.hub.device_push(uid, payload.to_string());
    // 원문 무필터 로그(Stage5): 무엇을 중지하라 했는지 payload 그대로. 하위의 실제 정지 과정은
    // 하위 앱 로그(log-forward)로 이어서 통신로그에 그대로 뜬다.
    st.audit(
        "[CMD]",
        &format!("Admin → {}", device.name),
        &req.device_id,
        &format!(
            "kill_publish(중지) commandId={cid} operator={} payload={payload}",
            op.login_id
        ),
        "cmd",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "commandId": cid })))
}

// ───────────────────────── 예약 게시(07-게시명령 4단계) ─────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateScheduledReq {
    device_id: String,
    #[serde(flatten)]
    spec: crate::scheduled::PublishSpec,
    /// 발송 시각(epoch ms).
    at: i64,
    #[serde(default)]
    detail: String,
}

/// 예약 등록 — 서버가 목록에 보관하고 스케줄러가 시각되면 발송한다. 등록 자체를 통신로그에 남긴다.
async fn create_scheduled(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateScheduledReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&req.device_id)
        .map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st
        .repo
        .find_device(uid)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let id = format!("sch-{}", Uuid::new_v4());
    let item = crate::scheduled::ScheduledPost {
        id: id.clone(),
        device_id: uid,
        device_name: device.name.clone(),
        spec: req.spec,
        at: req.at,
        detail: req.detail,
        created_at: Some(Utc::now().to_rfc3339()),
    };
    st.audit(
        "[예약]",
        &format!("Admin → {}", device.name),
        &req.device_id,
        &format!(
            "예약 등록 id={id} operator={} · 발송예정(at={}) · 글=\"{}\"(postId={}) · 대상={} · 방식={} · 계정×종목: {}",
            op.login_id,
            item.at,
            item.spec.post_title,
            item.spec.post_id,
            item.spec.target_label_or_default(),
            if item.spec.split { "나눠서" } else { "전체" },
            item.spec.detail(),
        ),
        "cmd",
    )
    .await;
    st.scheduled.lock().unwrap().push(item);
    Ok(Json(serde_json::json!({ "ok": true, "id": id })))
}

/// Admin '예약된 글' 목록(발송 시각 오름차순).
async fn list_scheduled(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<crate::scheduled::ScheduledDto>>> {
    st.auth_operator(&headers).await?;
    let mut items: Vec<crate::scheduled::ScheduledDto> = st
        .scheduled
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.to_dto())
        .collect();
    items.sort_by_key(|d| d.at);
    Ok(Json(items))
}

/// 예약 삭제 — **무엇을 지웠는지(글·대상·계정×종목) 원문 전부**를 통신로그에 남긴다(사용자 지시).
async fn delete_scheduled(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let removed = {
        let mut g = st.scheduled.lock().unwrap();
        if let Some(pos) = g.iter().position(|s| s.id == id) {
            Some(g.remove(pos))
        } else {
            None
        }
    };
    match removed {
        Some(item) => {
            st.audit(
                "[예약]",
                &format!("Admin → {}", item.device_name),
                &item.device_id.to_string(),
                &format!(
                    "예약 삭제 id={id} operator={} · 취소된 예약: 하위={} · 발송예정(at={}) · 글=\"{}\"(postId={}) · 대상={} · 방식={} · 계정×종목: {}",
                    op.login_id,
                    item.device_name,
                    item.at,
                    item.spec.post_title,
                    item.spec.post_id,
                    item.spec.target_label_or_default(),
                    if item.spec.split { "나눠서" } else { "전체" },
                    item.spec.detail(),
                ),
                "warn",
            )
            .await;
            Ok(Json(serde_json::json!({ "ok": true })))
        }
        None => Err(AppError::NotFound("없는 예약".into())),
    }
}

// ───────────────────────── 종목 프록시(07-게시명령 2단계) ─────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForumStocksQuery {
    category: String,
    #[serde(default)]
    exchange: Option<String>,
    #[serde(default)]
    market: Option<String>,
    #[serde(default)]
    page: Option<u32>,
}

/// Admin 게시명령 화면 종목 미리보기 — 서버가 네이버 공개 front-api(무쿠키)를 프록시해 실제 종목
/// 목록을 준다. 서버가 종목 코드의 **원천**이다(하위 크롤 아님). 통신로그에는 **네이버 원문 응답
/// 전체를 자르지 않고** 남긴다(사용자 지시: 종목 가져올 때도 원문 전부).
async fn forum_stocks(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ForumStocksQuery>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let category = crate::naver_stocks::Category::parse(&q.category)
        .ok_or_else(|| AppError::BadRequest(format!("알 수 없는 카테고리: {}", q.category)))?;
    let exchange = crate::naver_stocks::Exchange::parse(q.exchange.as_deref().unwrap_or("krx"))
        .ok_or_else(|| AppError::BadRequest("거래소는 krx/nxt".into()))?;
    let market = crate::naver_stocks::Market::parse(q.market.as_deref().unwrap_or("all"))
        .ok_or_else(|| AppError::BadRequest("시장은 all/kospi/kosdaq".into()))?;
    let page = q.page.unwrap_or(1).max(1);

    let client = crate::naver_stocks::NaverStockClient::new();
    let (result, raws) = client.list(category, exchange, market, page).await;

    // ★ 통신로그: 요청 파라미터 + 네이버 원문 응답 전체(각 호출별 URL·status·body 통째로).
    //   성공·실패 관계없이 **자르지 않고** 남긴다(사용자 지시: 종목 가져올 때도 원문 전부).
    let raw_dump = raws
        .iter()
        .map(|r| format!("GET {} → {}\n{}", r.url, r.status, r.body))
        .collect::<Vec<_>>()
        .join("\n---\n");
    let params = format!(
        "category={} exchange={} market={} page={page} operator={}",
        q.category,
        q.exchange.as_deref().unwrap_or("krx"),
        q.market.as_deref().unwrap_or("all"),
        op.login_id,
    );

    match result {
        Ok(pageres) => {
            let picked = pageres
                .stocks
                .iter()
                .map(|s| {
                    format!(
                        "{}{}({})",
                        if s.is_hot_discussion { "🔥" } else { "" },
                        s.name,
                        s.code
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            st.audit(
                "[프록시]",
                "Admin → 네이버",
                "",
                &format!(
                    "forum-stocks {params} · 결과 {}종목(total {}) · 종목=[{}]\n=== 네이버 원문 ===\n{}",
                    pageres.stocks.len(),
                    pageres.total_count,
                    picked,
                    raw_dump
                ),
                "info",
            )
            .await;
            Ok(Json(serde_json::json!({
                "stocks": pageres.stocks,
                "totalCount": pageres.total_count,
                "page": pageres.page,
                "hasNext": pageres.has_next,
            })))
        }
        Err(e) => {
            // 실패도 통신로그에 원문 전부 + 사유를 남긴다.
            st.audit(
                "[프록시]",
                "Admin → 네이버",
                "",
                &format!(
                    "forum-stocks {params} · 실패: {e}\n=== 네이버 원문 ===\n{}",
                    raw_dump
                ),
                "fail",
            )
            .await;
            Err(AppError::Internal(format!("네이버 종목 조회 실패: {e}")))
        }
    }
}

// ───────────────────────── 계정 스테이징·분배 ─────────────────────────

async fn list_accounts(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<AccountDto>>> {
    st.auth_operator(&headers).await?;
    let accts = st.repo.list_staged_accounts().await?;
    // pw는 절대 반환하지 않는다(암호문만 서버 보관, §7).
    Ok(Json(
        accts
            .into_iter()
            .map(|a| AccountDto {
                id: a.id.to_string(),
                login_id: a.login_id,
                platform: a.platform,
            })
            .collect(),
    ))
}

async fn import_accounts(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ImportReq>,
) -> AppResult<Json<ImportResp>> {
    st.auth_operator(&headers).await?;
    let mut staged = Vec::with_capacity(req.accounts.len());
    for a in &req.accounts {
        if a.login_id.trim().is_empty() || a.pw.is_empty() {
            continue; // 빈 행 건너뜀(엑셀 import와 동일 검증, §10-3)
        }
        let cipher = crypto::encrypt(&st.cfg.enc_key, &a.pw).map_err(AppError::Internal)?;
        staged.push(StagedAccount {
            id: Uuid::new_v4(),
            login_id: a.login_id.clone(),
            pw_cipher: cipher,
            platform: a.platform.clone(),
        });
    }
    let (imported, skipped) = st.repo.add_staged_accounts(staged).await?;
    let total = st.repo.list_staged_accounts().await?.len();
    st.audit("[CMD]", "Admin → 서버", "", &format!("계정 스테이징 추가: imported {imported} / skipped {skipped}"), "info").await;
    Ok(Json(ImportResp { imported, skipped, total }))
}

async fn distribute_accounts(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<DistributeReq>,
) -> AppResult<Json<DistributeResp>> {
    let op = st.auth_operator(&headers).await?;
    if req.account_ids.is_empty() || req.device_ids.is_empty() {
        return Err(AppError::BadRequest("계정·컴퓨터를 1개 이상 선택하세요".into()));
    }
    let acct_ids: Vec<Uuid> = req
        .account_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();
    let dev_ids: Vec<Uuid> = req
        .device_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();
    // ★MOVE 전에 대상 하위가 모두 존재 + online인지 검증(§4-2). 하나라도 아니면 거부(409),
    //   스테이징에서 제거하지 않는다 — 도달 못 할 하위로 계정이 사라지는 일 방지.
    for did in &dev_ids {
        match st.repo.find_device(*did).await? {
            None => return Err(AppError::NotFound("없는 기기가 분배 대상에 포함됨".into())),
            Some(d) if !AppState::is_commandable(d.state) => {
                st.audit(
                    "[REJECT]",
                    &format!("Admin → {}", d.name),
                    &did.to_string(),
                    &format!(
                        "거부: device_name={} 명령=distribute_accounts(계정 분배) 사유=대상이 online 아님({:?}·거부코드 409) operator={}",
                        d.name, d.state, op.login_id
                    ),
                    "fail",
                )
                .await;
                return Err(AppError::Conflict(format!(
                    "{} 가 online이 아니라 분배할 수 없습니다 — 재연결 후 다시 시도",
                    d.name
                )));
            }
            Some(_) => {}
        }
    }
    // 균등+랜덤 분배(겹침 없음, §10-3). 대상 하위는 모두 존재·online이 확인됨 → 손실 없음.
    let plan = crate::distribute::distribute(acct_ids.clone(), dev_ids);
    // MOVE: 스테이징에서 제거하며 가져온다(§7).
    let taken = st.repo.take_staged_accounts(&acct_ids).await?;
    let by_id: std::collections::HashMap<Uuid, StagedAccount> =
        taken.into_iter().map(|a| (a.id, a)).collect();

    let mut assignments = Vec::new();
    let mut moved = 0usize;
    for (dev_id, accts) in plan {
        let device = match st.repo.find_device(dev_id).await? {
            Some(d) => d,
            None => continue,
        };
        // 분배 대상 계정 평문 복원(하위로 보내기 직전, 서버 안에서만).
        let mut items = Vec::new();
        let mut log_pairs = Vec::new();
        for aid in &accts {
            if let Some(a) = by_id.get(aid) {
                let pw = crypto::decrypt(&st.cfg.enc_key, &a.pw_cipher).unwrap_or_default();
                log_pairs.push(format!("{}/{}", a.login_id, pw));
                // platform도 함께 내려보낸다 — 하위가 카페(naver)면 등록만 하고 로그인은 건너뛴다.
                items.push(
                    serde_json::json!({ "loginId": a.login_id, "pw": pw, "platform": a.platform }),
                );
            }
        }
        moved += items.len();
        let cid = format!("c-{}", Uuid::new_v4());
        let payload = serde_json::json!({
            "type": "distribute_accounts",
            "commandId": cid,
            "accounts": items,
        });
        st.hub.device_push(dev_id, payload.to_string());
        // §10-5: 분배 명령은 ID/PW 평문·건수·대상 명시(통신 로그 한정).
        st.audit(
            "[CMD]",
            &format!("Admin → {}", device.name),
            &dev_id.to_string(),
            &format!(
                "distribute_accounts(계정 분배) {}건 → {} (commandId={cid}, operator={})",
                items.len(),
                log_pairs.join(", "),
                op.login_id
            ),
            "cmd",
        )
        .await;
        assignments.push(DeviceAssignment {
            device_id: dev_id.to_string(),
            device_name: device.name,
            count: items.len(),
        });
    }
    Ok(Json(DistributeResp { assignments, moved }))
}

/// 계정 상태/플랫폼 원격 편집(14-계정상태-관리 §4) — Admin이 하위 accountRows 폴링본 대비 바뀐 행만
/// 모아 보낸다. 그 하위 SSE로 `update_account_meta`를 내려보내 loginId별 platform·status를 갱신한다.
/// 게이트·[REJECT]·원문 무필터 로그는 게시/중지 명령과 동일. 하위가 갱신하면 다음 inventory 보고에
/// 반영돼 Admin 폴링이 왕복을 닫는다(양방향).
async fn update_account_meta(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AccountMetaReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    if req.updates.is_empty() {
        return Err(AppError::BadRequest("변경할 계정이 없습니다".into()));
    }
    let uid = Uuid::parse_str(&req.device_id)
        .map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st
        .repo
        .find_device(uid)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let cid = req
        .command_id
        .clone()
        .unwrap_or_else(|| format!("c-{}", Uuid::new_v4()));

    if !AppState::is_commandable(device.state) {
        let reason = match device.state {
            DeviceState::Rotating => "대상 컴퓨터 IP 변경 중(ROTATING·거부코드 409)",
            DeviceState::Reconnecting => "대상 컴퓨터 재연결 중(거부코드 409)",
            _ => "대상 컴퓨터 꺼짐(offline·거부코드 409)",
        };
        st.audit(
            "[REJECT]",
            &format!("Admin → {}", device.name),
            &req.device_id,
            &format!(
                "거부: update_account_meta(계정 상태/플랫폼 변경) commandId={cid} 사유={reason} operator={} 건수={}",
                op.login_id,
                req.updates.len()
            ),
            "fail",
        )
        .await;
        return Err(AppError::Conflict(format!("{reason} — 재연결 후 다시 시도")));
    }

    let payload = serde_json::json!({
        "type": "update_account_meta",
        "commandId": cid,
        "accountUpdates": req.updates,
    });
    st.hub.device_push(uid, payload.to_string());
    // 원문 무필터 로그(Stage5): 어떤 계정을 어떤 platform·status로 바꾸라 했는지 payload 그대로.
    st.audit(
        "[CMD]",
        &format!("Admin → {}", device.name),
        &req.device_id,
        &format!(
            "update_account_meta(계정 상태/플랫폼 변경) {}건 commandId={cid} operator={} payload={payload}",
            req.updates.len(),
            op.login_id
        ),
        "cmd",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "commandId": cid })))
}

// ───────────────────────── 통신로그 + Admin 스트림 ─────────────────────────

async fn audit_log(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<AuditDto>>> {
    st.auth_operator(&headers).await?;
    let entries = st.repo.list_audit().await?;
    Ok(Json(
        entries
            .into_iter()
            .map(|e| AuditDto {
                ts: e.ts.to_rfc3339(),
                tag: e.tag,
                dir: e.dir,
                device: e.device,
                msg: e.msg,
                level: e.level,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct TokenQuery {
    token: String,
}

async fn admin_stream(
    State(st): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // 브라우저 EventSource는 헤더를 못 실으므로 토큰을 쿼리로 받는다. 단 헤더 경로와 동일하게
    // 토큰버전·존재·승인까지 검사한다(삭제·강제로그아웃된 운영자가 평문계정 스트림을 계속 듣지 못하게).
    st.auth_operator_token(&q.token).await?;
    let rx = st.hub.admin_subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|res| futures::future::ready(res.ok().map(|s| Ok(Event::default().data(s)))));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ───────────────────────── 에이전트(하위)용 ─────────────────────────

async fn register_device(
    State(st): State<AppState>,
    Json(req): Json<RegisterReq>,
) -> AppResult<Json<RegisterResp>> {
    let ok = st.repo.consume_device_code(&req.code, st.cfg.device_code_ttl_secs).await?;
    if !ok {
        return Err(AppError::BadRequest("유효하지 않거나 만료된 기기코드입니다".into()));
    }
    let id = Uuid::new_v4();
    let name = req.name.unwrap_or_else(|| format!("하위-{}", &id.to_string()[..4]));
    st.repo
        .create_device(Device {
            id,
            name: name.clone(),
            ip: None,
            state: DeviceState::Online,
            last_seen: Utc::now(),
        })
        .await?;
    let token = jwt::issue_device(&st.cfg.jwt_secret, &id.to_string()).map_err(AppError::Internal)?;
    st.audit("[REGISTER]", &format!("{name} → 서버"), &id.to_string(), &format!("기기코드 {} 등록 성공 → 기기토큰 발급 ✅", req.code), "ok").await;
    st.audit("[SSE]", &format!("{name} → 서버"), &id.to_string(), &format!("스트림 연결 준비(device_id={id})"), "info").await;
    Ok(Json(RegisterResp { device_id: id.to_string(), device_token: token }))
}

async fn agent_stream(
    State(st): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let claims = jwt::verify_device(&st.cfg.jwt_secret, &q.token)
        .map_err(|_| AppError::Unauthorized("기기 토큰 무효".into()))?;
    let id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized("토큰 형식 오류".into()))?;
    let device = st.repo.find_device(id).await?.ok_or_else(|| AppError::Unauthorized("등록 해제된 기기(§6-4)".into()))?;
    // re-attach(§4-1): 같은 device_id 채널에 다시 붙는다.
    let rx = st.hub.device_subscribe(id);
    st.repo.touch_device(id, None, DeviceState::Online, Utc::now()).await?;
    st.audit("[SSE]", &format!("{} → 서버", device.name), &id.to_string(), "스트림 연결됨", "ok").await;
    let stream = BroadcastStream::new(rx)
        .filter_map(|res| futures::future::ready(res.ok().map(|s| Ok(Event::default().data(s)))));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn agent_heartbeat(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<HeartbeatReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let state = req.state.unwrap_or(DeviceState::Online);
    st.repo.touch_device(device.id, req.ip.clone(), state, Utc::now()).await?;
    st.audit(
        "[HEARTBEAT]",
        &format!("{} → 서버", device.name),
        &device.id.to_string(),
        &format!("online · IP {}", req.ip.clone().unwrap_or_else(|| "—".into())),
        "info",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn agent_state(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<StateReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    st.repo.set_device_state(device.id, req.state).await?;
    let msg = match req.state {
        DeviceState::Rotating => format!("ROTATING — IP 회전 시작 (기존 IP {})", device.ip.clone().unwrap_or_else(|| "—".into())),
        DeviceState::Online => "online (재연결·준비 완료)".to_string(),
        DeviceState::Reconnecting => "재연결 중…".to_string(),
        DeviceState::Offline => "offline".to_string(),
    };
    st.audit("[STATE]", &format!("{} → 서버", device.name), &device.id.to_string(), &msg, "warn").await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn command_result(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(command_id): Path<String>,
    Json(req): Json<CommandResultReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let level = req.level.unwrap_or_else(|| "info".into());
    st.audit(
        "[RESULT]",
        &format!("{} → Admin", device.name),
        &device.id.to_string(),
        &format!("{command_id} {}", req.msg),
        &level,
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ───────────────────────── 게시 결과 보고(§10-4-2) ─────────────────────────

/// 하위 에이전트 → 게시 결과 보고. 하위가 만든 로컬 게시 완료 로그(`LogBatch`)를 그대로 받아
/// device 컨텍스트를 붙여 보관(보고 사본) + 통신 로그에 요약 1줄(§10-5).
async fn post_report(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PostReportReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let total = req.items.len();
    let ok = req.items.iter().filter(|i| i.status == "success").count();
    let report = PostReport {
        device_id: device.id,
        device_name: device.name.clone(),
        batch_id: req.id.clone(),
        title: req.title.clone(),
        at: req.at,
        kind: req.kind.clone(),
        received_at: Utc::now(),
        items: req.items,
    };
    st.repo.add_post_report(report).await?;
    // 통신 로그(§10-5)에도 배치 단위 요약을 남긴다(게시 내용·백트레이스는 결과 보고 화면에서).
    let level = if ok == total {
        "ok"
    } else if ok == 0 {
        "fail"
    } else {
        "info"
    };
    st.audit(
        "[RESULT]",
        &format!("{} → Admin", device.name),
        &device.id.to_string(),
        &format!(
            "{} 결과: '{}' — {ok}/{total}곳 성공 (batch={})",
            req.kind, req.title, req.id
        ),
        level,
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Admin '게시 결과' 탭 — 모든 하위의 게시 결과 보고(최신순).
async fn list_post_reports(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<PostReportDto>>> {
    st.auth_operator(&headers).await?;
    let reports = st.repo.list_post_reports().await?;
    Ok(Json(
        reports
            .into_iter()
            .map(|r| PostReportDto {
                device: r.device_name,
                device_id: r.device_id.to_string(),
                batch_id: r.batch_id,
                title: r.title,
                at: r.at,
                kind: r.kind,
                received_at: r.received_at.to_rfc3339(),
                items: r.items,
            })
            .collect(),
    ))
}

// ───────────────────────── 로그인 결과 보고(§10-4-1) ─────────────────────────

/// 결과 날짜 분류용 오늘 날짜(KST=UTC+9, YYYY-MM-DD). 하위·운영자 모두 한국이라 서버에서 KST로
/// 버킷팅한다(자정 근처 UTC 오분류 방지).
fn kst_date() -> String {
    (Utc::now() + chrono::Duration::hours(9))
        .format("%Y-%m-%d")
        .to_string()
}

/// 하위 에이전트 → 로그인 결과 보고. 4분류 + 누적을 device당 최신으로 보관 + 통신 로그 요약 1줄.
async fn login_report(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginReportReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let b = &req.batch;
    let summary = format!(
        "등록 {}건(로그인 대상 {}건) · 로그인 결과: 성공 {} / 보류 {} / 대기초과 {} / 실패 {}{}",
        req.registered,
        req.registered_visible,
        b.success,
        b.onhold.len(),
        b.timedout.len(),
        b.failed.len(),
        req.command_id
            .as_deref()
            .map(|c| format!(" (commandId={c})"))
            .unwrap_or_default(),
    );
    let level = if b.failed.is_empty() { "ok" } else { "fail" };
    // 날짜별 분류(KST): 이 배치 4분류를 그 날 버킷에 합산한다(결과보고 날짜 선택용).
    st.add_login_daily(device.id, &kst_date(), b);
    let report = LoginReport {
        device_id: device.id,
        device_name: device.name.clone(),
        batch: req.batch,
        cumulative: req.cumulative,
        registered: req.registered,
        registered_visible: req.registered_visible,
        received_at: Utc::now(),
    };
    st.repo.add_login_report(report).await?;
    st.audit("[RESULT]", &format!("{} → Admin", device.name), &device.id.to_string(), &summary, level)
        .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// 하위 앱 로그 수신(#324). 하위 에이전트가 링버퍼에서 꺼내 올린 앱 tracing 로그 줄들을 그대로
/// 감사로그에 실어 Admin 통신로그 창에 하위의 실제 로그(네이버 원문 응답·게시/로그인 등)를 보여준다.
/// 줄 텍스트에서 레벨(WARN/ERROR)을 읽어 색을 맞춘다.
async fn agent_log(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AgentLogReq>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let dir = format!("{} 로컬", device.name);
    let dev_id = device.id.to_string();
    for line in &req.lines {
        let level = if line.contains(" ERROR ") {
            "fail"
        } else if line.contains(" WARN ") {
            "warn"
        } else {
            "info"
        };
        st.audit("[하위로그]", &dir, &dev_id, line, level).await;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct AgentLogReq {
    #[serde(default)]
    lines: Vec<String>,
}

/// 하위 인벤토리 보고(07-게시명령 3단계). 하위가 자기 글목록(LibraryPost)·성공(Active)계정을
/// 주기적으로 올린다 → Admin 게시명령 화면이 실데이터로 렌더. 메모리에 최신 1건만 둔다.
/// **바뀌었을 때만** 통신로그에 원문(글 제목·계정 loginId 전부)을 남긴다(주기 보고 도배 방지).
async fn agent_inventory(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(mut inv): Json<DeviceInventory>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    inv.received_at = Some(Utc::now().to_rfc3339());
    let posts_dump = inv
        .posts
        .iter()
        .map(|p| format!("{}({})", p.title, p.id))
        .collect::<Vec<_>>()
        .join(", ");
    let accts_dump = inv.accounts.join(", ");
    let changed = st.set_inventory(device.id, inv.clone());
    if changed {
        st.audit(
            "[인벤토리]",
            &format!("{} → 서버", device.name),
            &device.id.to_string(),
            &format!(
                "글목록 {}건·성공계정 {}명 갱신 · 글=[{}] · 계정=[{}]",
                inv.posts.len(),
                inv.accounts.len(),
                posts_dump,
                accts_dump
            ),
            "info",
        )
        .await;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Admin 게시명령 화면 — 특정 하위의 글목록·성공계정(실데이터). 아직 보고 전이면 빈 목록.
async fn device_inventory(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<DeviceInventory>> {
    st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    Ok(Json(st.get_inventory(uid).unwrap_or(DeviceInventory {
        posts: vec![],
        accounts: vec![],
        account_rows: vec![],
        received_at: None,
    })))
}

// ───────────── 닉네임 잔여 횟수 실시간 조회(15-기타명령 §3·§6-2) ─────────────
// Admin이 닉네임 랜덤 체크박스를 켜면 선택한 종토 계정들의 remainingEditCount를 실시간으로 왕복
// 조회한다: Admin POST → 서버가 하위 SSE로 query_nickname_remaining 발송 → 하위가 계정별
// forum_nickname_remaining 조회 후 /agent/nickname-remaining 회신 → 서버 메모리 보관 → Admin GET 폴링.

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NicknameQueryReq {
    #[serde(default)]
    command_id: Option<String>,
    #[serde(default)]
    login_ids: Vec<String>,
}

/// Admin → 서버: 닉네임 잔여 조회 요청. online이 아니면 거부(409). 하위 SSE로 조회 명령을 내려보낸다.
async fn issue_nickname_query(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<NicknameQueryReq>,
) -> AppResult<Json<serde_json::Value>> {
    let op = st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    let device = st
        .repo
        .find_device(uid)
        .await?
        .ok_or_else(|| AppError::NotFound("없는 기기".into()))?;
    let cid = req
        .command_id
        .clone()
        .unwrap_or_else(|| format!("c-{}", Uuid::new_v4()));
    if !AppState::is_commandable(device.state) {
        return Err(AppError::Conflict(
            "대상 컴퓨터가 online 이 아닙니다 — 재연결 후 다시 시도".into(),
        ));
    }
    let payload = serde_json::json!({
        "type": "query_nickname_remaining",
        "commandId": cid,
        "nicknameQuery": { "loginIds": req.login_ids },
    });
    st.hub.device_push(uid, payload.to_string());
    st.audit(
        "[CMD]",
        &format!("Admin → {}", device.name),
        &id,
        &format!(
            "query_nickname_remaining(닉네임 잔여 조회) commandId={cid} 계정 {}건 operator={}",
            req.login_ids.len(),
            op.login_id
        ),
        "cmd",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "commandId": cid })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NicknameRemainingEntry {
    login_id: String,
    #[serde(default)]
    remaining: Option<i64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NicknameRemainingReport {
    #[serde(default)]
    results: Vec<NicknameRemainingEntry>,
}

/// 하위 → 서버: 닉네임 잔여 조회 회신. device당 loginId→(남은횟수|null) 맵에 병합 보관한다.
/// 주기 보고가 아니라 온디맨드라 통신로그엔 남기지 않는다(폴링 응답과 동일).
async fn agent_nickname_remaining(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<NicknameRemainingReport>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    let entries: Vec<(String, Option<i64>)> = req
        .results
        .into_iter()
        .map(|e| (e.login_id, e.remaining))
        .collect();
    st.set_nickname_remaining(device.id, entries);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Admin 게시명령 화면 — 이 하위의 닉네임 잔여 횟수 맵(loginId→남은횟수|null). 아직 회신 전이면 빈 맵.
async fn device_nickname_remaining(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<std::collections::HashMap<String, Option<i64>>>> {
    st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    Ok(Json(st.get_nickname_remaining(uid)))
}

/// 하위 → 서버: 실행/대기 게시큐 스냅샷 보고(설계서 08 §10-2). 주기 보고라 통신로그엔 안 남기고
/// 최신 1건만 보관한다(kill 명령만 원문 로그). Admin "중지 명령" 페이지가 폴링으로 읽는다.
async fn agent_queue_state(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(mut qs): Json<DeviceQueueState>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    qs.received_at = Some(Utc::now().to_rfc3339());
    st.set_queue_state(device.id, qs);
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Admin "중지 명령" 페이지 — 특정 하위의 실행/대기 게시큐(실데이터, 하위 화면과 동일). 아직 보고
/// 전이면 빈 목록.
async fn device_queue_state(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> AppResult<Json<DeviceQueueState>> {
    st.auth_operator(&headers).await?;
    let uid = Uuid::parse_str(&id).map_err(|_| AppError::BadRequest("기기 id 형식 오류".into()))?;
    Ok(Json(st.get_queue_state(uid).unwrap_or_default()))
}

/// 하위 → 서버: 중지(kill) 요약 보고(설계서 08 §10-3). 결과보고 "중지" 섹션 데이터. 요약도 원문
/// 그대로 통신로그에 남긴다(Stage5).
async fn agent_stop_report(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(mut rpt): Json<DeviceStopReport>,
) -> AppResult<Json<serde_json::Value>> {
    let device = st.auth_device(&headers).await?;
    rpt.received_at = Some(Utc::now().to_rfc3339());
    let n = rpt.stopped.len();
    let dump = rpt
        .stopped
        .iter()
        .map(|s| format!("{}({}/{})", s.login_id, s.done, s.total))
        .collect::<Vec<_>>()
        .join(", ");
    // 날짜별 분류(KST): 중지 요약을 그 날 버킷에도 합산한다(결과보고 날짜 선택용).
    st.add_stop_daily(device.id, &kst_date(), &rpt.stopped);
    st.set_stop_report(device.id, rpt);
    st.audit(
        "[중지]",
        &format!("{} → 서버", device.name),
        &device.id.to_string(),
        &format!("중지 요약 {n}건(계정(진행/전체)): {dump}"),
        "warn",
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Admin 결과보고 "중지" 섹션 — 모든 하위의 중지 요약(디바이스 이름 포함, 최신순).
async fn list_stop_reports(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<StopReportDto>>> {
    st.auth_operator(&headers).await?;
    let devices = st.repo.list_devices().await?;
    let name_of = |id: Uuid| {
        devices
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut out: Vec<StopReportDto> = st
        .stop_reports_snapshot()
        .into_iter()
        .map(|(id, r)| StopReportDto {
            device: name_of(id),
            device_id: id.to_string(),
            received_at: r.received_at.unwrap_or_default(),
            stopped: r.stopped,
        })
        .collect();
    out.sort_by(|a, b| b.received_at.cmp(&a.received_at));
    Ok(Json(out))
}

/// Admin 결과보고 날짜 분류 — 모든 하위의 날짜별 결과(로그인 4분류 + 중지). Admin이 하위별로
/// 날짜를 골라 그 날 결과만 보여준다(날짜 섞임 방지).
async fn list_daily_results(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<DeviceDailyDto>>> {
    st.auth_operator(&headers).await?;
    let devices = st.repo.list_devices().await?;
    let name_of = |id: Uuid| {
        devices
            .iter()
            .find(|d| d.id == id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let out: Vec<DeviceDailyDto> = st
        .daily_snapshot()
        .into_iter()
        .map(|(id, days)| DeviceDailyDto {
            device: name_of(id),
            device_id: id.to_string(),
            days,
        })
        .collect();
    Ok(Json(out))
}

/// Admin '로그인 결과' 탭 — 모든 하위의 로그인 결과(컴퓨터당 최신 1건, 최신순).
async fn list_login_reports(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<LoginReportDto>>> {
    st.auth_operator(&headers).await?;
    let reports = st.repo.list_login_reports().await?;
    Ok(Json(
        reports
            .into_iter()
            .map(|r| LoginReportDto {
                device: r.device_name,
                device_id: r.device_id.to_string(),
                received_at: r.received_at.to_rfc3339(),
                batch: r.batch,
                cumulative: r.cumulative,
                registered: r.registered,
                registered_visible: r.registered_visible,
            })
            .collect(),
    ))
}

use futures::StreamExt;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PostReportReq;
    use crate::state::AppState;
    use crate::model::DeviceState;

    fn req(kind: &str, links: Vec<&str>, login_ids: Vec<&str>, repeats: u32) -> CommandReq {
        CommandReq {
            kind: kind.into(),
            command_id: None,
            links: links.into_iter().map(String::from).collect(),
            login_ids: login_ids.into_iter().map(String::from).collect(),
            repeats,
        }
    }

    #[test]
    fn cmd_label_covers_etc_commands() {
        assert_eq!(cmd_label("like_posts"), "좋아요");
        assert_eq!(cmd_label("dislike_posts"), "싫어요");
        assert_eq!(cmd_label("boost_view"), "조회수");
        assert_eq!(cmd_label("rotate_ip"), "IP 변경");
        // 기존 명령은 그대로.
        assert_eq!(cmd_label("publish_posts"), "게시 명령");
        assert_eq!(cmd_label("unknown_thing"), "명령");
    }

    #[test]
    fn command_payload_carries_etc_for_like_and_boost() {
        // 좋아요: links×loginIds를 etc에 싣는다(repeats는 안 실음).
        let p = command_payload(&req("like_posts", vec!["l1", "l2"], vec!["a", "b"], 0), "c-1");
        assert_eq!(p["type"], "like_posts");
        assert_eq!(p["commandId"], "c-1");
        assert_eq!(p["etc"]["links"][1], "l2");
        assert_eq!(p["etc"]["loginIds"][0], "a");
        assert!(p["etc"].get("repeats").is_none());

        // 조회수: links×repeats(계정 없음).
        let p = command_payload(&req("boost_view", vec!["l1"], vec![], 30), "c-2");
        assert_eq!(p["etc"]["repeats"], 30);
        assert!(p["etc"].get("loginIds").is_none());
    }

    #[test]
    fn command_payload_rotate_ip_and_others_have_no_etc() {
        let p = command_payload(&req("rotate_ip", vec![], vec![], 0), "c-3");
        assert_eq!(p["type"], "rotate_ip");
        assert!(p.get("etc").is_none());
        // 기존 명령(전용 경로)도 etc 없이 type/commandId만.
        let p = command_payload(&req("publish_posts", vec![], vec![], 0), "c-4");
        assert!(p.get("etc").is_none());
    }

    #[test]
    fn online_gate_rejects_non_online_states() {
        // §4-2 온라인 게이트: issue_command가 재사용하는 판정. online만 명령 가능.
        assert!(AppState::is_commandable(DeviceState::Online));
        assert!(!AppState::is_commandable(DeviceState::Rotating));
        assert!(!AppState::is_commandable(DeviceState::Reconnecting));
        assert!(!AppState::is_commandable(DeviceState::Offline));
    }

    #[test]
    fn post_report_kind_defaults_to_publish() {
        // 게시 명령은 kind를 안 실으므로 기본 "게시".
        let r: PostReportReq =
            serde_json::from_str(r#"{"id":"b1","title":"10개 게시","at":1,"items":[]}"#).unwrap();
        assert_eq!(r.kind, "게시");
        // 기타 명령은 종류 태그를 실어 보낸다.
        let r: PostReportReq = serde_json::from_str(
            r#"{"id":"etc-1","title":"좋아요","at":1,"kind":"좋아요","items":[]}"#,
        )
        .unwrap();
        assert_eq!(r.kind, "좋아요");
    }
}
