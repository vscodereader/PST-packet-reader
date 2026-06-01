//! Accounts domain — the first slice wired from the React UI to Rust over Tauri IPC.
//!
//! Types here are the single source of truth: `ts-rs` generates the matching
//! TypeScript declarations into `src/shared/bindings/` (run `pnpm gen:bindings`).
//! State is held in memory (seeded on startup) via a managed `AccountStore`; the
//! commands are thin wrappers around the pure `apply_*` functions, which hold all
//! the logic and are unit-tested below.
//!
//! NOTE (PoC scope): persistence is in-memory only — the store resets on app
//! restart. Promoting to a JSON file / SQLite is a follow-up; the command
//! signatures stay identical, so the frontend contract won't change.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Mirrors the TS `PlatformId` literal union. `lowercase` keeps the JSON wire
/// form identical to the existing frontend values (`"forum"`, `"naver"`, …).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum PlatformId {
    Forum,
    Naver,
    Band,
    Instagram,
    Threads,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    New,
    Active,
    Error,
}

/// `camelCase` so field names match the frontend (`loginId`, not `login_id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub platform: PlatformId,
    pub login_id: String,
    pub pw: String,
    pub status: AccountStatus,
    pub last: String,
    pub tags: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure logic (unit-tested) — no IO, no Tauri.
// ---------------------------------------------------------------------------

/// Append a new account to the end of the list.
pub fn apply_add(mut accounts: Vec<Account>, account: Account) -> Vec<Account> {
    accounts.push(account);
    accounts
}

/// Replace the account whose `id` matches; leave the rest untouched.
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

/// Drop every account whose `id` is in `ids`.
pub fn apply_delete(accounts: Vec<Account>, ids: &[String]) -> Vec<Account> {
    accounts
        .into_iter()
        .filter(|a| !ids.contains(&a.id))
        .collect()
}

/// First-run seed, mirroring a slice of the frontend mock data so a fresh
/// launch isn't an empty table.
pub fn seed() -> Vec<Account> {
    vec![
        Account {
            id: "a1".into(),
            platform: PlatformId::Forum,
            login_id: "invest_king7".into(),
            pw: "ik7!naver22".into(),
            status: AccountStatus::Active,
            last: "12분 전".into(),
            tags: vec!["대형주".into(), "반도체".into()],
        },
        Account {
            id: "a2".into(),
            platform: PlatformId::Forum,
            login_id: "value_pick".into(),
            pw: "vp@2024kr".into(),
            status: AccountStatus::Active,
            last: "30분 전".into(),
            tags: vec!["반도체".into()],
        },
        Account {
            id: "a5".into(),
            platform: PlatformId::Naver,
            login_id: "money_lab".into(),
            pw: "mlab2024!!".into(),
            status: AccountStatus::Active,
            last: "3시간 전".into(),
            tags: vec!["분석방".into()],
        },
        Account {
            id: "a6".into(),
            platform: PlatformId::Naver,
            login_id: "stock_daily".into(),
            pw: "daily#stock1".into(),
            status: AccountStatus::New,
            last: "—".into(),
            tags: vec![],
        },
    ]
}

// ---------------------------------------------------------------------------
// Managed state — in-memory store, seeded at startup.
// ---------------------------------------------------------------------------

/// App-managed account list. Register with `Builder::manage(AccountStore::default())`.
pub struct AccountStore(pub Mutex<Vec<Account>>);

impl Default for AccountStore {
    fn default() -> Self {
        AccountStore(Mutex::new(seed()))
    }
}

impl AccountStore {
    fn snapshot(&self) -> Vec<Account> {
        self.0.lock().expect("account store poisoned").clone()
    }

    fn replace_with<F>(&self, f: F) -> Vec<Account>
    where
        F: FnOnce(Vec<Account>) -> Vec<Account>,
    {
        let mut guard = self.0.lock().expect("account store poisoned");
        let next = f(guard.clone());
        *guard = next.clone();
        next
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — each mutation updates the store and returns the full updated
// list so the frontend can replace its state in one step.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(store: tauri::State<'_, AccountStore>) -> Vec<Account> {
    store.snapshot()
}

#[tauri::command]
pub fn add_account(store: tauri::State<'_, AccountStore>, account: Account) -> Vec<Account> {
    store.replace_with(|accounts| apply_add(accounts, account))
}

#[tauri::command]
pub fn update_account(store: tauri::State<'_, AccountStore>, account: Account) -> Vec<Account> {
    store.replace_with(|accounts| apply_update(accounts, account))
}

#[tauri::command]
pub fn delete_accounts(store: tauri::State<'_, AccountStore>, ids: Vec<String>) -> Vec<Account> {
    store.replace_with(|accounts| apply_delete(accounts, &ids))
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
            last: "—".into(),
            tags: vec![],
        }
    }

    #[test]
    fn apply_add_appends_to_end() {
        let start = vec![acct("a1", "one")];
        let next = apply_add(start, acct("a2", "two"));
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
    fn apply_update_is_noop_for_unknown_id() {
        let start = vec![acct("a1", "one")];
        let next = apply_update(start.clone(), acct("zzz", "ghost"));
        assert_eq!(next, start);
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
        let json = serde_json::to_string(&seeded).unwrap();
        let back: Vec<Account> = serde_json::from_str(&json).unwrap();
        assert_eq!(seeded, back);
    }

    #[test]
    fn enums_serialize_as_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&PlatformId::Forum).unwrap(),
            "\"forum\""
        );
        assert_eq!(
            serde_json::to_string(&AccountStatus::Active).unwrap(),
            "\"active\""
        );
    }

    #[test]
    fn account_serializes_with_camelcase_fields() {
        let json = serde_json::to_string(&acct("a1", "u")).unwrap();
        assert!(json.contains("\"loginId\""));
        assert!(!json.contains("login_id"));
    }

    #[test]
    fn store_mutations_persist_across_calls() {
        let store = AccountStore::default();
        let seeded_len = store.snapshot().len();
        let after_add = store.replace_with(|a| apply_add(a, acct("new1", "fresh")));
        assert_eq!(after_add.len(), seeded_len + 1);
        assert_eq!(store.snapshot().len(), seeded_len + 1);
        let after_del = store.replace_with(|a| apply_delete(a, &["new1".into()]));
        assert_eq!(after_del.len(), seeded_len);
    }
}
