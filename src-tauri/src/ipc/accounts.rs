//! Accounts domain — JSON-file-backed store wired to the React UI over Tauri IPC.
//!
//! Types here are the single source of truth: `ts-rs` generates the matching
//! TypeScript declarations into `src/shared/bindings/` (run `pnpm gen:bindings`).
//! State is persisted as JSON via the shared [`JsonStore`]; the commands are thin
//! wrappers around the pure `apply_*` functions, which hold all the logic.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::activity::{record, ActivityType};
use crate::store::JsonStore;

/// Mirrors the TS `PlatformId` literal union. `lowercase` keeps the JSON wire
/// form identical to the existing frontend values (`"forum"`, `"naver"`, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum PlatformId {
    Forum,
    Naver,
    /// 네이버 블로그(#271). 카페와 같은 네이버 쿠키를 재사용하는 댓글 전용 플랫폼.
    Blog,
    Band,
    Instagram,
    Threads,
}

/// 계정의 로그인/활동 상태. `new`/`active`/`error`는 기존과 동일한 와이어 형태를 유지하고
/// (camelCase에서도 단일 단어라 그대로), 로그인 결과를 세분화하는 세 값을 추가한다:
/// `badCredentials`(아이디/비밀번호 오류), `challenge`(캡차·OTP·기기 등 추가 인증 필요),
/// `blocked`(접근 차단). `error`는 그 외(전송 오류/타임아웃/미상)의 catch-all로 남겨,
/// 디스크에 이미 저장된 `"error"` 값과의 하위호환을 보장한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    New,
    Active,
    /// 글 게시에 성공한 뒤의 "대기" 상태(#267-3). 노란 배지로 표시하고, 게시 선택 목록에서는
    /// 숨겨 같은 계정으로 연속 게시되지 않게 한다. 사용자가 상태 배지를 클릭하면 다시 `Active`로
    /// 돌아가 정상 게시에 쓸 수 있다(프론트 STATUS_ACCOUNT_CYCLE).
    Waiting,
    BadCredentials,
    Challenge,
    Blocked,
    Error,
}

/// `camelCase` so field names match the frontend (`loginId`, not `login_id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub platform: PlatformId,
    pub login_id: String,
    pub pw: String,
    pub status: AccountStatus,
    /// 마지막 상태 변경 사유(동결). 로그인 워커가 채운다 — 차단/타임아웃 원문이나 조치
    /// 안내. UI가 배지 tooltip에 보여준다. 과거 JSON엔 없을 수 있어 기본값 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status_msg: Option<String>,
    pub last: String,
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure logic (unit-tested) — no IO, no Tauri.
// ---------------------------------------------------------------------------

pub fn apply_add(mut accounts: Vec<Account>, account: Account) -> Vec<Account> {
    accounts.push(account);
    accounts
}

pub fn apply_update(accounts: Vec<Account>, account: Account) -> Vec<Account> {
    accounts
        .into_iter()
        .map(|a| {
            if a.id == account.id {
                account.clone()
            } else {
                a
            }
        })
        .collect()
}

pub fn apply_delete(accounts: Vec<Account>, ids: &[String]) -> Vec<Account> {
    accounts
        .into_iter()
        .filter(|a| !ids.contains(&a.id))
        .collect()
}

/// 로그인 워커가 결정한 상태/사유를 `login_id`가 일치하는 모든 계정에 반영한다(순수).
/// 키가 `login_id`인 이유: 로그인 잡은 쿠키 키(=loginId)로 식별되며, 같은 loginId를 쓰는
/// 여러 행이 있으면 모두 같은 로그인 결과를 받아야 하기 때문이다(프론트 `pollLogin`과 동일
/// 규약). 매칭이 없으면 원본을 그대로 둔다.
pub fn apply_status_by_login_id(
    accounts: Vec<Account>,
    login_id: &str,
    status: AccountStatus,
    status_msg: Option<String>,
) -> Vec<Account> {
    accounts
        .into_iter()
        .map(|mut a| {
            if a.login_id == login_id {
                a.status = status.clone();
                a.status_msg = status_msg.clone();
            }
            a
        })
        .collect()
}

