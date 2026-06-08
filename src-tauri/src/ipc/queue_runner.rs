//! 게시 큐 실행 워커(이슈 #144). now 큐(`JsonStore<QueueNowItem>`)를 작업의 단일
//! 진실원으로 두고, 위에서부터 `Waiting` 아이템을 하나씩 꺼내 실제로 게시한다.
//! 워커 자신은 실행 상태(`is_running`)만 in-memory로 들고, 잡 목록·순서·진행률은
//! 모두 디스크(JsonStore)에 반영한다(영속화·폴링은 #143).
//!
//! 범위: 워커 골격 + 카페 글(`run_post_jobs`) + 카페 댓글(both=방금 쓴 글에 self /
//! latest·popular=글목록 조회 / url) + 종목토론방 게시(`run_forum_publish`, 계정별
//! Chrome) + 완료 로그(`LogBatch`)/activity 기록(아이템별 결과를 알림에 남긴다).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Runtime};

use super::accounts::PlatformId;
use super::activity::{record, ActivityItem, ActivityType};
use super::log_batches::{BatchItem, BatchItemStatus, LogBatch, MAX_LOG_BATCHES};
use super::posts::{CommentTarget, ModeValue};
use super::queue::{apply_cancel_now, PublishPlan, QueueNowItem, QueueState};
use crate::discussion_batch::{run_forum_publish, ForumPublishRequest, ForumPublishResult};
use crate::naver_automation::types::DiscussionStock;
use crate::naver_cafe::article_list::models::SortBy;
use crate::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use crate::naver_cafe::orchestrator::{CommentJob, CommentJobReport, JobReport, PostJob};
use crate::naver_cafe::{fetch_article_list_for_account, run_comment_jobs, run_post_jobs};
use crate::store::JsonStore;
use crate::util::now_ms;

/// 완료 로그(`LogBatch`) id의 단조 증가 시퀀스 — 같은 ms에 여러 배치가 나도 id가
/// 겹치지 않게 한다. id는 `lb-q-` prefix를 써서 lib.rs forum 즉시게시(`lb-`)의 별도
/// 카운터와도 충돌하지 않는다.
static LB_SEQ: AtomicU64 = AtomicU64::new(0);

/// now 큐 실행 워커의 in-memory 상태. 잡 자체는 `JsonStore<QueueNowItem>`에 있다.
#[derive(Default, Clone)]
pub struct NowQueueRunner {
    inner: Arc<Mutex<RunnerInner>>,
}

#[derive(Default)]
struct RunnerInner {
    is_running: bool,
}

/// 워커 종료(정상/패닉) 시 `is_running`을 반드시 해제해, 패닉 한 번에 큐가 영구히
/// 멈추지(wedge) 않게 하는 안전망. 정상 종료 경로도 이 가드를 거친다.
struct RunningGuard {
    runner: NowQueueRunner,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.runner.inner.lock() {
            inner.is_running = false;
        }
    }
}

/// 댓글 게시 대상 1건 — 글 작성 결과(self) 또는 commentTarget 해석으로 만들어진다.
struct CommentTargetEntry {
    account_id: String,
    cafe_id: u64,
    article_id: u64,
}

