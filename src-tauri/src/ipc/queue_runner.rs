//! 게시 큐 실행 워커(이슈 #144). now 큐(`JsonStore<QueueNowItem>`)를 작업의 단일
//! 진실원으로 두고, 위에서부터 `Waiting` 아이템을 하나씩 꺼내 실제로 게시한다.
//! 워커 자신은 실행 상태(`is_running`, `current_id`)만 in-memory로 들고, 잡 목록·
//! 순서·진행률은 모두 디스크(JsonStore)에 반영한다(영속화·폴링은 #143).
//!
//! 1단계 범위: 워커 골격 + 카페 글(`run_post_jobs`). 카페 댓글(both=self /
//! latest·popular / url)·종목토론방 게시와 완료 로그/activity는 후속 단계에서
//! `execute_item`에 추가한다.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Runtime};

use super::posts::ModeValue;
use super::queue::{apply_cancel_now, PublishPlan, QueueNowItem, QueueState};
use crate::naver_cafe::orchestrator::PostJob;
use crate::naver_cafe::run_post_jobs;
use crate::store::JsonStore;

/// now 큐 실행 워커의 in-memory 상태. 잡 자체는 `JsonStore<QueueNowItem>`에 있다.
#[derive(Default, Clone)]
pub struct NowQueueRunner {
    inner: Arc<Mutex<RunnerInner>>,
}

#[derive(Default)]
struct RunnerInner {
    is_running: bool,
    current_id: Option<String>,
}

/// 위에서부터 첫 `Waiting` 아이템을 고른다(`Running`은 건너뛴다).
pub fn pick_next_waiting(items: &[QueueNowItem]) -> Option<QueueNowItem> {
    items
        .iter()
        .find(|i| i.state == QueueState::Waiting)
        .cloned()
}

/// plan의 네이버 카페 대상을 글 작성 작업(`PostJob`)으로 변환한다. 제목/본문은 예약
/// 시점에 동결된 plan 값을 쓴다(이슈 #142). 글을 쓰는 모드(post/both)에서만 의미가 있다.
pub fn plan_to_post_jobs(plan: &PublishPlan) -> Vec<PostJob> {
    plan.naver
        .iter()
        .map(|t| PostJob {
            account_id: t.account_id.clone(),
            cafe: t.cafe.clone(),
            menu_id: t.menu_id,
            board_type: t.board_type.clone(),
            subject: plan.title.clone(),
            body_text: plan.body_text.clone(),
            tag_list: Vec::new(),
        })
        .collect()
}

/// 이 plan이 카페 글을 쓰는지(post/both 모드).
fn runs_post(plan: &PublishPlan) -> bool {
    matches!(plan.kind, ModeValue::Post | ModeValue::Both)
}

/// 워커가 돌고 있지 않으면 기동한다. promote(예약→즉시 처리) 시 호출한다.
/// 이미 돌고 있으면 아무것도 하지 않는다(워커가 새 Waiting 아이템을 이어서 집어간다).
pub fn start_if_idle<R: Runtime>(runner: &NowQueueRunner, app: AppHandle<R>) {
    let should_start = {
        let Ok(mut inner) = runner.inner.lock() else {
            return;
        };
        if inner.is_running {
            false
        } else {
            inner.is_running = true;
            true
        }
    };
    if should_start {
        let runner = runner.clone();
        tauri::async_runtime::spawn(async move {
            worker_loop(app, runner).await;
        });
    }
}

async fn worker_loop<R: Runtime>(app: AppHandle<R>, runner: NowQueueRunner) {
    loop {
        let now_store = app.state::<JsonStore<QueueNowItem>>();

        let job = match pick_next_waiting(&now_store.snapshot()) {
            Some(job) => job,
            None => {
                if let Ok(mut inner) = runner.inner.lock() {
                    inner.is_running = false;
                    inner.current_id = None;
                }
                return;
            }
        };

        if let Ok(mut inner) = runner.inner.lock() {
            inner.current_id = Some(job.id.clone());
        }
        let total = job.plan.as_ref().map_or(0, |p| p.naver.len() as u32);
        now_store.mutate(|items| set_running(items, &job.id, total));

        execute_item(&app, &job).await;

        // 완료된 아이템은 큐에서 제거한다(취소와 동일 경로 재사용).
        app.state::<JsonStore<QueueNowItem>>()
            .mutate(|items| apply_cancel_now(items, &job.id));
        if let Ok(mut inner) = runner.inner.lock() {
            inner.current_id = None;
        }
    }
}

