use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use tauri::{AppHandle, Manager, Runtime};

use super::{
    accounts::{account_cookie_status_for_app_data, CookieStatus},
    error::OrchestratorError,
    process_account,
    types::{QueueJob, QueueJobStatus, QueueStatus},
    util::{mask_id, now_millis},
};

#[derive(Default)]
struct QueueInner {
    is_running: bool,
    current_account_id: Option<String>,
    jobs: VecDeque<QueueJob>,
    logs: Vec<String>,
}

#[derive(Clone, Default)]
pub struct QueueState {
    inner: Arc<Mutex<QueueInner>>,
}

/// 계정들을 처리 큐에 추가하고 필요시 처리를 시작한다.
pub fn enqueue_accounts<R: Runtime>(
    state: &QueueState,
    app: AppHandle<R>,
    account_ids: Vec<String>,
    headless: bool,
    use_adb: bool,
    force: bool,
) -> Result<QueueStatus, OrchestratorError> {
    let should_start = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| OrchestratorError::QueueLock)?;
        for account_id in account_ids {
            inner.jobs.push_back(QueueJob {
                account_id,
                headless,
                use_adb,
                force,
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

    get_queue_status(state)
}

/// 현재 큐의 상태를 조회한다.
pub fn get_queue_status(state: &QueueState) -> Result<QueueStatus, OrchestratorError> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| OrchestratorError::QueueLock)?;
    Ok(QueueStatus {
        is_running: inner.is_running,
        current_account_id: inner.current_account_id.clone(),
        jobs: inner.jobs.iter().cloned().collect(),
        logs: inner.logs.clone(),
    })
}

/// Resets the queue's running flag (and fails any in-flight job) if `worker_loop`
/// unwinds abnormally — e.g. `process_account` panics. Without it a panic would
/// leave `is_running = true` and the job stuck `Running`, permanently wedging the
/// queue (no later enqueue would spawn a new worker). `process_account` runs with
/// the lock released, so the mutex isn't poisoned and cleanup can proceed.
struct WorkerGuard<'a> {
    state: &'a QueueState,
}

impl Drop for WorkerGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.is_running = false;
            inner.current_account_id = None;
            for job in inner
                .jobs
                .iter_mut()
                .filter(|j| j.status == QueueJobStatus::Running)
            {
                job.status = QueueJobStatus::Failed;
                job.message = "작업이 비정상 종료되었습니다".to_string();
                job.finished_at = Some(now_millis());
            }
        }
    }
}

async fn worker_loop<R: Runtime>(state: QueueState, app: AppHandle<R>) {
    let _guard = WorkerGuard { state: &state };
    loop {
        let (job, idx) = {
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
            (job, index + 1)
        };

        // 로그용 마스킹 식별자(#순번 + 앞 3자). PW는 어떤 로그에도 넣지 않는다.
        let label = format!("#{idx} {}", mask_id(&job.account_id));
        tracing::info!("[LOGIN] {label}  로그인 시작");

        let cookie_status =
            account_cookie_status_for_app_data(&job.account_id).unwrap_or(CookieStatus::Missing);
        if cookie_status == CookieStatus::Expired {
            if let Ok(mut inner) = state.inner.lock() {
                if let Some(existing) = inner
                    .jobs
                    .iter_mut()
                    .find(|j| j.account_id == job.account_id && j.started_at == job.started_at)
                {
                    // 상태는 Running으로 유지하고 메시지만 바꾼다. 여기서 Expired로 바꾸면
                    // 프론트 폴링이 "갱신 중인 일시적 Expired"를 "갱신 완료(성공)"로 오인해
                    // 로그인 성공 토스트를 띄우고 폴링을 일찍 멈춘다. Expired는 process_account
                    // 가 끝난 뒤(아래) 종료 상태로만 쓴다.
                    existing.message = "expired; refreshing".to_string();
                }
                push_log(&mut inner, format!("{}: cookie expired", job.account_id));
            }
            tracing::info!("[LOGIN] {label}  쿠키 만료 — 갱신 시도");
        }

        let result =
            process_account(&app, &job.account_id, job.headless, job.use_adb, job.force).await;
        let message = match &result {
            Ok(()) if cookie_status == CookieStatus::Expired => "expired; refreshed".to_string(),
            Ok(()) => "success".to_string(),
            Err(err) => err.to_string(),
        };
        let status = if result.is_ok() && cookie_status == CookieStatus::Expired {
            QueueJobStatus::Expired
        } else if result.is_ok() {
            QueueJobStatus::Success
        } else {
            QueueJobStatus::Failed
        };

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

        // 로그인 결과를 pstmacro.log에 기록(가독성·상세화). 동작 무변경, 로그 줄만 추가.
        match status {
            QueueJobStatus::Success | QueueJobStatus::Expired => {
                tracing::info!("[LOGIN] {label}  로그인 성공 ✅");
            }
            _ => tracing::info!("[LOGIN] {label}  로그인 실패 ❌ — {message}"),
        }

        // Log login result to the activity feed.
        {
            use crate::ipc::activity::{record, ActivityItem, ActivityType};
            use crate::store::JsonStore;
            let activity = app.state::<JsonStore<ActivityItem>>();
            let (ty, msg) = match status {
                QueueJobStatus::Success | QueueJobStatus::Expired => (
                    ActivityType::Success,
                    format!("계정 {} 로그인 성공", job.account_id),
                ),
                _ => (
                    ActivityType::Error,
                    format!("계정 {} 로그인 실패 — {message}", job.account_id),
                ),
            };
            record(activity.inner(), ty, msg);
        }
    }
}

// 큐 상태 조회(`QueueStatus.logs`)용 in-memory 로그만 유지한다. 과거에는 별도
// `queue.log` 파일에도 기록했지만, 로그를 `pstmacro.log` 하나로 일원화하면서 파일
// 출력을 제거했다(같은 이벤트를 worker_loop가 `tracing`으로 pstmacro.log에 남긴다).
fn push_log(inner: &mut QueueInner, message: String) {
    inner.logs.push(format!("{} {}", now_millis(), message));
    if inner.logs.len() > 200 {
        inner.logs.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_transitions_to_running() {
        let state = QueueState::default();
        {
            let mut inner = state.inner.lock().unwrap();
            inner.jobs.push_back(QueueJob {
                account_id: "id1".to_string(),
                headless: true,
                use_adb: false,
                force: false,
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
        let status = get_queue_status(&state).expect("status should load");

        assert!(status.is_running);
        assert_eq!(status.current_account_id, Some("id1".to_string()));
        assert_eq!(status.jobs[0].status, QueueJobStatus::Running);
        assert!(status.jobs[0].headless);
    }

    #[test]
    fn push_log_caps_in_memory_history() {
        // 로그는 pstmacro.log로 일원화되었으므로 push_log는 in-memory 큐 상태만 유지한다.
        let mut inner = QueueInner::default();
        for i in 0..205 {
            push_log(&mut inner, format!("event {i}"));
        }

        // 메모리 로그는 최근 200개로 제한되고 가장 최신 항목이 남는다.
        assert_eq!(inner.logs.len(), 200);
        assert!(inner.logs.last().unwrap().contains("event 204"));
    }
}