/// 종목토론방 게시 결과 1건 — 어느 계정으로 돌렸는지(login_id)와 종목별 결과를 묶는다.
/// `ForumPublishResult`에는 계정 정보가 없으므로 워커가 계정과 짝지어 보존한다.
struct ForumOutcome {
    account_id: String,
    result: ForumPublishResult,
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

/// 이 plan이 카페 댓글을 다는지(comment/both 모드).
fn runs_comment(plan: &PublishPlan) -> bool {
    matches!(plan.kind, ModeValue::Comment | ModeValue::Both)
}

fn sort_by_for(mode: &CommentTarget) -> Option<SortBy> {
    match mode {
        CommentTarget::Latest => Some(SortBy::Latest),
        CommentTarget::Popular => Some(SortBy::Popular),
        CommentTarget::Url => None,
    }
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
    // 정상/패닉 종료 모두 is_running을 해제한다(큐 wedge 방지).
    let _guard = RunningGuard {
        runner: runner.clone(),
    };

    loop {
        let now_store = app.state::<JsonStore<QueueNowItem>>();

        let job = match pick_next_waiting(&now_store.snapshot()) {
            Some(job) => job,
            None => {
                // drain 경계 race 방지: is_running 해제를 runner 락 안에서, 큐를 한 번 더
                // 확인한 뒤 한다. 락이 promote의 start_if_idle(검사+set)과 직렬화되므로,
                // 막 추가된 Waiting 아이템이 stranded 되지 않는다.
                let Ok(mut inner) = runner.inner.lock() else {
                    return;
                };
                if pick_next_waiting(&now_store.snapshot()).is_some() {
                    drop(inner);
                    continue;
                }
                inner.is_running = false;
                return;
            }
        };

        now_store.mutate(|items| mark_running(items, &job.id));

        execute_item(&app, &job).await;

        // 완료된 아이템은 큐에서 제거한다(취소와 동일 경로 재사용).
        app.state::<JsonStore<QueueNowItem>>()
            .mutate(|items| apply_cancel_now(items, &job.id));
    }
}

/// 큐에 해당 id의 아이템이 아직 있는지(협조적 취소 확인용). cancel_queue_now가
/// 아이템을 제거하면, 워커는 다음 단계로 진입하기 전 이를 보고 중단한다.
fn item_present<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    app.state::<JsonStore<QueueNowItem>>()
        .snapshot()
        .iter()
        .any(|i| i.id == id)
}

/// 한 큐 아이템을 실제로 게시한다(카페 글 + 카페 댓글). 각 단계 진입 전 아이템이
/// 아직 큐에 있는지 확인해, 진행 중 단계는 끝까지 두되 다음 단계는 협조적으로 멈춘다.
/// (실행 중 단계의 강한 중단과 종목토론방·완료 로그는 후속 단계.)
async fn execute_item<R: Runtime>(app: &AppHandle<R>, item: &QueueNowItem) {
    let Some(plan) = item.plan.as_ref() else {
        return;
    };
    let id = item.id.as_str();

    // 1. 카페 글(post/both).
    let post_reports = if runs_post(plan) && item_present(app, id) {
        let jobs = plan_to_post_jobs(plan);
        if jobs.is_empty() {
            Vec::new()
        } else {
            run_post_jobs(&jobs).await
        }
    } else {
        Vec::new()
    };

    // 2. 카페 댓글(comment/both) 대상 확정.
    let comment_targets = if runs_comment(plan) && item_present(app, id) {
        collect_comment_targets(plan, &post_reports).await
    } else {
        Vec::new()
    };

    // 3. 카페 댓글 작업 구성. 진행률 total은 comment_targets가 아니라 **실제 만들어진
    // 작업 수**로 잡아야 100%에 도달한다(빈 댓글 풀로 인한 영구 미완 방지 — build_comment_jobs).
    let comment_jobs = build_comment_jobs(comment_targets, &plan.comments);

    // 진행률 총계 확정(실제 카페 글 + 카페 댓글 작업 + 종목토론방 종목 수).
    let total = (post_reports.len() + comment_jobs.len() + plan.forum.len()) as u32;
    let mut done = post_reports.len() as u32;
    update_progress(app, id, done, total);

    let comment_reports = if comment_jobs.is_empty() {
        Vec::new()
    } else {
        let posted = comment_jobs.len() as u32;
        let reports = run_comment_jobs(&comment_jobs).await;
        done += posted;
        update_progress(app, id, done, total);
        reports
    };

    // 4. 종목토론방 게시(계정별 Chrome, 본문은 평문 = plan.body_text). 진행 중 단계는
    // 끝까지 두되, 진입 전 협조적 취소를 확인한다.
    let forum_outcomes = if !plan.forum.is_empty() && item_present(app, id) {
        let outcomes = run_forum_targets(app, plan).await;
        done += outcomes.len() as u32;
        update_progress(app, id, done, total);
        outcomes
    } else {
        Vec::new()
    };

    // 5. 완료 로그(LogBatch)/activity: 실제 실행한 카페 글·댓글·토론방 결과를 알림에
    // 남긴다. 실행한 작업이 하나도 없으면(빈 plan) 빈 배치는 만들지 않는다.
    let batch = build_log_batch(
        plan,
        &post_reports,
        &comment_reports,
        &forum_outcomes,
        now_ms(),
        LB_SEQ.fetch_add(1, Ordering::Relaxed),
    );
    if !batch.items.is_empty() {
        record_completion(app, batch);
    }
}

