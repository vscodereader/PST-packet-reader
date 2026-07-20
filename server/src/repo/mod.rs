//! 저장소 추상화. in-memory(개발/오프라인·테스트) + PostgreSQL(운영, 사수 확정) 두 구현이
//! 같은 트레잇을 만족한다. 설계 §9·§13.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::AppResult;
use crate::model::{
    AuditEntry, Device, DeviceCode, DeviceState, LoginReport, Operator, PostReport, StagedAccount,
};

pub mod memory;
pub mod postgres;

pub use memory::MemoryRepo;
pub use postgres::PostgresRepo;

#[async_trait]
pub trait Repository: Send + Sync {
    // ── 운영자(§5) ──
    async fn create_operator(&self, op: Operator) -> AppResult<()>;
    async fn find_operator(&self, login_id: &str) -> AppResult<Option<Operator>>;
    async fn list_operators(&self) -> AppResult<Vec<Operator>>;
    async fn set_operator_approved(&self, login_id: &str, approved: bool) -> AppResult<()>;
    async fn delete_operator(&self, login_id: &str) -> AppResult<()>;
    /// 비번 변경/재설정: 새 해시 저장 + 토큰버전 +1(옛 토큰 무효) + must_change 설정(§5).
    async fn set_operator_password(
        &self,
        login_id: &str,
        new_hash: &str,
        must_change: bool,
    ) -> AppResult<()>;
    async fn count_super_admins(&self) -> AppResult<usize>;

    // ── 기기(§4·§6) ──
    async fn create_device(&self, device: Device) -> AppResult<()>;
    async fn find_device(&self, id: Uuid) -> AppResult<Option<Device>>;
    /// machine_id(기기 고유값, §E)로 기기 조회 — 재설치·재등록에도 같은 기기 인식용. 없으면 None.
    async fn find_device_by_machine_id(&self, machine_id: &str) -> AppResult<Option<Device>>;
    async fn list_devices(&self) -> AppResult<Vec<Device>>;
    async fn delete_device(&self, id: Uuid) -> AppResult<bool>;
    /// 기기 이름 갱신(재등록 시 최신 컴퓨터 이름 반영, §E).
    async fn set_device_name(&self, id: Uuid, name: &str) -> AppResult<()>;
    async fn touch_device(
        &self,
        id: Uuid,
        ip: Option<String>,
        state: DeviceState,
        last_seen: DateTime<Utc>,
    ) -> AppResult<()>;
    async fn set_device_state(&self, id: Uuid, state: DeviceState) -> AppResult<()>;

    // ── 기기코드(§6) ──
    async fn create_device_code(&self, code: DeviceCode) -> AppResult<()>;
    /// 유효한(미사용·미만료) 코드면 used=true로 소비하고 true 반환(1회용, §6).
    async fn consume_device_code(&self, code: &str, ttl_secs: i64) -> AppResult<bool>;

    // ── 계정 스테이징(§7) ──
    /// 같은 login_id가 스테이징에 이미 있으면 건너뜀(skipped). (imported, skipped) 반환.
    async fn add_staged_accounts(&self, accounts: Vec<StagedAccount>) -> AppResult<(usize, usize)>;
    async fn list_staged_accounts(&self) -> AppResult<Vec<StagedAccount>>;
    /// 분배(MOVE): 주어진 id들을 제거하고 그 계정들을 반환(§7). 없는 id는 무시.
    async fn take_staged_accounts(&self, ids: &[Uuid]) -> AppResult<Vec<StagedAccount>>;
    /// 계정 삭제(휴지통): 주어진 login_id들을 스테이징에서 제거하고 제거된 개수를 반환. 없는 것은 무시.
    async fn remove_staged_accounts_by_login_ids(&self, login_ids: &[String]) -> AppResult<usize>;

    // ── 감사로그(§10-5) ──
    async fn add_audit(&self, entry: AuditEntry) -> AppResult<()>;
    async fn list_audit(&self) -> AppResult<Vec<AuditEntry>>;

    // ── 게시 결과 보고(§10-4-2) ──
    /// 게시 결과 보고 1건 저장. 같은 (device_id, batch_id)는 멱등(재보고해도 중복 안 쌓임).
    async fn add_post_report(&self, report: PostReport) -> AppResult<()>;
    async fn list_post_reports(&self) -> AppResult<Vec<PostReport>>;

    // ── 로그인 결과 보고(§10-4-1) ──
    /// 로그인 결과 보고 저장. 컴퓨터(device_id)당 **최신 1건**으로 덮어쓴다(누적이 합계를 담음).
    async fn add_login_report(&self, report: LoginReport) -> AppResult<()>;
    async fn list_login_reports(&self) -> AppResult<Vec<LoginReport>>;
}