/// First-run seed, mirroring a slice of the frontend mock data.
pub fn seed() -> Vec<Account> {
    vec![
        Account {
            id: "a1".into(),
            platform: PlatformId::Forum,
            login_id: "invest_king7".into(),
            pw: "ik7!naver22".into(),
            status: AccountStatus::Active,
            status_msg: None,
            last: "12분 전".into(),
            tags: vec!["대형주".into(), "반도체".into()],
        },
        Account {
            id: "a2".into(),
            platform: PlatformId::Forum,
            login_id: "value_pick".into(),
            pw: "vp@2024kr".into(),
            status: AccountStatus::Active,
            status_msg: None,
            last: "30분 전".into(),
            tags: vec!["반도체".into()],
        },
        Account {
            id: "a5".into(),
            platform: PlatformId::Naver,
            login_id: "money_lab".into(),
            pw: "mlab2024!!".into(),
            status: AccountStatus::Active,
            status_msg: None,
            last: "3시간 전".into(),
            tags: vec!["분석방".into()],
        },
        Account {
            id: "a6".into(),
            platform: PlatformId::Naver,
            login_id: "stock_daily".into(),
            pw: "daily#stock1".into(),
            status: AccountStatus::New,
            status_msg: None,
            last: "—".into(),
            tags: vec![],
        },
    ]
}

// ---------------------------------------------------------------------------
// Activity message builders (pure, unit-tested).
// ---------------------------------------------------------------------------

pub fn added_msg(login_id: &str) -> String {
    format!("계정 {login_id} 추가됨")
}
pub fn updated_msg(login_id: &str) -> String {
    format!("계정 {login_id} 수정됨")
}
pub fn deleted_msg(n: usize) -> String {
    format!("계정 {n}건 삭제됨")
}

// ---------------------------------------------------------------------------
// Tauri commands — each mutation persists (via JsonStore) and returns the full
// updated list so the frontend can replace its state in one step.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(store: tauri::State<'_, JsonStore<Account>>) -> Vec<Account> {
    store.snapshot()
}

#[tauri::command]
pub fn add_account(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    account: Account,
) -> Vec<Account> {
    let login = account.login_id.clone();
    let next = store.mutate(|accounts| apply_add(accounts, account));
    record(activity.inner(), ActivityType::Success, added_msg(&login));
    next
}

#[tauri::command]
pub fn update_account(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    account: Account,
) -> Vec<Account> {
    let login = account.login_id.clone();
    let next = store.mutate(|accounts| apply_update(accounts, account));
    record(activity.inner(), ActivityType::Info, updated_msg(&login));
    next
}