/// 댓글 대상을 모은다. both 모드는 방금 게시에 성공한 글(self)에, comment 전용은
/// 각 naver 대상의 `commentTarget`(url 직접 / latest·popular 글목록 조회)에 단다.
async fn collect_comment_targets(
    plan: &PublishPlan,
    post_reports: &[JobReport],
) -> Vec<CommentTargetEntry> {
    let mut targets = Vec::new();

    if matches!(plan.kind, ModeValue::Both) {
        // self-comment: 방금 게시에 성공한 글에 단다(즉시게시 both 동작과 동일).
        for report in post_reports {
            if let Some(result) = &report.result {
                targets.push(CommentTargetEntry {
                    account_id: report.account_id.clone(),
                    cafe_id: result.cafe_id,
                    article_id: result.article_id,
                });
            }
        }
        return targets;
    }

    // comment 전용: 대상마다 commentTarget을 해석한다.
    for t in &plan.naver {
        let Some(spec) = &t.comment_target else {
            continue;
        };
        match spec.mode {
            CommentTarget::Url => {
                if let (Some(cafe_id), Some(article_id)) = (spec.cafe_id, spec.article_id) {
                    targets.push(CommentTargetEntry {
                        account_id: t.account_id.clone(),
                        cafe_id,
                        article_id,
                    });
                }
            }
            CommentTarget::Latest | CommentTarget::Popular => {
                let (Some(cafe_id), Some(sort)) = (spec.cafe_id, sort_by_for(&spec.mode)) else {
                    continue;
                };
                let count = spec.count.unwrap_or(1).max(1) as usize;
                // 실행 시점에 상위 N개를 다시 조회한다(예약과 실행 사이 새 글 반영).
                if let Ok(resp) =
                    fetch_article_list_for_account(&cafe_id.to_string(), sort, &t.account_id).await
                {
                    for article in resp.articles.iter().take(count) {
                        targets.push(CommentTargetEntry {
                            account_id: t.account_id.clone(),
                            cafe_id,
                            article_id: article.article_id,
                        });
                    }
                }
            }
        }
    }
    targets
}

/// plan의 종목토론방 대상을 계정별로 묶어 `ForumPublishRequest`로 만든다. 본문은
/// 동결된 평문(`body_text`)을 쓰고(토론방은 평문만 지원), 댓글은 풀의 첫 항목을 쓴다
/// (즉시게시 forum 경로와 동일). host/port는 호출부가 띄운 Chrome 값으로 채운다.
fn plan_to_forum_requests(plan: &PublishPlan) -> Vec<ForumPublishRequest> {
    use std::collections::BTreeMap;

    let mut by_account: BTreeMap<String, Vec<DiscussionStock>> = BTreeMap::new();
    for f in &plan.forum {
        by_account
            .entry(f.account_id.clone())
            .or_default()
            .push(DiscussionStock {
                name: f.name.clone(),
                code: f.code.clone(),
                link: String::new(),
            });
    }

    let run_post = runs_post(plan);
    let run_comment = runs_comment(plan);
    let comment = plan.comments.first().cloned().unwrap_or_default();

    by_account
        .into_iter()
        .map(|(account_id, stocks)| ForumPublishRequest {
            host: "127.0.0.1".to_owned(),
            port: 0,
            account_id,
            run_post,
            run_comment,
            title: plan.title.clone(),
            body: plan.body_text.clone(),
            comment: comment.clone(),
            stocks,
        })
        .collect()
}

