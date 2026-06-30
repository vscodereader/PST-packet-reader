//! 도메인 타입 + API DTO. 프론트(`src/admin`) 데이터 모양과 맞춘다.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ───────────────────────── 운영자(§5) ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Super,
    Operator,
}

#[derive(Debug, Clone)]
pub struct Operator {
    pub login_id: String,
    pub pw_hash: String,
    pub role: Role,
    pub approved: bool,
    pub must_change_password: bool,
    pub token_version: i64,
}

// ───────────────────────── 기기(§4·§6) ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceState {
    Online,
    Rotating,
    Reconnecting,
    Offline,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: Uuid,
    pub name: String,
    pub ip: Option<String>,
    pub state: DeviceState,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub code: String,
    pub created_at: DateTime<Utc>,
    pub used: bool,
}

// ───────────────────────── 계정 스테이징(§7) ─────────────────────────

/// 스테이징 계정. pw는 항상 AES-256-GCM 암호문으로만 저장(§7).
#[derive(Debug, Clone)]
pub struct StagedAccount {
    pub id: Uuid,
    pub login_id: String,
    pub pw_cipher: String,
}

// ───────────────────────── 감사로그/통신로그(§10-5) ─────────────────────────

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub id: Uuid,
    pub ts: DateTime<Utc>,
    pub tag: String,   // [CMD] [RESULT] [HEARTBEAT] [SSE] [REGISTER] [STATE] [REJECT]
    pub dir: String,   // "Admin → 하위-001" 등
    pub device: String, // 필터용("" = 전체/시스템)
    pub msg: String,
    pub level: String, // cmd | ok | fail | info | warn
}

// ===================== API DTO =====================
// JSON 키는 camelCase(프론트 TS 관례)로 직렬화/역직렬화한다.

