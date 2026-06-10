//! Dashboard stat tiles (운영 계정 / 예약 대기 / 오늘 게시 / 성공률) — **derived**,
//! not stored. `list_stats` computes the four tiles live from the accounts,
//! queue-scheduled and log-batch stores, so the dashboard stays consistent with
//! the queue and 알림 screens. `value` is a number *or* a formatted string
//! (`"97.4%"`), modelled as an untagged enum so ts-rs emits `number | string`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::{Account, AccountStatus};
use super::log_batches::{BatchItemStatus, LogBatch};
use super::posts::ModeValue;
use super::queue::QueueScheduledItem;
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

/// Counts a batch toward "today" if it landed within the last 24 hours.
/// NOTE: this is a rolling 24h window, deliberately simpler than the frontend
/// `dayBucket` (which uses local calendar-midnight). They can differ near
/// midnight; acceptable for a soft dashboard count, and avoids a TZ dependency.
fn is_today(at: i64) -> bool {
    let now = crate::util::now_ms();
    // Range-check guards against future-dated batches (clock skew): a future `at`
    // makes `now - at` negative, which would otherwise pass the upper bound.
    (0..86_400_000).contains(&(now - at))
}

/// 대시보드 "오류" 타일에 집계할 문제 상태. 사용자 조치가 필요한 실패 계열(비번오류·차단·
/// 기타 오류)을 포함한다. `challenge`(추가 인증 진행 중)는 일시 단계라 제외한다.
fn is_problem_status(s: &AccountStatus) -> bool {
    matches!(
        s,
        AccountStatus::Error | AccountStatus::BadCredentials | AccountStatus::Blocked
    )
}

/// Derive the four dashboard tiles from the live domain data.
pub fn compute(
    accounts: &[Account],
    scheduled: &[QueueScheduledItem],
    batches: &[LogBatch],
) -> Vec<DashStat> {
    let active = accounts
        .iter()
        .filter(|a| a.status == AccountStatus::Active)
        .count();
    let errors = accounts
        .iter()
        .filter(|a| is_problem_status(&a.status))
        .count();

    let next = scheduled
        .first()
        .map(|s| format!("다음 게시 {}", s.rel))
        .unwrap_or_else(|| "예약 없음".into());

    // Today's completed publications (each successful destination) + today's
    // success rate over *resolved* items (success or fail; skip running/대기).
    let (mut posts_done, mut comments_done) = (0u32, 0u32);
    let (mut ok, mut resolved) = (0u32, 0u32);
    for b in batches.iter().filter(|b| is_today(b.at)) {
        for item in &b.items {
            match item.status {
                BatchItemStatus::Success => {
                    ok += 1;
                    resolved += 1;
                    match b.kind {
                        ModeValue::Comment => comments_done += 1,
                        _ => posts_done += 1,
                    }
                }
                BatchItemStatus::Fail => resolved += 1,
                BatchItemStatus::Running | BatchItemStatus::Waiting => {}
            }
        }
    }
    let today_done = posts_done + comments_done;
    let rate = if resolved == 0 {
        100.0
    } else {
        (ok as f64) / (resolved as f64) * 100.0
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
            "오늘 기준",
            "checkCircle",
            "forum",
        ),
    ]
}