/// 종목토론방 대상을 계정별로 게시한다. 계정마다 디버그 포트 Chrome을 직접 띄워
/// (run_forum_publish_now와 동일 패턴) 패킷 엔진으로 게시하고, 계정·종목별 결과를
/// 돌려준다(완료 로그용). 단일 워커가 순차로 돌므로 Chrome 인스턴스 충돌이 없다.
/// Chrome 기동 실패 시 그 계정의 종목들을 실패 결과로 합성해, 진행률·로그가 조용히
/// 누락되지 않게 한다(거짓 100% 방지).
async fn run_forum_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
) -> Vec<ForumOutcome> {
    let mut outcomes = Vec::new();
    for req in plan_to_forum_requests(plan) {
        let account_id = req.account_id.clone();
        let app_for_job = app.clone();
        let results = tauri::async_runtime::spawn_blocking(move || {
            match crate::auth::launch_debug_chrome(true) {
                Ok(chrome) => {
                    let mut req = req;
                    // host는 plan_to_forum_requests에서 이미 127.0.0.1; 포트만 띄운 Chrome 값으로.
                    req.port = chrome.port;
                    let results = run_forum_publish(req, app_for_job);
                    drop(chrome);
                    results
                }
                // Chrome 기동 실패 → 이 계정 종목 전부 실패로 기록(누락 대신 명시).
                Err(error) => req
                    .stocks
                    .iter()
                    .map(|s| ForumPublishResult {
                        code: s.code.clone(),
                        name: s.name.clone(),
                        ok: false,
                        message: format!("Chrome 실행 실패: {error}"),
                    })
                    .collect(),
            }
        })
        .await
        .unwrap_or_default();
        for result in results {
            outcomes.push(ForumOutcome {
                account_id: account_id.clone(),
                result,
            });
        }
    }
    outcomes
}

/// 댓글 대상에 댓글 풀(comments)을 분배해 작업으로 만든다. 풀이 비면 빈 목록을 내
/// 댓글 작업이 0건이 되며, 호출부의 진행률 total이 실제 작업 수로 잡혀 영구 미완을 피한다.
fn build_comment_jobs(targets: Vec<CommentTargetEntry>, comments: &[String]) -> Vec<CommentJob> {
    if targets.is_empty() {
        return Vec::new();
    }
    let mut rng = mulberry32(seed_from_clock());
    let contents = distribute_comments(targets.len(), comments, &mut rng);
    targets
        .into_iter()
        .zip(contents)
        .map(|(t, content)| CommentJob {
            account_id: t.account_id,
            cafe_id: t.cafe_id,
            article_id: t.article_id,
            content,
        })
        .collect()
}

fn status_of(ok: bool) -> BatchItemStatus {
    if ok {
        BatchItemStatus::Success
    } else {
        BatchItemStatus::Fail
    }
}

/// 카페 글·댓글·종목토론방 실행 결과를 알림 배치(`LogBatch`) 한 건으로 묶는다.
/// 본문/댓글 스냅샷은 실제 그 작업을 돌린 모드일 때만 남긴다(즉시게시 forum 경로와 동일).
fn build_log_batch(
    plan: &PublishPlan,
    post_reports: &[JobReport],
    comment_reports: &[CommentJobReport],
    forum_outcomes: &[ForumOutcome],
    at: i64,
    seq: u64,
) -> LogBatch {
    let mut items = Vec::new();

    for r in post_reports {
        items.push(BatchItem {
            platform: PlatformId::Naver,
            target: r.cafe.clone(),
            code: None,
            board: Some(r.menu_id.to_string()),
            login_id: r.account_id.clone(),
            status: status_of(r.success),
            // msg/status를 같은 기준(success)으로 묶는다 — 실패인데 오류가 비면(불변식
            // 위반 시) "완료"로 오인되지 않게.
            msg: if r.success {
                "글 게시 완료".to_owned()
            } else {
                r.error
                    .as_ref()
                    .map_or_else(|| "글 게시 실패".to_owned(), |e| e.message.clone())
            },
            trace: r
                .error
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message)),
        });
    }

    for r in comment_reports {
        items.push(BatchItem {
            platform: PlatformId::Naver,
            target: r.cafe_id.to_string(),
            code: None,
            board: None,
            login_id: r.account_id.clone(),
            status: status_of(r.success),
            msg: if r.success {
                "댓글 게시 완료".to_owned()
            } else {
                r.error
                    .as_ref()
                    .map_or_else(|| "댓글 게시 실패".to_owned(), |e| e.message.clone())
            },
            trace: r
                .error
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message)),
        });
    }

    for o in forum_outcomes {
        items.push(BatchItem {
            platform: PlatformId::Forum,
            target: o.result.name.clone(),
            code: Some(o.result.code.clone()),
            board: None,
            login_id: o.account_id.clone(),
            status: status_of(o.result.ok),
            msg: o.result.message.clone(),
            trace: if o.result.ok {
                None
            } else {
                Some(o.result.message.clone())
            },
        });
    }

    LogBatch {
        // 큐 경로 전용 prefix(`lb-q-`): lib.rs forum 즉시게시의 `lb-` 시퀀스와 별도
        // 카운터라, 같은 ms·seq라도 id가 겹치지 않게 한다.
        id: format!("lb-q-{at}-{seq}"),
        title: plan.title.clone(),
        // 게시 시점 원문 스냅샷: 실제 그 작업을 돌린 모드만 남긴다.
        body: if runs_post(plan) && !plan.body_text.is_empty() {
            Some(plan.body_text.clone())
        } else {
            None
        },
        comment: if runs_comment(plan) {
            plan.comments.iter().find(|c| !c.trim().is_empty()).cloned()
        } else {
            None
        },
        kind: plan.kind.clone(),
        at,
        state: None,
        items,
    }
}

