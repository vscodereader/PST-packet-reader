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
    /// 기기 고유값(Windows MachineGuid 등, §E). 재설치·재등록해도 같은 PC면 동일 → 같은 기기로 인식.
    /// nullable: 옛 앱/옛 row는 None(기존처럼 신규 생성 폴백).
    pub machine_id: Option<String>,
}

/// 하위 인벤토리(글목록·성공계정) — 하위가 주기적으로 보고. Admin 게시명령 화면이 실데이터로 렌더한다.
/// serde(camelCase)로 하위 보고 body와 Admin 응답 DTO를 겸한다(07-게시명령 3단계).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvPost {
    pub id: String,
    pub title: String,
    /// 글 종류: "post"(글)·"comment"(댓글)·"both"(글+댓글). Admin에서 글 종류로 필터한다.
    /// 하위호환: 옛 하위가 안 보내면 기본 "post"(빈값도 post로 본다).
    #[serde(default)]
    pub kind: String,
    /// 댓글 내용 미리보기(LibraryPost.excerpt). 댓글은 제목이 없어(당연) title이 비거나
    /// "제목 없음"이므로, Admin이 이 내용을 제목 대신 보여준다("작성한 댓글 내용"이 보이게).
    /// 글/글+댓글은 제목을 쓰므로 이 값은 무시된다. 옛 하위가 안 보내면 빈 문자열.
    #[serde(default)]
    pub excerpt: String,
    /// 작성한 댓글 수(≥2 게이트용, 15-기타명령 §3·§6-1). Admin이 종토 댓글 글을 고르면 이 값으로
    /// 닉네임 랜덤 체크박스 노출을 판정한다(N≥2일 때만). 옛 하위가 안 보내면 0.
    #[serde(default)]
    pub comment_count: u32,
}

/// 인벤토리 계정 1건(전체 계정 — loginId·platform·status). 카페 게시명령은 로그인 성공/실패
/// 무관 카페(naver) 계정을 전부 노출하므로 상태를 그대로 싣는다. 옛 하위는 안 보낼 수 있어
/// 기본값을 허용한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvAccount {
    pub login_id: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInventory {
    /// 로컬 글 목록(LibraryPost id/title).
    pub posts: Vec<InvPost>,
    /// 로그인 성공(Active) 계정 loginId 목록(종토 게시명령용 — 기존 동작 유지).
    pub accounts: Vec<String>,
    /// 전체 계정(loginId·platform·status) — 카페 게시명령이 로그인 무관 카페 계정을 전부 쓰기 위함.
    /// 옛 하위는 안 보낼 수 있어 기본값(빈 Vec)을 허용한다.
    #[serde(default)]
    pub account_rows: Vec<InvAccount>,
    /// 마지막 보고 시각(Admin 표시용).
    #[serde(default)]
    pub received_at: Option<String>,
}

/// 하위 실행/대기 게시큐 스냅샷(설계서 08 §10-2). 하위가 주기 보고하는 실시간 상태라 인벤토리와
/// 같이 메모리에 최신 1건만 둔다. Admin "중지 명령" 페이지가 이걸 폴링해 하위 게시큐 화면과
/// 동일한 내용을 실시간으로 보여주고, 각 큐 옆 [중지]가 그 큐의 id로 kill을 보낸다.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceQueueState {
    pub items: Vec<QueueItemDto>,
    #[serde(default)]
    pub received_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemDto {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub login_ids: Vec<String>,
}

/// 중지(kill) 요약 1줄(설계서 08 §10-3) — 어느 계정을 "몇 개 중 몇 개 진행 후 중지"했는지.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopLineDto {
    pub login_id: String,
    #[serde(default)]
    pub pw: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub done: u32,
    #[serde(default)]
    pub total: u32,
}

/// 하위 → 서버 중지 리포트(설계서 08 §10-3). 하위가 kill한 큐 요약. 인벤토리처럼 메모리에
/// 디바이스별로 **누적** 보관(결과보고 "중지" 섹션이 렌더).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStopReport {
    pub stopped: Vec<StopLineDto>,
    #[serde(default)]
    pub received_at: Option<String>,
}

/// Admin 결과보고용 중지 리포트(디바이스 이름 포함).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopReportDto {
    pub device: String,
    pub device_id: String,
    pub received_at: String,
    pub stopped: Vec<StopLineDto>,
}

