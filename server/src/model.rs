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