/// 완료 배치를 로그 스토어 맨 앞에 넣고(최신순) activity에 요약을 남긴다.
fn record_completion<R: Runtime>(app: &AppHandle<R>, batch: LogBatch) {
    let total = batch.items.len();
    let ok = batch
        .items
        .iter()
        .filter(|i| i.status == BatchItemStatus::Success)
        .count();
    let title = batch.title.clone();

    app.state::<JsonStore<LogBatch>>().mutate(|mut v| {
        v.insert(0, batch);
        v.truncate(MAX_LOG_BATCHES);
        v
    });

    let ty = if ok == total {
        ActivityType::Success
    } else if ok == 0 {
        ActivityType::Error
    } else {
        ActivityType::Info
    };
    let activity = app.state::<JsonStore<ActivityItem>>();
    record(
        activity.inner(),
        ty,
        format!("'{title}' 예약 게시 — {total}곳 중 {ok}곳 성공"),
    );
}

/// 아이템을 `Running`으로 전이한다. 진행률은 execute_item이 총계를 확정한 뒤 채운다.
fn mark_running(mut items: Vec<QueueNowItem>, id: &str) -> Vec<QueueNowItem> {
    for item in &mut items {
        if item.id == id {
            item.state = QueueState::Running;
        }
    }
    items
}

fn update_progress<R: Runtime>(app: &AppHandle<R>, id: &str, done: u32, total: u32) {
    app.state::<JsonStore<QueueNowItem>>()
        .mutate(|items| set_progress_value(items, id, done, total));
}

