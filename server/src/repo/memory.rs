//! In-memory 저장소 — 개발/오프라인 미리보기·테스트용(DATABASE_URL 미설정 시 기본).
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::Repository;
use crate::error::AppResult;
use crate::model::{
    AuditEntry, DailyResultDto, Device, DeviceCode, DeviceState, DeviceStopReport, LoginReport,
    Operator, PostReport, Role, StagedAccount,
};
use crate::scheduled::ScheduledPost;

/// 게시 결과 보고 누적 상한(감사로그처럼 무한 증가 방지).
const MAX_POST_REPORTS: usize = 1000;

#[derive(Default)]
struct Inner {
    operators: HashMap<String, Operator>,
    devices: HashMap<Uuid, Device>,
    codes: HashMap<String, DeviceCode>,
    accounts: HashMap<Uuid, StagedAccount>,
    audit: Vec<AuditEntry>,
    post_reports: Vec<PostReport>,
    login_reports: HashMap<Uuid, LoginReport>, // device_id당 최신 1건
    scheduled: Vec<ScheduledPost>,             // 예약 게시(id 유일)
    stop_reports: HashMap<Uuid, DeviceStopReport>, // device_id당 누적 1건
    daily_results: HashMap<(Uuid, String), DailyResultDto>, // (device_id, date)당 1건
}

#[derive(Default)]
pub struct MemoryRepo {
    inner: Mutex<Inner>,
}