#[tauri::command]
pub fn delete_accounts(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    ids: Vec<String>,
) -> Vec<Account> {
    let n = ids.len();
    let next = store.mutate(|accounts| apply_delete(accounts, &ids));
    record(activity.inner(), ActivityType::Info, deleted_msg(n));
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acct(id: &str, login: &str) -> Account {
        Account {
            id: id.into(),
            platform: PlatformId::Forum,
            login_id: login.into(),
            pw: "pw".into(),
            status: AccountStatus::New,
            status_msg: None,
            last: "—".into(),
            tags: vec![],
        }
    }

    #[test]
    fn account_event_messages() {
        assert_eq!(added_msg("invest_king7"), "계정 invest_king7 추가됨");
        assert_eq!(updated_msg("invest_king7"), "계정 invest_king7 수정됨");
        assert_eq!(deleted_msg(3), "계정 3건 삭제됨");
    }

    #[test]
    fn apply_add_appends_to_end() {
        let next = apply_add(vec![acct("a1", "one")], acct("a2", "two"));
        assert_eq!(next.len(), 2);
        assert_eq!(next[1].id, "a2");
    }

    #[test]
    fn apply_update_replaces_only_matching_id() {
        let start = vec![acct("a1", "one"), acct("a2", "two")];
        let mut edited = acct("a2", "renamed");
        edited.status = AccountStatus::Active;
        let next = apply_update(start, edited);
        assert_eq!(next[0].login_id, "one");
        assert_eq!(next[1].login_id, "renamed");
        assert_eq!(next[1].status, AccountStatus::Active);
    }

    #[test]
    fn apply_delete_removes_listed_ids() {
        let start = vec![acct("a1", "one"), acct("a2", "two"), acct("a3", "three")];
        let next = apply_delete(start, &["a1".into(), "a3".into()]);
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].id, "a2");
    }

    #[test]
    fn seed_is_nonempty_and_json_roundtrips() {
        let seeded = seed();
        assert!(!seeded.is_empty());
        let back: Vec<Account> =
            serde_json::from_str(&serde_json::to_string(&seeded).unwrap()).unwrap();
        assert_eq!(seeded, back);
    }

    #[test]
    fn enums_serialize_as_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&PlatformId::Forum).unwrap(),
            "\"forum\""
        );
        // 블로그(#271)는 lowercase 와이어 형태가 "blog".
        assert_eq!(
            serde_json::to_string(&PlatformId::Blog).unwrap(),
            "\"blog\""
        );
        // 기존 3개 값은 camelCase 전환 후에도 단일 단어라 와이어 형태 불변(하위호환).
        assert_eq!(
            serde_json::to_string(&AccountStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::Error).unwrap(),
            "\"error\""
        );
        // 새 다단어 값은 camelCase로 직렬화된다.
        assert_eq!(
            serde_json::to_string(&AccountStatus::BadCredentials).unwrap(),
            "\"badCredentials\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::Challenge).unwrap(),
            "\"challenge\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::Blocked).unwrap(),
            "\"blocked\""
        );
    }

    #[test]
    fn account_serializes_with_camelcase_fields() {
        let json = serde_json::to_string(&acct("a1", "u")).unwrap();
        assert!(json.contains("\"loginId\""));
        assert!(!json.contains("login_id"));
    }

    #[test]
    fn legacy_account_without_status_msg_deserializes() {
        // 구버전 JSON(statusMsg 없음)도 status_msg=None으로 역직렬화된다.
        let json = r#"{"id":"a1","platform":"naver","loginId":"u","pw":"p","status":"error","last":"—","tags":[]}"#;
        let acc: Account = serde_json::from_str(json).unwrap();
        assert_eq!(acc.status, AccountStatus::Error);
        assert_eq!(acc.status_msg, None);
    }

    #[test]
    fn status_msg_omitted_when_none_present_when_set() {
        // None이면 키가 생략돼 기존 JSON과 호환된다.
        assert!(!serde_json::to_string(&acct("a1", "u"))
            .unwrap()
            .contains("statusMsg"));
        // Some이면 camelCase 필드로 직렬화된다.
        let mut a = acct("a1", "u");
        a.status_msg = Some("차단됨".into());
        assert!(serde_json::to_string(&a)
            .unwrap()
            .contains("\"statusMsg\":\"차단됨\""));
    }

    #[test]
    fn apply_status_by_login_id_updates_all_matching_rows() {
        let start = vec![
            acct("r1", "shared"),
            acct("r2", "other"),
            acct("r3", "shared"),
        ];
        let next = apply_status_by_login_id(
            start,
            "shared",
            AccountStatus::Blocked,
            Some("접근 차단".into()),
        );
        // 같은 loginId(shared) 두 행 모두 갱신, 사유도 동결.
        assert_eq!(next[0].status, AccountStatus::Blocked);
        assert_eq!(next[0].status_msg.as_deref(), Some("접근 차단"));
        assert_eq!(next[2].status, AccountStatus::Blocked);
        // 비매칭(other)은 불변.
        assert_eq!(next[1].status, AccountStatus::New);
        assert_eq!(next[1].status_msg, None);
    }

    #[test]
    fn apply_status_by_login_id_no_match_is_noop() {
        let start = vec![acct("r1", "a"), acct("r2", "b")];
        let next = apply_status_by_login_id(start.clone(), "zzz", AccountStatus::Active, None);
        assert_eq!(next, start);
    }
}
