//! band 로그인 처리 큐(네이버 `auth/queue.rs` 미러). 네이버 큐와 동일한 구조·상태 전이를
//! 가지되, band 쿠키 상태와 `process_band_account`를 사용하고 로그는 `[BAND]` 접두로 남긴다.
//!
//! 큐가 노출하는 타입은 네이버와 동일한 `auth::{QueueJob, QueueJobStatus, QueueStatus}`를
//! 재사용해 ts-rs 바인딩 중복을 피한다.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use tauri::{AppHandle, Manager, Runtime};

use crate::auth::{QueueJob, QueueJobStatus, QueueStatus};

use super::{
    cookies::{account_band_cookie_status, BandCookieStatus},
    process_band_account,
    util::now_millis,
};

#[derive(Default)]
struct BandQueueInner {
    is_running: bool,
    current_account_id: Option<String>,
    jobs: VecDeque<QueueJob>,
    logs: Vec<String>,
}

#[derive(Clone, Default)]
pub struct BandQueueState {
    inner: Arc<Mutex<BandQueueInner>>,
}

// 워커 패닉 시에도 큐를 "실행 중"으로 영구히 묶어두지 않도록, Drop에서 is_running을 내린다.
// 정상 종료 시에는 worker_loop가 미리 is_running=false로 내리므로 이 Drop은 멱등이다.
struct WorkerGuard {
    state: BandQueueState,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.is_running = false;
            inner.current_account_id = None;
        }
    }
}

/// 계정들을 band 처리 큐에 추가하고 필요시 처리를 시작한다.
pub fn enqueue_band_accounts<R: Runtime>(
    state: &BandQueueState,
    app: AppHandle<R>,
    account_ids: Vec<String>,
    headless: bool,
    use_adb: bool,
) -> Result<QueueStatus, crate::auth::OrchestratorError> {
    let should_start = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| crate::auth::OrchestratorError::QueueLock)?;
        for account_id in account_ids {
            inner.jobs.push_back(QueueJob {
                account_id,
                headless,
                use_adb,
                status: QueueJobStatus::Pending,
                message: "queued".to_string(),
                queued_at: now_millis(),
                started_at: None,
                finished_at: None,
            });
        }
        let should_start = !inner.is_running;
        if should_start {
            inner.is_running = true;
        }
        should_start
    };

    if should_start {
        let worker_state = state.clone();
        tauri::async_runtime::spawn(async move {
            worker_loop(worker_state, app).await;
        });
    }

    get_band_queue_status(state)
}

/// 현재 band 큐의 상태를 조회한다.
pub fn get_band_queue_status(
    state: &BandQueueState,
) -> Result<QueueStatus, crate::auth::OrchestratorError> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| crate::auth::OrchestratorError::QueueLock)?;
    Ok(QueueStatus {
        is_running: inner.is_running,
        current_account_id: inner.current_account_id.clone(),
        jobs: inner.jobs.iter().cloned().collect(),
        logs: inner.logs.clone(),
    })
}