// 인증
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignupReq {
    pub login_id: String,
    pub pw: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReq {
    pub login_id: String,
    pub pw: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResp {
    pub token: String,
    pub login_id: String,
    pub role: Role,
    /// 기본 비번 그대로면 true → 프론트가 강제 비번변경 화면으로(§5).
    pub must_change_password: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePwReq {
    pub current_pw: String,
    pub new_pw: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPwReq {
    pub new_pw: String,
}

// 운영자 목록
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDto {
    pub login_id: String,
    pub role: Role,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorsResp {
    pub operators: Vec<OperatorDto>,
    pub pending: Vec<String>,
}

// 기기
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub ip: Option<String>,
    pub last_seen: String,
    pub state: DeviceState,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeResp {
    pub code: String,
    pub server_url: Option<String>, // §6-1: 비면 프론트가 "배포 전 결정" 안내
    pub expires_in_secs: i64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterReq {
    pub code: String,
    pub name: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResp {
    pub device_id: String,
    pub device_token: String,
}

// 계정
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIn {
    pub login_id: String,
    pub pw: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReq {
    pub accounts: Vec<AccountIn>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResp {
    pub imported: usize,
    pub skipped: usize,
    pub total: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub id: String,
    pub login_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributeReq {
    pub account_ids: Vec<String>,
    pub device_ids: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistributeResp {
    /// device_id → 받은 계정 수(균등+랜덤, §10-3).
    pub assignments: Vec<DeviceAssignment>,
    pub moved: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAssignment {
    pub device_id: String,
    pub device_name: String,
    pub count: usize,
}

// 에이전트(하위)용
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatReq {
    pub ip: Option<String>,
    pub state: Option<DeviceState>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateReq {
    pub state: DeviceState,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResultReq {
    pub level: Option<String>, // ok | fail | info
    pub msg: String,
}

// 통신로그
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditDto {
    pub ts: String,
    pub tag: String,
    pub dir: String,
    pub device: String,
    pub msg: String,
    pub level: String,
}

// ───────────────────────── 게시 결과 보고(§10-4-2) ─────────────────────────
// 하위가 자기 로컬 게시 완료 로그(`LogBatch`)를 그대로 올리면(에이전트), Admin '게시 결과'
// 탭이 데스크톱 앱 알림(`notifications.tsx`)과 같은 모델로 렌더한다. 서버는 기록·중계만 하고
// 쿠키·게시큐 같은 원본은 보관하지 않는다(§7).

/// 한 대상에 실제로 게시된 내용(성공 시). `LogBatch.items[].posted`와 동형(camelCase).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostedDto {
    pub title: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 게시 결과 한 줄(= `BatchItem`). 하위가 만든 모양 그대로 받아 그대로 돌려준다. status는
/// 데스크톱 모델의 lowercase 문자열(success/fail/skip/…), 마스킹은 프론트가 표시 시점에.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostItemDto {
    pub platform: String,
    pub target: String,
    pub login_id: String,
    pub status: String,
    pub msg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted: Option<PostedDto>,
}

/// 에이전트 → 서버 게시 결과 보고 본문(= `LogBatch`의 부분집합). 모르는 필드(body/comment/
/// kind/state)는 serde가 무시한다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostReportReq {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub at: i64,
    #[serde(default)]
    pub items: Vec<PostItemDto>,
}

/// 서버 보관용 게시 결과(보고 사본). device 컨텍스트(누가 올렸는지)를 더한다.
#[derive(Debug, Clone)]
pub struct PostReport {
    pub device_id: Uuid,
    pub device_name: String,
    pub batch_id: String,
    pub title: String,
    pub at: i64,
    pub received_at: DateTime<Utc>,
    pub items: Vec<PostItemDto>,
}

/// Admin '게시 결과' 탭 응답(컴퓨터당 배치 카드 1장).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostReportDto {
    pub device: String,
    pub device_id: String,
    pub batch_id: String,
    pub title: String,
    pub at: i64,
    pub received_at: String,
    pub items: Vec<PostItemDto>,
}

// ───────────────────────── 로그인 결과 보고(§10-4-1) ─────────────────────────
// 하위가 자동 전체 로그인 배치를 끝내면 4분류(성공/보류/대기초과/실패) + 누적을 보고한다.
// 화면 표시 규칙(마스킹·정렬·누적)은 프론트가 처리. 성공은 개수만, 나머지는 ID/PW(+사유).

/// 한 계정 줄(보류·대기초과·실패). 성공은 개수만이라 줄이 없다.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginLineDto {
    pub login_id: String,
    pub pw: String,
    /// 보류사유(캡차/전화번호)·실패사유(trace/메시지). 대기초과는 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 이번 배치 분류.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginBatchDto {
    pub success: usize,
    #[serde(default)]
    pub onhold: Vec<LoginLineDto>,
    #[serde(default)]
    pub timedout: Vec<LoginLineDto>,
    #[serde(default)]
    pub failed: Vec<LoginLineDto>,
}

/// 그 하위의 누적 합계(배치마다 갱신).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCumulativeDto {
    pub received: usize,
    pub success: usize,
    pub onhold: usize,
    pub timedout: usize,
    pub failed: usize,
}

/// 에이전트 → 서버 로그인 결과 보고 본문.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReportReq {
    #[serde(default)]
    pub command_id: Option<String>,
    pub batch: LoginBatchDto,
    #[serde(default)]
    pub cumulative: LoginCumulativeDto,
}

/// 서버 보관용 로그인 결과(컴퓨터당 최신 1건 — 누적이 합계를 담으므로).
#[derive(Debug, Clone)]
pub struct LoginReport {
    pub device_id: Uuid,
    pub device_name: String,
    pub batch: LoginBatchDto,
    pub cumulative: LoginCumulativeDto,
    pub received_at: DateTime<Utc>,
}

/// Admin '로그인 결과' 탭 응답(컴퓨터당 카드 1장).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReportDto {
    pub device: String,
    pub device_id: String,
    pub received_at: String,
    pub batch: LoginBatchDto,
    pub cumulative: LoginCumulativeDto,
}