/// 한 큐 아이템을 실제로 게시한다. 1단계는 카페 글(post/both 모드)만 처리한다.
async fn execute_item<R: Runtime>(app: &AppHandle<R>, item: &QueueNowItem) {
    let Some(plan) = item.plan.as_ref() else {
        return;
    };

    if runs_post(plan) {
        let post_jobs = plan_to_post_jobs(plan);
        if !post_jobs.is_empty() {
            let done = post_jobs.len() as u32;
            // 한 건이 실패해도 나머지를 계속 진행한다(run_post_jobs 내부 보장).
            let _reports = run_post_jobs(&post_jobs).await;
            app.state::<JsonStore<QueueNowItem>>()
                .mutate(|items| set_progress(items, &item.id, done));
        }
    }
    // 2단계: 카페 댓글(both=self / latest·popular / url) + 종목토론방 + 완료 로그/activity.
}

/// 아이템을 `Running`으로 전이하고 진행률을 `(0, total)`로 초기화한다.
fn set_running(mut items: Vec<QueueNowItem>, id: &str, total: u32) -> Vec<QueueNowItem> {
    for item in &mut items {
        if item.id == id {
            item.state = QueueState::Running;
            item.progress = Some((0, total));
        }
    }
    items
}

/// 진행률의 완료 수(done)를 갱신한다(총계는 유지).
fn set_progress(mut items: Vec<QueueNowItem>, id: &str, done: u32) -> Vec<QueueNowItem> {
    for item in &mut items {
        if item.id == id {
            if let Some((_, total)) = item.progress {
                item.progress = Some((done, total));
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::queue::NaverTarget;

    fn plan_with_naver(n: usize, kind: ModeValue) -> PublishPlan {
        PublishPlan {
            post_id: "p1".into(),
            kind,
            title: "T".into(),
            body_text: "B".into(),
            comments: vec![],
            naver: (0..n)
                .map(|i| NaverTarget {
                    account_id: format!("u{i}"),
                    cafe: "123".into(),
                    menu_id: 7,
                    board_type: "L".into(),
                    comment_target: None,
                })
                .collect(),
            forum: vec![],
        }
    }

    fn now_item(id: &str, state: QueueState, plan: Option<PublishPlan>) -> QueueNowItem {
        QueueNowItem {
            id: id.into(),
            title: "t".into(),
            kind: ModeValue::Post,
            state,
            batch_id: None,
            progress: None,
            locs: vec![],
            plan,
        }
    }

    #[test]
    fn pick_next_waiting_skips_running() {
        let items = vec![
            now_item("r", QueueState::Running, None),
            now_item("w1", QueueState::Waiting, None),
            now_item("w2", QueueState::Waiting, None),
        ];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "w1");
    }

    #[test]
    fn pick_next_waiting_none_when_empty_or_all_running() {
        assert!(pick_next_waiting(&[]).is_none());
        assert!(pick_next_waiting(&[now_item("r", QueueState::Running, None)]).is_none());
    }

    #[test]
    fn plan_to_post_jobs_uses_frozen_title_and_body() {
        let jobs = plan_to_post_jobs(&plan_with_naver(2, ModeValue::Post));
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].subject, "T");
        assert_eq!(jobs[0].body_text, "B");
        assert_eq!(jobs[0].menu_id, 7);
        assert_eq!(jobs[0].account_id, "u0");
        assert!(jobs[0].tag_list.is_empty());
    }

    #[test]
    fn runs_post_only_for_post_and_both() {
        assert!(runs_post(&plan_with_naver(1, ModeValue::Post)));
        assert!(runs_post(&plan_with_naver(1, ModeValue::Both)));
        assert!(!runs_post(&plan_with_naver(1, ModeValue::Comment)));
    }

    #[test]
    fn set_running_transitions_state_and_initializes_progress() {
        let next = set_running(vec![now_item("a", QueueState::Waiting, None)], "a", 3);
        assert_eq!(next[0].state, QueueState::Running);
        assert_eq!(next[0].progress, Some((0, 3)));
    }

    #[test]
    fn set_progress_updates_done_and_keeps_total() {
        let mut item = now_item("a", QueueState::Running, None);
        item.progress = Some((0, 3));
        let next = set_progress(vec![item], "a", 2);
        assert_eq!(next[0].progress, Some((2, 3)));
    }
}