async fn worker_loop<R: Runtime>(state: BandQueueState, app: AppHandle<R>) {
    // 워커가 패닉으로 빠져나가도 is_running을 내려 큐가 멈추지 않게 한다.
    let _guard = WorkerGuard {
        state: state.clone(),
    };
    loop {
        let job = {
            let Ok(mut inner) = state.inner.lock() else {
                return;
            };
            let Some(index) = inner
                .jobs
                .iter()
                .position(|job| job.status == QueueJobStatus::Pending)
            else {
                inner.is_running = false;
                inner.current_account_id = None;
                return;
            };

            let mut job = inner.jobs[index].clone();
            job.status = QueueJobStatus::Running;
            job.message = "running".to_string();
            job.started_at = Some(now_millis());
            inner.current_account_id = Some(job.account_id.clone());
            inner.jobs[index] = job.clone();
            push_log(&mut inner, format!("{}: started", job.account_id));
            job
        };

        tracing::info!("[BAND] 로그인 시작 — 계정 {}", job.account_id);

        let cookie_status =
            account_band_cookie_status(&job.account_id).unwrap_or(BandCookieStatus::Missing);
        if cookie_status == BandCookieStatus::Expired {
            if let Ok(mut inner) = state.inner.lock() {
                if let Some(existing) = inner
                    .jobs
                    .iter_mut()
                    .find(|j| j.account_id == job.account_id && j.started_at == job.started_at)
                {
                    existing.message = "expired; refreshing".to_string();
                }
                push_log(&mut inner, format!("{}: cookie expired", job.account_id));
            }
            tracing::info!("[BAND] 쿠키 만료 — 재로그인 진행 (계정 {})", job.account_id);
        }

        let result = process_band_account(&app, &job.account_id, job.headless, job.use_adb).await;
        let message = match &result {
            Ok(()) if cookie_status == BandCookieStatus::Expired => "expired; refreshed".to_string(),
            Ok(()) => "success".to_string(),
            Err(err) => err.to_string(),
        };
        let status = if result.is_ok() && cookie_status == BandCookieStatus::Expired {
            QueueJobStatus::Expired
        } else if result.is_ok() {
            QueueJobStatus::Success
        } else {
            QueueJobStatus::Failed
        };

        match &result {
            Ok(()) => tracing::info!("[BAND] ✅ 로그인 성공 — 계정 {}", job.account_id),
            Err(err) => {
                tracing::info!("[BAND] ❌ 로그인 실패 — 계정 {} ({err})", job.account_id)
            }
        }

        if let Ok(mut inner) = state.inner.lock() {
            if let Some(existing) = inner
                .jobs
                .iter_mut()
                .find(|j| j.account_id == job.account_id && j.started_at == job.started_at)
            {
                existing.status = status.clone();
                existing.message = message.clone();
                existing.finished_at = Some(now_millis());
            }
            push_log(&mut inner, format!("{}: {}", job.account_id, message));
            inner.current_account_id = None;
        }

        // 결과를 activity feed에 기록한다(네이버 큐와 동일).
        {
            use crate::ipc::activity::{record, ActivityItem, ActivityType};
            use crate::store::JsonStore;
            let activity = app.state::<JsonStore<ActivityItem>>();
            let (ty, msg) = match status {
                QueueJobStatus::Success | QueueJobStatus::Expired => (
                    ActivityType::Success,
                    format!("밴드 계정 {} 로그인 성공", job.account_id),
                ),
                _ => (
                    ActivityType::Error,
                    format!("밴드 계정 {} 로그인 실패 — {message}", job.account_id),
                ),
            };
            record(activity.inner(), ty, msg);
        }
    }
}

fn push_log(inner: &mut BandQueueInner, message: String) {
    inner.logs.push(format!("{} {}", now_millis(), message));
    if inner.logs.len() > 200 {
        inner.logs.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_queue_status_transitions_to_running() {
        let state = BandQueueState::default();
        {
            let mut inner = state.inner.lock().unwrap();
            inner.jobs.push_back(QueueJob {
                account_id: "id1".to_string(),
                headless: true,
                use_adb: false,
                status: QueueJobStatus::Pending,
                message: "queued".to_string(),
                queued_at: 1,
                started_at: None,
                finished_at: None,
            });
            inner.is_running = true;
            inner.current_account_id = Some("id1".to_string());
            inner.jobs[0].status = QueueJobStatus::Running;
            inner.jobs[0].started_at = Some(2);
        }
        let status = get_band_queue_status(&state).expect("status should load");

        assert!(status.is_running);
        assert_eq!(status.current_account_id, Some("id1".to_string()));
        assert_eq!(status.jobs[0].status, QueueJobStatus::Running);
        assert!(status.jobs[0].headless);
    }

    #[test]
    fn push_log_caps_history_at_200() {
        let mut inner = BandQueueInner::default();
        for i in 0..205 {
            push_log(&mut inner, format!("event {i}"));
        }
        assert_eq!(inner.logs.len(), 200);
        assert!(inner.logs.last().unwrap().contains("event 204"));
        // 가장 오래된 항목은 밀려났다.
        assert!(!inner.logs.first().unwrap().contains("event 0 "));
    }

    #[test]
    fn worker_guard_clears_running_on_drop() {
        let state = BandQueueState::default();
        {
            let mut inner = state.inner.lock().unwrap();
            inner.is_running = true;
            inner.current_account_id = Some("id1".to_string());
        }
        {
            let _guard = WorkerGuard {
                state: state.clone(),
            };
        }
        let inner = state.inner.lock().unwrap();
        assert!(!inner.is_running);
        assert_eq!(inner.current_account_id, None);
    }
}