#[tauri::command]
pub fn list_stats(
    accounts: tauri::State<'_, JsonStore<Account>>,
    scheduled: tauri::State<'_, JsonStore<QueueScheduledItem>>,
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
    use super::super::log_batches::BatchItem;
    use super::super::queue::QueueLocation;
    use super::*;

    fn acc(status: AccountStatus) -> Account {
        Account {
            id: "x".into(),
            platform: super::super::accounts::PlatformId::Forum,
            login_id: "u".into(),
            pw: "p".into(),
            status,
            status_msg: None,
            last: "—".into(),
            tags: vec![],
        }
    }

    fn item(status: BatchItemStatus) -> BatchItem {
        BatchItem {
            platform: super::super::accounts::PlatformId::Forum,
            target: "t".into(),
            code: None,
            board: None,
            login_id: "u".into(),
            status,
            msg: "".into(),
            trace: None,
        }
    }

    fn batch(kind: ModeValue, at: i64, items: Vec<BatchItem>) -> LogBatch {
        LogBatch {
            id: "b".into(),
            title: "t".into(),
            body: None,
            comment: None,
            kind,
            at,
            state: None,
            items,
        }
    }

    // A "today" timestamp: 30 minutes ago
    fn today_at() -> i64 {
        crate::util::now_ms() - 30 * 60_000
    }

    // A "yesterday" timestamp: 25 hours ago
    fn yesterday_at() -> i64 {
        crate::util::now_ms() - 25 * 3_600_000
    }

    fn sched() -> QueueScheduledItem {
        QueueScheduledItem {
            id: "qs1".into(),
            title: "t".into(),
            kind: ModeValue::Post,
            when: "오늘 18:30".into(),
            rel: "5시간 후".into(),
            at: 1_700_000_000_000,
            missed: false,
            locs: vec![QueueLocation {
                p: super::super::accounts::PlatformId::Forum,
                name: "n".into(),
                code: None,
            }],
            plan: None,
        }
    }

    #[test]
    fn is_today_excludes_old_and_future_dated_batches() {
        let now = crate::util::now_ms();
        assert!(is_today(now - 60_000)); // 1 min ago → today
        assert!(!is_today(now - 25 * 3_600_000)); // 25h ago → not today
        assert!(!is_today(now + 3_600_000)); // 1h in the future (clock skew) → not today
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
        assert_eq!(stats[0].value, StatValue::Num(2.0));
        assert_eq!(stats[0].sub, "전체 4개 · 오류 1");
    }

    #[test]
    fn error_tile_counts_blocked_and_bad_credentials_but_not_challenge() {
        let accounts = vec![
            acc(AccountStatus::Active),
            acc(AccountStatus::Error),
            acc(AccountStatus::BadCredentials),
            acc(AccountStatus::Blocked),
            acc(AccountStatus::Challenge), // 진행 중 — 오류로 세지 않음
        ];
        let stats = compute(&accounts, &[], &[]);
        // 활성 1, 오류 계열 3(error/badCredentials/blocked), challenge 제외.
        assert_eq!(stats[0].value, StatValue::Num(1.0));
        assert_eq!(stats[0].sub, "전체 5개 · 오류 3");
    }

    #[test]
    fn scheduled_uses_queue_count_and_next() {
        let stats = compute(&[], &[sched()], &[]);
        assert_eq!(stats[1].value, StatValue::Num(1.0));
        assert_eq!(stats[1].sub, "다음 게시 5시간 후");
    }

    #[test]
    fn today_counts_successes_by_kind_and_rate_skips_running() {
        use BatchItemStatus::*;
        let batches = vec![
            // today: 2 success posts + 1 running (running ignored in rate)
            batch(
                ModeValue::Post,
                today_at(),
                vec![item(Success), item(Running)],
            ),
            // today: 1 success comment + 1 fail
            batch(
                ModeValue::Comment,
                today_at(),
                vec![item(Success), item(Fail)],
            ),
            // yesterday: ignored entirely
            batch(ModeValue::Post, yesterday_at(), vec![item(Success)]),
        ];
        let stats = compute(&[], &[], &batches);
        // 완료: 1 post + 1 comment = 2 (글 1 · 댓글 1)
        assert_eq!(stats[2].value, StatValue::Num(2.0));
        assert_eq!(stats[2].sub, "글 1 · 댓글 1");
        // rate: 2 success / 3 resolved (2 success + 1 fail; running skipped) = 66.7%
        assert_eq!(stats[3].value, StatValue::Text("66.7%".into()));
    }

    #[test]
    fn empty_data_is_well_formed() {
        let stats = compute(&[], &[], &[]);
        assert_eq!(stats.len(), 4);
        assert_eq!(stats[1].sub, "예약 없음");
        assert_eq!(stats[3].value, StatValue::Text("100.0%".into()));
    }
}