/// 하루치 결과 집계(날짜별 분류) — 그 날(KST)의 로그인 4분류 + 중지를 합산 보관. 결과보고에서
/// 하위별로 날짜를 골라 그 날의 성공/보류/대기초과/실패/중지만 보게 한다(절대 날짜 섞임 없음).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyResultDto {
    pub date: String, // YYYY-MM-DD (KST)
    pub success: usize,
    pub onhold: Vec<LoginLineDto>,
    pub timedout: Vec<LoginLineDto>,
    pub failed: Vec<LoginLineDto>,
    pub stopped: Vec<StopLineDto>,
}

/// 한 하위의 날짜별 결과 목록(최신 날짜 우선).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDailyDto {
    pub device: String,
    pub device_id: String,
    pub days: Vec<DailyResultDto>,
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
    /// 계정 플랫폼("forum"/"naver"/…). 분배 payload로 하위에 전달돼 카페=등록만 판정에 쓰인다.
    /// 빈값=forum(하위호환).
    pub platform: String,
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
    /// 기기 고유값(§E). 있으면 서버가 같은 machine_id 기기를 재사용(upsert). 옛 앱은 미전송(None).
    pub machine_id: Option<String>,
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
    /// 계정 플랫폼("forum"/"naver"/"blog"/"clip"/"band"). 빈값=forum(하위호환). 카페("naver")는
    /// 하위가 분배 시 로그인하지 않고 등록만 한다.
    #[serde(default)]
    pub platform: String,
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
    /// 계정 플랫폼(계정 풀에 배지로 표시). 빈값이면 프론트가 forum으로 본다.
    #[serde(default)]
    pub platform: String,
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
// 계정 상태/플랫폼 원격 편집(14-계정상태-관리 §4). Admin이 하위 accountRows를 폴링해 바꾼 행만
// 모아 보낸다. platform/status는 선택(생략하면 그 필드 유지). status는 하위가 active/waiting/onHold만 허용.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMetaUpdate {
    pub login_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMetaReq {
    pub device_id: String,
    #[serde(default)]
    pub command_id: Option<String>,
    pub updates: Vec<AccountMetaUpdate>,
}
// 계정 삭제(14-계정상태-관리 휴지통). Admin이 고른 loginId들을 Admin(서버 staged)과 그 하위 PC
// 양쪽에서 지운다. 하위엔 delete_accounts 명령이 내려가고, 서버 staged에서도 같은 loginId를 제거한다.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeleteReq {
    pub device_id: String,
    #[serde(default)]
    pub command_id: Option<String>,
    pub login_ids: Vec<String>,
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
    /// 결과 종류 태그(15-기타명령 §6-3) — 게시/좋아요/싫어요/조회수/IP. 게시 명령은 이 필드를
    /// 안 실으므로 기본 "게시"로 본다(하위호환).
    #[serde(default = "default_report_kind")]
    pub kind: String,
    #[serde(default)]
    pub items: Vec<PostItemDto>,
}

/// 결과 보고 종류 기본값(옛 하위/게시 명령 = "게시").
fn default_report_kind() -> String {
    "게시".to_string()
}

/// 서버 보관용 게시 결과(보고 사본). device 컨텍스트(누가 올렸는지)를 더한다.
#[derive(Debug, Clone)]
pub struct PostReport {
    pub device_id: Uuid,
    pub device_name: String,
    pub batch_id: String,
    pub title: String,
    pub at: i64,
    /// 결과 종류 태그(15-기타명령 §6-3) — 게시/좋아요/싫어요/조회수/IP.
    pub kind: String,
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
    /// 결과 종류 태그(15-기타명령 §6-3) — 게시/좋아요/싫어요/조회수/IP.
    pub kind: String,
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
    /// 보류사유(캡차/전화번호)·실패사유(메시지 한 줄). 대기초과는 없음.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 실패 백트레이스(게시 결과의 PostItemDto.trace와 동일 역할) — Admin "자세히 보기"에 노출.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
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
    /// 이 분배에서 새로 *등록*된 계정 수(자동 로그인과 별개로 "등록은 됐는지" 확인용, §10-1).
    #[serde(default)]
    pub registered: usize,
    /// 그중 로그인 엔진이 실제로 볼 수 있는(accounts.json에 반영된) 계정 수 — 0이면 등록은
    /// 됐지만 로그인 대상으로 안 잡힌 것(과거 "account not found" 버그의 신호).
    #[serde(default)]
    pub registered_visible: usize,
}

/// 서버 보관용 로그인 결과(컴퓨터당 최신 1건 — 누적이 합계를 담으므로).
#[derive(Debug, Clone)]
pub struct LoginReport {
    pub device_id: Uuid,
    pub device_name: String,
    pub batch: LoginBatchDto,
    pub cumulative: LoginCumulativeDto,
    pub registered: usize,
    pub registered_visible: usize,
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
    pub registered: usize,
    pub registered_visible: usize,
}