impl MemoryRepo {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Repository for MemoryRepo {
    async fn create_operator(&self, op: Operator) -> AppResult<()> {
        self.inner.lock().unwrap().operators.insert(op.login_id.clone(), op);
        Ok(())
    }
    async fn find_operator(&self, login_id: &str) -> AppResult<Option<Operator>> {
        Ok(self.inner.lock().unwrap().operators.get(login_id).cloned())
    }
    async fn list_operators(&self) -> AppResult<Vec<Operator>> {
        Ok(self.inner.lock().unwrap().operators.values().cloned().collect())
    }
    async fn set_operator_approved(&self, login_id: &str, approved: bool) -> AppResult<()> {
        if let Some(o) = self.inner.lock().unwrap().operators.get_mut(login_id) {
            o.approved = approved;
        }
        Ok(())
    }
    async fn delete_operator(&self, login_id: &str) -> AppResult<()> {
        self.inner.lock().unwrap().operators.remove(login_id);
        Ok(())
    }
    async fn set_operator_password(
        &self,
        login_id: &str,
        new_hash: &str,
        must_change: bool,
    ) -> AppResult<()> {
        if let Some(o) = self.inner.lock().unwrap().operators.get_mut(login_id) {
            o.pw_hash = new_hash.to_string();
            o.token_version += 1; // 옛 토큰 무효(§5)
            o.must_change_password = must_change;
        }
        Ok(())
    }
    async fn count_super_admins(&self) -> AppResult<usize> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .operators
            .values()
            .filter(|o| o.role == Role::Super)
            .count())
    }

    async fn create_device(&self, device: Device) -> AppResult<()> {
        self.inner.lock().unwrap().devices.insert(device.id, device);
        Ok(())
    }
    async fn find_device(&self, id: Uuid) -> AppResult<Option<Device>> {
        Ok(self.inner.lock().unwrap().devices.get(&id).cloned())
    }
    async fn list_devices(&self) -> AppResult<Vec<Device>> {
        Ok(self.inner.lock().unwrap().devices.values().cloned().collect())
    }
    async fn delete_device(&self, id: Uuid) -> AppResult<bool> {
        Ok(self.inner.lock().unwrap().devices.remove(&id).is_some())
    }
    async fn touch_device(
        &self,
        id: Uuid,
        ip: Option<String>,
        state: DeviceState,
        last_seen: DateTime<Utc>,
    ) -> AppResult<()> {
        if let Some(d) = self.inner.lock().unwrap().devices.get_mut(&id) {
            if ip.is_some() {
                d.ip = ip;
            }
            d.state = state;
            d.last_seen = last_seen;
        }
        Ok(())
    }
    async fn set_device_state(&self, id: Uuid, state: DeviceState) -> AppResult<()> {
        if let Some(d) = self.inner.lock().unwrap().devices.get_mut(&id) {
            d.state = state;
        }
        Ok(())
    }

    async fn create_device_code(&self, code: DeviceCode) -> AppResult<()> {
        self.inner.lock().unwrap().codes.insert(code.code.clone(), code);
        Ok(())
    }
    async fn consume_device_code(&self, code: &str, ttl_secs: i64) -> AppResult<bool> {
        let mut g = self.inner.lock().unwrap();
        if let Some(c) = g.codes.get_mut(code) {
            let age = (Utc::now() - c.created_at).num_seconds();
            if !c.used && age <= ttl_secs {
                c.used = true;
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn add_staged_accounts(
        &self,
        accounts: Vec<StagedAccount>,
    ) -> AppResult<(usize, usize)> {
        let mut g = self.inner.lock().unwrap();
        let existing: std::collections::HashSet<String> =
            g.accounts.values().map(|a| a.login_id.clone()).collect();
        let mut seen = existing;
        let (mut imported, mut skipped) = (0usize, 0usize);
        for a in accounts {
            if seen.contains(&a.login_id) {
                skipped += 1;
                continue;
            }
            seen.insert(a.login_id.clone());
            g.accounts.insert(a.id, a);
            imported += 1;
        }
        Ok((imported, skipped))
    }
    async fn list_staged_accounts(&self) -> AppResult<Vec<StagedAccount>> {
        Ok(self.inner.lock().unwrap().accounts.values().cloned().collect())
    }
    async fn take_staged_accounts(&self, ids: &[Uuid]) -> AppResult<Vec<StagedAccount>> {
        let mut g = self.inner.lock().unwrap();
        Ok(ids.iter().filter_map(|id| g.accounts.remove(id)).collect())
    }
    async fn remove_staged_accounts_by_login_ids(
        &self,
        login_ids: &[String],
    ) -> AppResult<usize> {
        let mut g = self.inner.lock().unwrap();
        let set: std::collections::HashSet<&String> = login_ids.iter().collect();
        let to_remove: Vec<Uuid> = g
            .accounts
            .iter()
            .filter(|(_, a)| set.contains(&a.login_id))
            .map(|(id, _)| *id)
            .collect();
        for id in &to_remove {
            g.accounts.remove(id);
        }
        Ok(to_remove.len())
    }

    async fn add_audit(&self, entry: AuditEntry) -> AppResult<()> {
        self.inner.lock().unwrap().audit.push(entry);
        Ok(())
    }
    async fn list_audit(&self) -> AppResult<Vec<AuditEntry>> {
        let mut v = self.inner.lock().unwrap().audit.clone();
        v.sort_by_key(|e| e.ts);
        Ok(v)
    }

    async fn add_post_report(&self, report: PostReport) -> AppResult<()> {
        let mut g = self.inner.lock().unwrap();
        // (device_id, batch_id) 멱등 — 재보고/재연결로 같은 배치가 또 와도 갱신만.
        if let Some(existing) = g
            .post_reports
            .iter_mut()
            .find(|r| r.device_id == report.device_id && r.batch_id == report.batch_id)
        {
            *existing = report;
        } else {
            g.post_reports.push(report);
            let len = g.post_reports.len();
            if len > MAX_POST_REPORTS {
                g.post_reports.drain(0..len - MAX_POST_REPORTS);
            }
        }
        Ok(())
    }
    async fn list_post_reports(&self) -> AppResult<Vec<PostReport>> {
        let mut v = self.inner.lock().unwrap().post_reports.clone();
        // 최신(received_at) 먼저.
        v.sort_by_key(|r| std::cmp::Reverse(r.received_at));
        Ok(v)
    }

    async fn add_login_report(&self, report: LoginReport) -> AppResult<()> {
        // device_id당 최신 1건으로 덮어쓴다(누적이 합계를 담으므로 배치 이력은 보관 안 함).
        self.inner
            .lock()
            .unwrap()
            .login_reports
            .insert(report.device_id, report);
        Ok(())
    }
    async fn list_login_reports(&self) -> AppResult<Vec<LoginReport>> {
        let mut v: Vec<LoginReport> = self
            .inner
            .lock()
            .unwrap()
            .login_reports
            .values()
            .cloned()
            .collect();
        v.sort_by_key(|r| std::cmp::Reverse(r.received_at));
        Ok(v)
    }

    async fn add_scheduled_post(&self, post: ScheduledPost) -> AppResult<()> {
        let mut g = self.inner.lock().unwrap();
        // 같은 id면 갱신(멱등), 없으면 추가.
        if let Some(existing) = g.scheduled.iter_mut().find(|s| s.id == post.id) {
            *existing = post;
        } else {
            g.scheduled.push(post);
        }
        Ok(())
    }
    async fn list_scheduled_posts(&self) -> AppResult<Vec<ScheduledPost>> {
        let mut v = self.inner.lock().unwrap().scheduled.clone();
        v.sort_by_key(|s| s.at); // 발송 시각 오름차순.
        Ok(v)
    }
    async fn delete_scheduled_post(&self, id: &str) -> AppResult<bool> {
        let mut g = self.inner.lock().unwrap();
        let before = g.scheduled.len();
        g.scheduled.retain(|s| s.id != id);
        Ok(g.scheduled.len() != before)
    }

    async fn upsert_stop_report(
        &self,
        device_id: Uuid,
        report: DeviceStopReport,
    ) -> AppResult<()> {
        // 하위당 누적본을 통째로 최신으로 덮어쓴다(메모리 누적 결과 write-through).
        self.inner.lock().unwrap().stop_reports.insert(device_id, report);
        Ok(())
    }
    async fn list_stop_reports(&self) -> AppResult<Vec<(Uuid, DeviceStopReport)>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .stop_reports
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect())
    }

    async fn upsert_daily_result(
        &self,
        device_id: Uuid,
        date: &str,
        dto: DailyResultDto,
    ) -> AppResult<()> {
        // (device_id, date)당 하루치 집계를 최신으로 덮어쓴다(멱등).
        self.inner
            .lock()
            .unwrap()
            .daily_results
            .insert((device_id, date.to_string()), dto);
        Ok(())
    }
    async fn list_daily_results(&self) -> AppResult<Vec<(Uuid, DailyResultDto)>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .daily_results
            .iter()
            .map(|((id, _), dto)| (*id, dto.clone()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::model::PostItemDto;

    fn report(device: Uuid, batch: &str, title: &str, secs: i64) -> PostReport {
        PostReport {
            device_id: device,
            device_name: "하위-001".into(),
            batch_id: batch.into(),
            title: title.into(),
            at: 1,
            kind: "게시".into(),
            received_at: Utc.timestamp_opt(secs, 0).unwrap(),
            items: vec![PostItemDto {
                platform: "forum".into(),
                target: "삼성전자 종목토론방".into(),
                login_id: "chol_invest".into(),
                status: "success".into(),
                msg: "게시 완료".into(),
                trace: None,
                posted: None,
            }],
        }
    }

    #[tokio::test]
    async fn post_report_dedup_by_device_and_batch() {
        let repo = MemoryRepo::new();
        let dev = Uuid::new_v4();
        // 같은 (device, batch)를 두 번 보고 → 1건만, 마지막 내용으로 갱신.
        repo.add_post_report(report(dev, "lb-q-1", "첫 제목", 10))
            .await
            .unwrap();
        repo.add_post_report(report(dev, "lb-q-1", "갱신 제목", 20))
            .await
            .unwrap();
        let all = repo.list_post_reports().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].title, "갱신 제목");
    }

    #[tokio::test]
    async fn post_report_lists_newest_first() {
        let repo = MemoryRepo::new();
        let dev = Uuid::new_v4();
        repo.add_post_report(report(dev, "lb-q-1", "오래된", 10))
            .await
            .unwrap();
        repo.add_post_report(report(dev, "lb-q-2", "최신", 20))
            .await
            .unwrap();
        let all = repo.list_post_reports().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "최신"); // received_at 큰 것이 앞.
    }

    fn login_report(device: Uuid, success: usize, secs: i64) -> crate::model::LoginReport {
        use crate::model::{LoginBatchDto, LoginCumulativeDto, LoginReport};
        LoginReport {
            device_id: device,
            device_name: "하위-001".into(),
            received_at: Utc.timestamp_opt(secs, 0).unwrap(),
            batch: LoginBatchDto { success, onhold: vec![], timedout: vec![], failed: vec![] },
            cumulative: LoginCumulativeDto::default(),
            registered: success,
            registered_visible: success,
        }
    }

    #[tokio::test]
    async fn login_report_keeps_latest_per_device() {
        let repo = MemoryRepo::new();
        let dev = Uuid::new_v4();
        // 같은 device를 두 번 보고 → 1건만, 최신(나중) 배치로 덮어씀.
        repo.add_login_report(login_report(dev, 1, 10))
            .await
            .unwrap();
        repo.add_login_report(login_report(dev, 5, 20))
            .await
            .unwrap();
        let all = repo.list_login_reports().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].batch.success, 5);
    }

    // ── 예약 게시 영속화 ──
    fn sched(id: &str, at: i64) -> ScheduledPost {
        use crate::scheduled::PublishSpec;
        ScheduledPost {
            id: id.into(),
            device_id: Uuid::nil(),
            device_name: "하위-001".into(),
            spec: PublishSpec {
                post_id: "p1".into(),
                post_title: "글".into(),
                target_label: String::new(),
                split: false,
                mode: String::new(),
                comment_urls: vec![],
                forum_comment_distribute: false,
                target: String::new(),
                cafe_boards: vec![],
                blog_links: vec![],
                clip_links: vec![],
                band_targets: vec![],
                comment_mode: String::new(),
                comment_count: 0,
                comment_nickname_random: false,
                content_change: None,
                assignments: vec![],
            },
            at,
            detail: String::new(),
            created_at: None,
        }
    }

    #[tokio::test]
    async fn scheduled_add_list_roundtrip_sorted_by_at() {
        let repo = MemoryRepo::new();
        // 시각 역순으로 넣어도 list는 at 오름차순.
        repo.add_scheduled_post(sched("c", 300)).await.unwrap();
        repo.add_scheduled_post(sched("a", 100)).await.unwrap();
        repo.add_scheduled_post(sched("b", 200)).await.unwrap();
        let all = repo.list_scheduled_posts().await.unwrap();
        let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "at 오름차순");
    }

    #[tokio::test]
    async fn scheduled_add_same_id_is_idempotent() {
        let repo = MemoryRepo::new();
        repo.add_scheduled_post(sched("x", 100)).await.unwrap();
        repo.add_scheduled_post(sched("x", 500)).await.unwrap(); // 같은 id 재저장
        let all = repo.list_scheduled_posts().await.unwrap();
        assert_eq!(all.len(), 1, "같은 id는 중복 없이 1건");
        assert_eq!(all[0].at, 500, "최신 내용으로 갱신");
    }

    #[tokio::test]
    async fn scheduled_delete_removes_and_reports() {
        let repo = MemoryRepo::new();
        repo.add_scheduled_post(sched("a", 100)).await.unwrap();
        repo.add_scheduled_post(sched("b", 200)).await.unwrap();
        assert!(repo.delete_scheduled_post("a").await.unwrap(), "지운 게 있으면 true");
        assert!(!repo.delete_scheduled_post("a").await.unwrap(), "이미 없으면 false");
        let left: Vec<String> =
            repo.list_scheduled_posts().await.unwrap().into_iter().map(|s| s.id).collect();
        assert_eq!(left, vec!["b".to_string()]);
    }

    // ── 중지 리포트 영속화 ──
    fn stop_report(login: &str, done: u32, total: u32, at: &str) -> DeviceStopReport {
        use crate::model::StopLineDto;
        DeviceStopReport {
            stopped: vec![StopLineDto {
                login_id: login.into(),
                pw: String::new(),
                title: String::new(),
                done,
                total,
            }],
            received_at: Some(at.into()),
        }
    }

    #[tokio::test]
    async fn stop_report_upsert_is_idempotent_per_device() {
        let repo = MemoryRepo::new();
        let dev = Uuid::new_v4();
        repo.upsert_stop_report(dev, stop_report("acc", 1, 3, "t1")).await.unwrap();
        // 같은 device 재UPSERT → 누적본을 통째로 최신으로 덮어씀(중복 행 없음).
        repo.upsert_stop_report(dev, stop_report("acc", 2, 3, "t2")).await.unwrap();
        let all = repo.list_stop_reports().await.unwrap();
        assert_eq!(all.len(), 1, "device당 1건");
        assert_eq!(all[0].0, dev);
        assert_eq!(all[0].1.received_at.as_deref(), Some("t2"));
        assert_eq!(all[0].1.stopped[0].done, 2);
    }

    #[tokio::test]
    async fn stop_report_lists_all_devices() {
        let repo = MemoryRepo::new();
        let d1 = Uuid::new_v4();
        let d2 = Uuid::new_v4();
        repo.upsert_stop_report(d1, stop_report("a", 1, 1, "t")).await.unwrap();
        repo.upsert_stop_report(d2, stop_report("b", 1, 1, "t")).await.unwrap();
        assert_eq!(repo.list_stop_reports().await.unwrap().len(), 2);
    }

    // ── 날짜별 집계 영속화 ──
    fn daily(date: &str, success: usize) -> DailyResultDto {
        DailyResultDto {
            date: date.into(),
            success,
            onhold: vec![],
            timedout: vec![],
            failed: vec![],
            stopped: vec![],
        }
    }

    #[tokio::test]
    async fn daily_upsert_is_idempotent_per_device_and_date() {
        let repo = MemoryRepo::new();
        let dev = Uuid::new_v4();
        repo.upsert_daily_result(dev, "2026-07-13", daily("2026-07-13", 3)).await.unwrap();
        // 같은 (device, date) 재UPSERT → 1건, 최신 내용으로 갱신.
        repo.upsert_daily_result(dev, "2026-07-13", daily("2026-07-13", 9)).await.unwrap();
        // 다른 날짜는 별개 행.
        repo.upsert_daily_result(dev, "2026-07-14", daily("2026-07-14", 1)).await.unwrap();
        let all = repo.list_daily_results().await.unwrap();
        assert_eq!(all.len(), 2, "(device,date) 2건");
        let today = all.iter().find(|(_, d)| d.date == "2026-07-13").unwrap();
        assert_eq!(today.1.success, 9, "최신 내용으로 갱신");
        assert_eq!(today.0, dev);
    }
}