fn set_progress_value(
    mut items: Vec<QueueNowItem>,
    id: &str,
    done: u32,
    total: u32,
) -> Vec<QueueNowItem> {
    for item in &mut items {
        if item.id == id {
            item.progress = Some((done, total));
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::queue::{CommentTargetSpec, NaverTarget};
    use crate::naver_cafe::orchestrator::JobReport;
    use crate::naver_cafe::post::parser::ArticleRegisterResult;

    fn naver_target(account: &str) -> NaverTarget {
        NaverTarget {
            account_id: account.into(),
            cafe: "123".into(),
            menu_id: 7,
            board_type: "L".into(),
            comment_target: None,
        }
    }

    fn plan(kind: ModeValue, naver: Vec<NaverTarget>) -> PublishPlan {
        PublishPlan {
            post_id: "p1".into(),
            kind,
            title: "T".into(),
            body_text: "B".into(),
            comments: vec!["c1".into()],
            naver,
            forum: vec![],
        }
    }

    fn now_item(id: &str, state: QueueState, p: Option<PublishPlan>) -> QueueNowItem {
        QueueNowItem {
            id: id.into(),
            title: "t".into(),
            kind: ModeValue::Post,
            state,
            batch_id: None,
            progress: None,
            locs: vec![],
            plan: p,
        }
    }

    fn post_report(account: &str, cafe_id: u64, article_id: u64) -> JobReport {
        JobReport {
            account_id: account.into(),
            cafe: cafe_id.to_string(),
            menu_id: 7,
            success: true,
            result: Some(ArticleRegisterResult {
                cafe_id,
                article_id,
                menu_id: 7,
            }),
            error: None,
        }
    }

    fn post_fail(account: &str, cafe: &str, code: &str, msg: &str) -> JobReport {
        use crate::naver_cafe::ErrorEnvelope;
        JobReport {
            account_id: account.into(),
            cafe: cafe.into(),
            menu_id: 7,
            success: false,
            result: None,
            error: Some(ErrorEnvelope {
                trace_id: "t".into(),
                code: code.into(),
                message: msg.into(),
                error_data: None,
            }),
        }
    }

    fn forum_ok(account: &str, name: &str, code: &str) -> ForumOutcome {
        ForumOutcome {
            account_id: account.into(),
            result: ForumPublishResult {
                code: code.into(),
                name: name.into(),
                ok: true,
                message: "게시 완료".into(),
            },
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
        let jobs = plan_to_post_jobs(&plan(
            ModeValue::Post,
            vec![naver_target("u0"), naver_target("u1")],
        ));
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].subject, "T");
        assert_eq!(jobs[0].body_text, "B");
        assert_eq!(jobs[0].menu_id, 7);
        assert_eq!(jobs[0].account_id, "u0");
        assert!(jobs[0].tag_list.is_empty());
    }

    #[test]
    fn runs_post_and_runs_comment_match_mode() {
        assert!(runs_post(&plan(ModeValue::Post, vec![])));
        assert!(runs_post(&plan(ModeValue::Both, vec![])));
        assert!(!runs_post(&plan(ModeValue::Comment, vec![])));
        assert!(runs_comment(&plan(ModeValue::Comment, vec![])));
        assert!(runs_comment(&plan(ModeValue::Both, vec![])));
        assert!(!runs_comment(&plan(ModeValue::Post, vec![])));
    }

    #[test]
    fn mark_running_sets_state_only() {
        let next = mark_running(vec![now_item("a", QueueState::Waiting, None)], "a");
        assert_eq!(next[0].state, QueueState::Running);
        assert!(next[0].progress.is_none());
    }

    #[test]
    fn set_progress_value_writes_done_and_total() {
        let next = set_progress_value(vec![now_item("a", QueueState::Running, None)], "a", 2, 3);
        assert_eq!(next[0].progress, Some((2, 3)));
    }

    #[tokio::test]
    async fn collect_targets_both_comments_on_just_posted_articles() {
        let p = plan(ModeValue::Both, vec![naver_target("u0")]);
        let reports = vec![post_report("u0", 111, 222), post_report("u0", 111, 333)];
        let targets = collect_comment_targets(&p, &reports).await;
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].cafe_id, 111);
        assert_eq!(targets[0].article_id, 222);
        assert_eq!(targets[1].article_id, 333);
        assert_eq!(targets[0].account_id, "u0");
    }

    #[tokio::test]
    async fn collect_targets_url_uses_frozen_ids() {
        let mut t = naver_target("u0");
        t.comment_target = Some(CommentTargetSpec {
            mode: CommentTarget::Url,
            count: None,
            cafe_id: Some(444),
            article_id: Some(555),
        });
        let p = plan(ModeValue::Comment, vec![t]);
        let targets = collect_comment_targets(&p, &[]).await;
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].cafe_id, 444);
        assert_eq!(targets[0].article_id, 555);
    }

    #[test]
    fn forum_requests_group_by_account_with_plain_body() {
        use crate::ipc::queue::ForumTarget;
        let mut p = plan(ModeValue::Post, vec![]);
        p.body_text = "평문 본문".into();
        p.comments = vec!["댓글1".into()];
        p.forum = vec![
            ForumTarget {
                account_id: "u0".into(),
                name: "삼성전자".into(),
                code: "005930".into(),
            },
            ForumTarget {
                account_id: "u0".into(),
                name: "SK하이닉스".into(),
                code: "000660".into(),
            },
            ForumTarget {
                account_id: "u1".into(),
                name: "에코프로".into(),
                code: "086520".into(),
            },
        ];
        let reqs = plan_to_forum_requests(&p);
        assert_eq!(reqs.len(), 2); // 계정별(u0, u1) 그룹
        let u0 = reqs.iter().find(|r| r.account_id == "u0").unwrap();
        assert_eq!(u0.stocks.len(), 2);
        assert_eq!(u0.body, "평문 본문"); // 동결 평문 본문(토론방은 평문만 지원)
        assert_eq!(u0.comment, "댓글1");
        assert!(u0.run_post);
        assert!(!u0.run_comment); // Post 모드
    }

    #[test]
    fn build_comment_jobs_empty_when_pool_empty() {
        let targets = vec![
            CommentTargetEntry {
                account_id: "u0".into(),
                cafe_id: 1,
                article_id: 2,
            },
            CommentTargetEntry {
                account_id: "u0".into(),
                cafe_id: 1,
                article_id: 3,
            },
        ];
        // 댓글 풀이 비면 작업 0건 — 진행률 영구 미완(comment_targets만큼 total) 방지.
        assert!(build_comment_jobs(targets, &[]).is_empty());
    }

    #[test]
    fn build_comment_jobs_one_per_target_when_pool_present() {
        let targets = vec![
            CommentTargetEntry {
                account_id: "u0".into(),
                cafe_id: 1,
                article_id: 2,
            },
            CommentTargetEntry {
                account_id: "u0".into(),
                cafe_id: 1,
                article_id: 3,
            },
        ];
        let jobs = build_comment_jobs(targets, &["c1".into()]);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].article_id, 2);
        assert_eq!(jobs[1].article_id, 3);
    }

    #[test]
    fn build_log_batch_maps_post_success_and_fail() {
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        let reports = vec![
            post_report("u0", 123, 999),
            post_fail("u1", "456", "NO_COOKIES", "쿠키 없음"),
        ];
        let b = build_log_batch(&p, &reports, &[], &[], 1_700_000_000_000, 0);
        assert_eq!(b.id, "lb-q-1700000000000-0");
        assert_eq!(b.title, "T");
        assert_eq!(b.body.as_deref(), Some("B")); // post 모드 → 본문 스냅샷
        assert!(b.comment.is_none()); // post 모드 → 댓글 스냅샷 없음
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.items[0].platform, PlatformId::Naver);
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
        assert_eq!(b.items[0].board.as_deref(), Some("7"));
        assert_eq!(b.items[1].status, BatchItemStatus::Fail);
        assert_eq!(b.items[1].msg, "쿠키 없음");
        assert_eq!(b.items[1].trace.as_deref(), Some("NO_COOKIES: 쿠키 없음"));
    }

    #[test]
    fn build_log_batch_comment_mode_snapshots_comment_not_body() {
        let mut p = plan(ModeValue::Comment, vec![naver_target("u0")]);
        p.comments = vec!["  ".into(), "좋은 글이네요".into()];
        let b = build_log_batch(&p, &[], &[], &[], 1, 0);
        assert!(b.body.is_none()); // comment 모드 → 본문 스냅샷 없음
        assert_eq!(b.comment.as_deref(), Some("좋은 글이네요")); // 공백 항목은 건너뜀
        assert!(b.items.is_empty());
    }

    #[test]
    fn build_log_batch_includes_forum_items_with_code_and_account() {
        let p = plan(ModeValue::Post, vec![]);
        let forum = vec![forum_ok("u0", "삼성전자", "005930")];
        let b = build_log_batch(&p, &[], &[], &forum, 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Forum);
        assert_eq!(b.items[0].target, "삼성전자");
        assert_eq!(b.items[0].code.as_deref(), Some("005930"));
        assert_eq!(b.items[0].login_id, "u0");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
    }

    #[tokio::test]
    async fn collect_targets_url_skips_when_ids_missing() {
        let mut t = naver_target("u0");
        t.comment_target = Some(CommentTargetSpec {
            mode: CommentTarget::Url,
            count: None,
            cafe_id: Some(444),
            article_id: None,
        });
        let p = plan(ModeValue::Comment, vec![t]);
        assert!(collect_comment_targets(&p, &[]).await.is_empty());
    }
}
