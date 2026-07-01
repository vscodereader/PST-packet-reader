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
    Router::new()
        .route("/", get(|| async { "pstmacro-server ok" }))
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
        // ── 계정 스테이징·분배(§7·§10-3) ──
        .route("/admin/accounts", get(list_accounts))
        .route("/admin/accounts/import", post(import_accounts))
        .route("/admin/accounts/distribute", post(distribute_accounts))
        // ── 통신로그(§10-5) + Admin 실시간 스트림(§3) ──
        .route("/admin/audit-log", get(audit_log))
        .route("/admin/stream", get(admin_stream))
        // ── 에이전트(하위)용(§10) ──
        .route("/device/register", post(register_device))
        .route("/agent/stream", get(agent_stream))
        .route("/agent/heartbeat", post(agent_heartbeat))
        .route("/agent/state", post(agent_state))
        .route("/agent/log", post(agent_log))
        .route("/agent/commands/:command_id/result", post(command_result))
        .route("/agent/post-report", post(post_report))
        .route("/admin/post-reports", get(list_post_reports))
        .route("/agent/login-report", post(login_report))
        .route("/admin/login-reports", get(list_login_reports))
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
struct CommandReq {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "commandId")]
    command_id: Option<String>,
}

fn cmd_label(kind: &str) -> &'static str {
    match kind {
        "import_then_login_all" => "전체로그인",
        "distribute_accounts" => "계정 분배",
        "delete_accounts" => "계정 삭제",
        _ => "명령",
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
    let payload = serde_json::json!({ "type": req.kind, "commandId": cid });
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

// ───────────────────────── 계정 스테이징·분배 ─────────────────────────

async fn list_accounts(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Vec<AccountDto>>> {
    st.auth_operator(&headers).await?;
    let accts = st.repo.list_staged_accounts().await?;
    // pw는 절대 반환하지 않는다(암호문만 서버 보관, §7).
    Ok(Json(
        accts.into_iter().map(|a| AccountDto { id: a.id.to_string(), login_id: a.login_id }).collect(),
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
        staged.push(StagedAccount { id: Uuid::new_v4(), login_id: a.login_id.clone(), pw_cipher: cipher });
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
                items.push(serde_json::json!({ "loginId": a.login_id, "pw": pw }));
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
            "게시 결과: '{}' — {ok}/{total}곳 성공 (batch={})",
            req.title, req.id
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
                received_at: r.received_at.to_rfc3339(),
                items: r.items,
            })
            .collect(),
    ))
}

// ───────────────────────── 로그인 결과 보고(§10-4-1) ─────────────────────────

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
