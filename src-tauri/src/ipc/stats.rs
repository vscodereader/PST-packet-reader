//! Dashboard stat tiles (운영 계정 / 예약 대기 / 오늘 게시 / 성공률) — **derived**,
//! not stored. `list_stats` computes the four tiles live from the accounts,
//! scheduled and log-batch stores, so the dashboard reflects real data as those
//! domains change. The `value` is a number *or* a formatted string (`"97.4%"`),
//! modelled as an untagged enum so ts-rs emits the `number | string` union.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::{Account, AccountStatus};
use super::log_batches::{BatchItemStatus, LogBatch};
use super::posts::ModeValue;
use super::scheduled::Scheduled;
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(untagged)]
pub enum StatValue {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DashStat {
    pub key: String,
    pub label: String,
    pub value: StatValue,
    pub sub: String,
    pub icon: String,
    pub color: String,
}

fn stat(key: &str, label: &str, value: StatValue, sub: &str, icon: &str, color: &str) -> DashStat {
    DashStat {
        key: key.into(),
        label: label.into(),
        value,
        sub: sub.into(),
        icon: icon.into(),
        color: color.into(),
    }
}

/// Mirror the frontend `dayBucket`: treat "오늘 …" and relative recents as today.
fn is_today(time: &str) -> bool {
    time.starts_with("오늘")
        || time.contains("방금")
        || time.contains("분 전")
        || time.contains("시간 전")
}

/// Derive the four dashboard tiles from the live domain data.
pub fn compute(
    accounts: &[Account],
    scheduled: &[Scheduled],
    batches: &[LogBatch],
) -> Vec<DashStat> {
    let active = accounts
        .iter()
        .filter(|a| a.status == AccountStatus::Active)
        .count();
    let errors = accounts
        .iter()
        .filter(|a| a.status == AccountStatus::Error)
        .count();

    let next = scheduled
        .first()
        .map(|s| format!("다음 게시 {}", s.rel))
        .unwrap_or_else(|| "예약 없음".into());

    // Overall success rate (all log items) + today's *fully* successful posts.
    let (mut posts_done, mut comments_done) = (0u32, 0u32);
    let (mut ok, mut total_items) = (0u32, 0u32);
    for b in batches {
        for item in &b.items {
            total_items += 1;
            if item.status == BatchItemStatus::Success {
                ok += 1;
            }
        }
        // "오늘 게시 완료" counts only batches that fully succeeded (no fails).
        let perfect =
            !b.items.is_empty() && b.items.iter().all(|i| i.status == BatchItemStatus::Success);
        if perfect && is_today(&b.time) {
            let n = b.items.len() as u32;
            match b.kind {
                ModeValue::Comment => comments_done += n,
                _ => posts_done += n,
            }
        }
    }
    let today_done = posts_done + comments_done;
    let rate = if total_items == 0 {
        100.0
    } else {
        (ok as f64) / (total_items as f64) * 100.0
    };

    vec![
        stat(
            "accounts",
            "운영 계정",
            StatValue::Num(active as f64),
            &format!("전체 {}개 · 오류 {}", accounts.len(), errors),
            "users",
            "blue",
        ),
        stat(
            "scheduled",
            "예약 대기",
            StatValue::Num(scheduled.len() as f64),
            &next,
            "clock",
            "yellow",
        ),
        stat(
            "today",
            "오늘 게시 완료",
            StatValue::Num(today_done as f64),
            &format!("글 {} · 댓글 {}", posts_done, comments_done),
            "send",
            "green",
        ),
        stat(
            "rate",
            "게시 성공률",
            StatValue::Text(format!("{:.1}%", rate)),
            "최근 로그 기준",
            "checkCircle",
            "forum",
        ),
    ]
}

#[tauri::command]
pub fn list_stats(
    accounts: tauri::State<'_, JsonStore<Account>>,
    scheduled: tauri::State<'_, JsonStore<Scheduled>>,
    log_batches: tauri::State<'_, JsonStore<LogBatch>>,
) -> Vec<DashStat> {
    compute(
        &accounts.snapshot(),
        &scheduled.snapshot(),
        &log_batches.snapshot(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(status: AccountStatus) -> Account {
        Account {
            id: "x".into(),
            platform: super::super::accounts::PlatformId::Forum,
            login_id: "u".into(),
            pw: "p".into(),
            status,
            last: "—".into(),
            tags: vec![],
        }
    }

    #[test]
    fn computes_account_counts() {
        let accounts = vec![
            acc(AccountStatus::Active),
            acc(AccountStatus::Active),
            acc(AccountStatus::Error),
            acc(AccountStatus::New),
        ];
        let stats = compute(&accounts, &[], &[]);
        let a = &stats[0];
        assert_eq!(a.key, "accounts");
        assert_eq!(a.value, StatValue::Num(2.0));
        assert_eq!(a.sub, "전체 4개 · 오류 1");
    }

    #[test]
    fn computes_scheduled_count_and_next() {
        let scheduled = vec![Scheduled {
            id: "s1".into(),
            title: "t".into(),
            accounts: vec![],
            kind: ModeValue::Post,
            when: "오늘 14:00".into(),
            rel: "1시간 후".into(),
        }];
        let stats = compute(&[], &scheduled, &[]);
        assert_eq!(stats[1].value, StatValue::Num(1.0));
        assert_eq!(stats[1].sub, "다음 게시 1시간 후");
    }

    #[test]
    fn empty_data_is_well_formed() {
        let stats = compute(&[], &[], &[]);
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[1].sub, "예약 없음");
        // No items → success rate defaults to 100%.
        assert_eq!(stats[3].value, StatValue::Text("100.0%".into()));
    }

    #[test]
    fn value_serializes_untagged_as_number_or_string() {
        let json = serde_json::to_string(&compute(&[], &[], &[])).unwrap();
        assert!(json.contains("\"value\":0"));
        assert!(json.contains("\"value\":\"100.0%\""));
    }
}
