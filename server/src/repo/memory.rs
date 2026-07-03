//! In-memory 저장소 — 개발/오프라인 미리보기·테스트용(DATABASE_URL 미설정 시 기본).
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::Repository;
use crate::error::AppResult;
use crate::model::{
    AuditEntry, Device, DeviceCode, DeviceState, LoginReport, Operator, PostReport, Role,
    StagedAccount,
};

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
}
