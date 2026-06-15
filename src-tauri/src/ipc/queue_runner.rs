//! 게시 큐 실행 워커(이슈 #144). now 큐(`JsonStore<QueueNowItem>`)를 작업의 단일
//! 진실원으로 두고, 위에서부터 `Waiting` 아이템을 하나씩 꺼내 실제로 게시한다.
//! 워커 자신은 실행 상태(`is_running`)만 in-memory로 들고, 잡 목록·순서·진행률은
//! 모두 디스크(JsonStore)에 반영한다(영속화·폴링은 #143).
//!
//! 범위: 워커 골격 + 카페 글(`run_post_jobs`) + 카페 댓글(both=방금 쓴 글에 self /
//! latest·popular=글목록 조회 / url) + 종목토론방 게시(`run_forum_publish`, 계정별
//! Chrome) + 밴드 게시(`band_publish`, 순수 HTTP) + 완료 로그(`LogBatch`)/activity
//! 기록(아이템별 결과를 알림에 남긴다).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Runtime};

use super::accounts::{AccountStatus, PlatformId};
use super::activity::{record, ActivityItem, ActivityType};
use super::log_batches::{BatchItem, BatchItemStatus, LogBatch, MAX_LOG_BATCHES};
use super::posts::{CommentTarget, ModeValue};
use super::queue::{apply_cancel_now, LoginTarget, PublishPlan, QueueNowItem, QueueState};
use crate::auth::outcome::LoginResolution;
use crate::auth::OrchestratorError;
use crate::band_post::error::{BandPostError, BandPostErrorKind};
use crate::band_post::{
    band_comment, band_publish, BandCommentOutcome, BandFeedSort, BandPublishOutcome,
};
use crate::discussion_batch::{run_forum_publish, ForumPublishRequest, ForumPublishResult};
use crate::naver_automation::types::DiscussionStock;
use crate::naver_cafe::article_list::models::SortBy;
use crate::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use crate::naver_cafe::orchestrator::{CommentJob, CommentJobReport, JobReport, PostJob};
use crate::naver_cafe::{
    fetch_article_list_for_account, run_comment_jobs_with_progress, run_post_jobs_with_progress,
    NaverCafeCommonErrorData,
};
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

/// 글목록 조회(latest/popular)에 실패해 댓글을 시도조차 못 한 대상. 그냥 누락하면
/// 사용자는 "N곳 중 0곳"이 됐다는 사실조차 모르므로, 완료 로그에 실패로 남기려고 모은다.
/// 카페 글/댓글과 동일하게 메인=친절 사유 / 자세히=trace로 나누려고, 조회 에러의
/// 코드·원문·상세(`error_data`)를 그대로 보존한다(#199).
struct CommentFetchFailure {
    account_id: String,
    cafe_id: u64,
    code: String,
    message: String,
    cafe: Option<NaverCafeCommonErrorData>,
}

/// 댓글 대상 수집 결과 — 실제로 댓글을 달 대상(`targets`)과, 글목록 조회 실패로 댓글을
/// 못 단 대상(`fetch_failures`)을 함께 돌려준다. 후자는 완료 로그에 실패 항목으로 남긴다.
#[derive(Default)]
struct CommentCollect {
    targets: Vec<CommentTargetEntry>,
    fetch_failures: Vec<CommentFetchFailure>,
}

/// 종목토론방 게시 결과 1건 — 어느 계정으로 돌렸는지(login_id)와 종목별 결과를 묶는다.
/// `ForumPublishResult`에는 계정 정보가 없으므로 워커가 계정과 짝지어 보존한다.
struct ForumOutcome {
    account_id: String,
    result: ForumPublishResult,
}

/// 밴드 실행 1건의 결과 — post/both는 새 글 게시(`BandPublishOutcome`), comment 전용은
/// 기존 글(최신/인기)에 댓글(`BandCommentOutcome`). 둘은 로그 문구·성공 판정이 달라
/// 구분해 보존한다.
enum BandJobResult {
    Published(BandPublishOutcome),
    Commented(BandCommentOutcome),
}

/// 밴드 게시 결과 1건 — 어느 계정·밴드(표시 이름)로 돌렸는지와 실행 결과를 묶는다.
/// 성공/실패 모두 완료 로그(`PlatformId::Band`)에 남기려고 보존한다. 게시 응답에 실제
/// 밴드명이 와도 로그 라벨은 예약 시점에 동결된 `band_name`을 우선 쓴다(forum이 동결
/// 이름을 쓰는 것과 일관).
struct BandOutcome {
    account_id: String,
    band_name: String,
    result: Result<BandJobResult, BandPostError>,
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

        // 완료(N/N) 진행률이 프론트 폴링에 한 번은 잡혀 "N/N까지 차오른 뒤 사라짐"이 보이도록,
        // 실제 작업을 한 아이템은 큐에서 빼기 전 한 폴링 주기(750ms)보다 살짝 길게 100% 상태로
        // 머문다. plan 없는(표시 전용) 아이템은 곧장 제거한다.
        if job.plan.is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        }

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

    // 로그인 전용 아이템(#210): 게시 경로를 타지 않고 계정별 로그인만 수행하고 종료한다.
    // 로그인도 게시와 같은 now 큐로 일원화되며, 한 계정이 실패해도 다음 계정으로 진행한다.
    if let Some(login) = plan.login.as_ref().filter(|l| !l.is_empty()) {
        run_login_targets(app, id, login).await;
        return;
    }

    // 글/댓글 1건마다 진행률을 0/N→1/N→…로 올릴 때 쓸 추정 분모. 실제 작업 수(total)가
    // 확정되기 전 단계(글 작성 중)에는 이 상한 추정치를 분모로 쓰고, 확정 후 보정한다.
    let est = estimate_total(plan);

    // 1. 카페 글(post/both). 글 1건이 끝날 때마다 진행률을 올려, 폴링이 배치 완료만 보고
    // 0/N에서 곧장 사라지지 않게 한다(이슈 #198 즉시 게시는 이 진행률을 직접 본다).
    let post_reports = if runs_post(plan) && item_present(app, id) {
        let jobs = plan_to_post_jobs(plan);
        if jobs.is_empty() {
            Vec::new()
        } else {
            run_post_jobs_with_progress(&jobs, |done| {
                update_progress(app, id, done as u32, est);
            })
            .await
        }
    } else {
        Vec::new()
    };

    // 2. 카페 댓글(comment/both) 대상 확정. 글목록 조회 실패 대상(fetch_failures)도 함께
    // 받아 완료 로그에 실패로 남긴다(조용한 누락 방지).
    let collected = if runs_comment(plan) && item_present(app, id) {
        collect_comment_targets(plan, &post_reports).await
    } else {
        CommentCollect::default()
    };
    let comment_fetch_failures = collected.fetch_failures;

    // 3. 카페 댓글 작업 구성. both(쓴 글에 self-comment)는 글마다 템플릿의 **모든**
    // 댓글을 달고(writer-modal "위에서 작성한 글에 바로 댓글이 달립니다"), comment 전용은
    // 대상마다 풀에서 1개씩 분배한다(#98 "계정마다 다른 댓글"). 진행률 total은 실제
    // 만들어진 작업 수로 잡아 100%에 도달하게 한다(빈 풀로 인한 영구 미완 방지).
    let comment_jobs = if matches!(plan.kind, ModeValue::Both) {
        build_self_comment_jobs(collected.targets, &plan.comments)
    } else {
        build_comment_jobs(collected.targets, &plan.comments)
    };

    // 진행률 총계 확정(실제 카페 글 + 카페 댓글 작업 + 종목토론방 종목 수 + 밴드 수). 글
    // 단계에서 쓰던 추정 분모(est)를 여기서 실제 total로 보정한다.
    let total =
        (post_reports.len() + comment_jobs.len() + plan.forum.len() + plan.band.len()) as u32;
    let posts_done = post_reports.len() as u32;
    let mut done = posts_done;
    update_progress(app, id, done, total);

    // 댓글도 1건이 끝날 때마다 진행률을 올린다(글 완료분 위에 누적). 안티스팸 간격은 보존된다.
    let comment_reports = if comment_jobs.is_empty() {
        Vec::new()
    } else {
        let reports = run_comment_jobs_with_progress(&comment_jobs, |c| {
            update_progress(app, id, posts_done + c as u32, total);
        })
        .await;
        done += reports.len() as u32;
        update_progress(app, id, done, total);
        reports
    };

    // 4. 종목토론방 게시(계정별 Chrome, 본문은 평문 = plan.body_text). 진행 중 단계는
    // 끝까지 두되, 진입 전 협조적 취소를 확인한다.
    let forum_outcomes = if !plan.forum.is_empty() && item_present(app, id) {
        let outcomes = run_forum_targets(app, plan, id).await;
        done += outcomes.len() as u32;
        update_progress(app, id, done, total);
        outcomes
    } else {
        Vec::new()
    };

    // 5. 밴드 게시(band.us, 순수 HTTP). 진행 중 단계는 끝까지 두되 진입 전 협조적 취소를 확인한다.
    let band_outcomes = if !plan.band.is_empty() && item_present(app, id) {
        let outcomes = run_band_targets(app, plan, id).await;
        done += outcomes.len() as u32;
        update_progress(app, id, done, total);
        outcomes
    } else {
        Vec::new()
    };

    // 6. 완료 로그(LogBatch)/activity: 실제 실행한 카페 글·댓글·토론방·밴드 결과 + 댓글 대상
    // 조회 실패를 알림에 남긴다. 실행한 작업이 하나도 없으면(빈 plan) 빈 배치는 만들지 않는다.
    let batch = build_log_batch(
        plan,
        &post_reports,
        &comment_reports,
        &forum_outcomes,
        &band_outcomes,
        &comment_fetch_failures,
        now_ms(),
        LB_SEQ.fetch_add(1, Ordering::Relaxed),
    );
    if !batch.items.is_empty() {
        record_completion(app, batch);
    }
}

/// 댓글 대상을 모은다. both 모드는 방금 게시에 성공한 글(self)에, comment 전용은
/// 각 naver 대상의 `commentTarget`(url 직접 / latest·popular 글목록 조회)에 단다.
async fn collect_comment_targets(plan: &PublishPlan, post_reports: &[JobReport]) -> CommentCollect {
    let mut out = CommentCollect::default();

    if matches!(plan.kind, ModeValue::Both) {
        // self-comment: 방금 게시에 성공한 글에 단다(즉시게시 both 동작과 동일). 게시에
        // 실패한 글은 여기 없다 — 그 실패는 post_reports(글 게시 실패)로 이미 로그에 남으므로
        // "글이 없어 댓글도 못 달았다"는 별도 항목은 만들지 않는다.
        for report in post_reports {
            if let Some(result) = &report.result {
                out.targets.push(CommentTargetEntry {
                    account_id: report.account_id.clone(),
                    cafe_id: result.cafe_id,
                    article_id: result.article_id,
                });
            }
        }
        return out;
    }

    // comment 전용: 대상마다 commentTarget을 해석한다.
    for t in &plan.naver {
        let Some(spec) = &t.comment_target else {
            continue;
        };
        match spec.mode {
            CommentTarget::Url => {
                if let (Some(cafe_id), Some(article_id)) = (spec.cafe_id, spec.article_id) {
                    out.targets.push(CommentTargetEntry {
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
                match fetch_article_list_for_account(&cafe_id.to_string(), sort, &t.account_id)
                    .await
                {
                    Ok(resp) => {
                        for article in resp.articles.iter().take(count) {
                            out.targets.push(CommentTargetEntry {
                                account_id: t.account_id.clone(),
                                cafe_id,
                                article_id: article.article_id,
                            });
                        }
                    }
                    // 조회 실패 → 이 대상은 댓글을 못 단다. 조용히 누락하지 않고 실패로 남긴다.
                    Err(error) => {
                        tracing::warn!(%cafe_id, account = %t.account_id, code = %error.code, message = %error.message, "댓글 대상 글목록 조회 실패");
                        out.fetch_failures.push(CommentFetchFailure {
                            account_id: t.account_id.clone(),
                            cafe_id,
                            code: error.code.clone(),
                            message: error.message.clone(),
                            cafe: error.error_data.clone(),
                        });
                    }
                }
            }
        }
    }
    out
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
/// 누락되지 않게 한다(거짓 100% 방지). 계정마다 게시를 시작하기 전 협조적 취소(item_present)를
/// 확인해, 취소된 아이템의 남은 계정은 게시하지 않는다.
async fn run_forum_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    id: &str,
) -> Vec<ForumOutcome> {
    let mut outcomes = Vec::new();
    for req in plan_to_forum_requests(plan) {
        // 계정별 Chrome 게시는 비싸고 비가역적이라, 시작 전마다 취소를 확인해 멈춘다.
        if !item_present(app, id) {
            break;
        }
        let account_id = req.account_id.clone();
        // spawn_blocking 태스크가 패닉(JoinError)하면 결과를 잃으므로, 합성 실패에 쓸
        // 종목 목록을 미리 복제해 둔다(누락 대신 명시 실패).
        let stocks_for_panic = req.stocks.clone();
        let app_for_job = app.clone();
        let results = match tauri::async_runtime::spawn_blocking(move || {
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
                    .map(|s| {
                        // 인프라 실패(엔진 진입 전)는 backtrace가 없어 메시지를 trace로도 쓴다.
                        let message = format!("Chrome 실행 실패: {error}");
                        ForumPublishResult {
                            code: s.code.clone(),
                            name: s.name.clone(),
                            ok: false,
                            trace: Some(message.clone()),
                            message,
                        }
                    })
                    .collect(),
            }
        })
        .await
        {
            Ok(results) => results,
            // 블로킹 태스크 패닉 → 빈 결과로 조용히 누락하지 않고 그 계정 종목 전부 실패로 합성.
            Err(join_error) => stocks_for_panic
                .iter()
                .map(|s| {
                    let message = format!("게시 작업이 비정상 종료됐어요: {join_error}");
                    ForumPublishResult {
                        code: s.code.clone(),
                        name: s.name.clone(),
                        ok: false,
                        trace: Some(message.clone()),
                        message,
                    }
                })
                .collect(),
        };
        for result in results {
            outcomes.push(ForumOutcome {
                account_id: account_id.clone(),
                result,
            });
        }
    }
    outcomes
}

/// plan의 밴드 대상을 순차로 게시한다. 밴드는 순수 async HTTP라 forum처럼 Chrome/
/// spawn_blocking이 필요 없어 루프에서 직접 await한다. 댓글은 즉시게시(runNow) 경로와
/// 동일하게 comment/both 모드일 때만 댓글 풀 전체를 게시한 글에 모두 단다(밴드는 항상 글을
/// 새로 쓰고 그 글에 self-comment만 가능). 각 대상 게시 전 협조적 취소(item_present)를
/// 확인해, 취소된 아이템의 남은 밴드는 게시하지 않는다.
async fn run_band_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    id: &str,
) -> Vec<BandOutcome> {
    // comment/both면 댓글 풀 전체를 넘긴다. post/both는 새 글에, comment 전용은 기존
    // 글(최신/인기)에 같은 풀을 분배해 단다. post 전용 모드면 빈 슬라이스라 댓글 없음.
    let comments: &[String] = if runs_comment(plan) {
        &plan.comments
    } else {
        &[]
    };
    // 댓글 전용 모드는 새 글을 쓰지 않는다. band_publish(create_post)는 리더 승인제
    // 밴드에서 result_code=1003("리더 승인 후 등록")을 부르므로, 기존 글에 댓글을 다는
    // band_comment로 간다(즉시게시 runNow의 댓글 전용 경로와 동일).
    let comment_only = matches!(plan.kind, ModeValue::Comment);
    let mut outcomes = Vec::new();
    for t in &plan.band {
        // 밴드 게시도 비가역적이라, 시작 전마다 취소를 확인해 멈춘다(forum과 동일).
        if !item_present(app, id) {
            break;
        }
        let result = if comment_only {
            // 대상 spec에서 정렬·개수를 꺼낸다. 밴드는 url 미지원이라 latest로 편다.
            // spec이 없으면(비정상) 최신글 1개 기본 — 어떤 경우에도 새 글은 쓰지 않는다.
            let (sort, count) = match &t.comment_target {
                Some(spec) => (
                    if matches!(spec.mode, CommentTarget::Popular) {
                        BandFeedSort::Popular
                    } else {
                        BandFeedSort::Latest
                    },
                    spec.count.unwrap_or(1).max(1),
                ),
                None => (BandFeedSort::Latest, 1),
            };
            band_comment(&t.account_id, &t.link, sort, count, comments)
                .await
                .map(BandJobResult::Commented)
        } else {
            band_publish(
                &t.account_id,
                &t.link,
                &plan.title,
                &plan.body_text,
                comments,
            )
            .await
            .map(BandJobResult::Published)
        };
        outcomes.push(BandOutcome {
            account_id: t.account_id.clone(),
            band_name: t.name.clone(),
            result,
        });
    }
    outcomes
}

/// 로그인 1건의 결과를 (계정 상태, 사용자 사유, 자세히보기 trace)로 해석한다(순수). Ok면
/// resolution의 세밀 상태/메시지/trace를 그대로, Err(인프라 오류)면 Error + 오류 문자열(trace
/// 없음)로 본다. trace는 CDP 실패(AutomationError)에서만 채워져 "자세히 보기"에 노출된다(#210).
fn resolve_login_status(
    result: &Result<LoginResolution, OrchestratorError>,
) -> (AccountStatus, String, Option<String>) {
    match result {
        Ok(res) => (res.status.clone(), res.message.clone(), res.trace.clone()),
        Err(err) => (AccountStatus::Error, err.to_string(), None),
    }
}

/// 로그인 전용 아이템을 처리한다(#210): 계정을 순서대로 로그인하고, 1건마다 진행률과
/// 계정 상태(`apply_status_by_login_id`)·활동 피드를 갱신한다. 성공/실패와 무관하게 항상
/// 다음 계정으로 진행해(한 계정 실패가 큐를 멈추지 않게) 모든 계정을 성공 또는 실패로
/// 확정한다. `platform`이 Band면 band.us 로그인, 그 외(naver/forum 등)는 네이버 로그인으로
/// 보낸다(프론트 runLogin 분기 미러). 각 계정 시작 전 협조적 취소(item_present)를 확인한다.
///
/// 결과 표시: 계정별 활동 피드(알림 화면) + 계정 상태 배지 + 완료 시 OS 토스트로 충분하므로,
/// 알림 로그(LogBatch)에는 **남기지 않는다**(중복 제거). 실패 사유·백트레이스(trace)는 디버깅용
/// 으로 `pstmacro.log`에만 기록한다(전용 로그인 큐 제거로 사라졌던 `[LOGIN]` 로그도 복원).
async fn run_login_targets<R: Runtime>(app: &AppHandle<R>, id: &str, targets: &[LoginTarget]) {
    use crate::auth::mask_id;
    use crate::auth::outcome::{activity_message, status_activity_type};
    use crate::ipc::accounts::{apply_status_by_login_id, Account};

    let total = targets.len() as u32;
    update_progress(app, id, 0, total);
    let mut done = 0u32;
    let mut ok = 0u32;
    for t in targets {
        // 로그인은 비싸고(브라우저 기동) 비가역적이라 시작 전마다 취소를 확인한다.
        if !item_present(app, id) {
            break;
        }
        let result = if matches!(t.platform, PlatformId::Band) {
            crate::band_auth::process_band_account(
                app,
                &t.account_id,
                t.headless,
                t.use_adb,
                t.force,
            )
            .await
        } else {
            crate::auth::process_account(app, &t.account_id, t.headless, t.use_adb, t.force).await
        };
        let (status, msg, trace) = resolve_login_status(&result);
        // 성공 여부는 resolution의 succeeded(쿠키 저장 완료)로 판정한다(기존 전용 로그인 큐와 동일).
        let succeeded = matches!(&result, Ok(res) if res.succeeded);
        if succeeded {
            ok += 1;
        }

        // 계정 세밀 상태/사유를 accounts 스토어에 반영한다(loginId가 같은 모든 행). 프론트
        // accounts 화면이 이 값을 폴링해 상태 배지/tooltip을 갱신한다.
        app.state::<JsonStore<Account>>().mutate(|list| {
            apply_status_by_login_id(list, &t.account_id, status.clone(), Some(msg.clone()))
        });
        // 활동 피드에도 상태별 타입으로 남긴다(기존 전용 로그인 큐와 동일 UX).
        record(
            app.state::<JsonStore<ActivityItem>>().inner(),
            status_activity_type(&status),
            activity_message(&t.account_id, &status, &msg),
        );
        // pstmacro.log에 성공/실패를 남긴다(알림 로그 UI는 중복이라 안 남김). 실패는 사유와
        // 백트레이스(trace)를 함께 기록해 디버깅에 쓴다(#210). PW는 어떤 로그에도 넣지 않는다.
        if succeeded {
            tracing::info!("[LOGIN] {} 로그인 성공 ✅", mask_id(&t.account_id));
        } else if let Some(tr) = &trace {
            tracing::warn!(
                "[LOGIN] {} 로그인 실패 ❌ — {msg}\n{tr}",
                mask_id(&t.account_id)
            );
        } else {
            tracing::warn!("[LOGIN] {} 로그인 실패 ❌ — {msg}", mask_id(&t.account_id));
        }

        done += 1;
        update_progress(app, id, done, total);
    }

    // 완료 시 OS 토스트로 결과를 통지한다(게시 완료 토스트와 동일 UX, #163/#210). 트레이
    // 상주로 창을 닫아둔 경우에도 인지할 수 있게 한다. 알림 로그(LogBatch)는 만들지 않는다.
    if total > 0 {
        let (toast_title, toast_body) =
            super::notify::login_completion_message(total as usize, ok as usize);
        super::notify::notify_desktop(app, &toast_title, &toast_body);
    }
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

/// both(쓴 글에 self-comment)용: 각 대상(쓴 글)에 템플릿의 **모든** 비어있지 않은 댓글을
/// 각각 단다. 분배(1개씩)와 달리 글 하나에 댓글 풀 전체가 올라간다. 빈 댓글은 건너뛴다.
fn build_self_comment_jobs(
    targets: Vec<CommentTargetEntry>,
    comments: &[String],
) -> Vec<CommentJob> {
    let texts: Vec<&String> = comments.iter().filter(|c| !c.trim().is_empty()).collect();
    let mut jobs = Vec::new();
    for t in &targets {
        for content in &texts {
            jobs.push(CommentJob {
                account_id: t.account_id.clone(),
                cafe_id: t.cafe_id,
                article_id: t.article_id,
                content: (*content).clone(),
            });
        }
    }
    jobs
}

fn status_of(ok: bool) -> BatchItemStatus {
    if ok {
        BatchItemStatus::Success
    } else {
        BatchItemStatus::Fail
    }
}

/// 매핑되지 않은 알 수 없는 실패의 메인 라인 폴백(#169). 원문(영어/개발자 메시지)을
/// 사용자에게 쏟지 않고, 일반 안내 + "자세히 보기"(trace)로 유도한다.
const GENERIC_REASON: &str = "알 수 없는 오류가 발생했습니다. 자세히 보기를 확인해 주세요";

/// HTTP 상태코드를 사용자용 한국어 사유로 매핑한다(#169).
fn status_reason(status: u16) -> &'static str {
    match status {
        400 => "요청 형식이 올바르지 않습니다",
        401 | 403 => "권한이 없거나 로그인이 만료되었습니다",
        404 => "해당 게시판을 찾을 수 없습니다",
        429 => "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요",
        500..=599 => "네이버 서버에 문제가 발생했습니다",
        _ => "네이버에서 요청을 거부했습니다",
    }
}

/// 네이버 API errorCode를 사용자용 한국어 사유로 매핑한다(#169). 이 매핑은 네이버
/// 원문보다 **우선**한다 — 원문이 영어("Page Not Found")거나, 한국어여도 모호한
/// ("알 수 없는 오류" = 0001) 경우를 행동 가능한 문구로 덮어쓴다. 매핑에 없는 코드는
/// 네이버 한국어 원문을 그대로 살린다(예: 4003 "삭제되었거나 존재하지 않는 게시글입니다").
fn naver_code_reason(api_code: &str) -> Option<&'static str> {
    match api_code {
        "10404" => Some("해당 게시판 또는 게시물이 존재하지 않습니다"),
        "9999" => Some("네이버에서 요청을 거부했습니다"),
        // 0001: 네이버가 "알 수 없는 오류"로만 답하는 일반 코드 — 잘못된 댓글 대상에서 관측됨.
        "0001" => Some("댓글 대상을 찾을 수 없거나 잘못된 요청입니다"),
        _ => None,
    }
}

/// 내부 오류 코드(HTTP 단계 이전 실패)를 사용자용 한국어 사유로 매핑한다(#169).
/// 개발자용 원본 message("contentJson…")가 그대로 노출되지 않게 코드로 치환한다.
fn internal_code_reason(code: &str) -> Option<&'static str> {
    match code {
        "SESSION_INVALID" => Some("로그인이 만료되었습니다. 다시 로그인해 주세요"),
        "NO_COOKIES" => Some("로그인 정보가 없습니다. 먼저 로그인해 주세요"),
        "CONTENT_BUILD_FAILED" | "FORM_BUILD_FAILED" => {
            Some("글 내용을 구성하는 중 문제가 발생했습니다")
        }
        "REGISTER_PARSE_ERROR" | "COMMENT_PARSE_ERROR" | "PARSE_ERROR" => {
            Some("네이버 응답을 처리하지 못했습니다")
        }
        "HTTP_TRANSPORT_ERROR" => Some("네트워크 연결에 문제가 있습니다"),
        "INVALID_CAFE_INPUT" => Some("카페 또는 게시판 정보가 올바르지 않습니다"),
        _ => None,
    }
}

/// 문자열에 한글(음절/자모)이 들어 있는지. 네이버가 준 실패 사유가 한국어면(댓글
/// `reason`처럼 이미 친절) 그대로 노출하고, 영어/기술 메시지("Page Not Found")일 때만
/// 우리 매핑으로 치환하기 위한 판별(#169).
fn contains_hangul(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c,
            '\u{AC00}'..='\u{D7A3}'   // 완성형 음절
            | '\u{1100}'..='\u{11FF}' // 자모
            | '\u{3130}'..='\u{318F}' // 호환 자모
        )
    })
}

/// 실패한 게시/댓글의 사용자용 메인 라인 사유. 한국어 사유 뒤에 식별 코드를 짧게 붙여,
/// "알 수 없는 오류"처럼 모호한 경우에도 무슨 에러인지 추적할 수 있게 한다(#169). 코드는
/// 네이버 숫자 errorCode를 우선 쓰고, 없으면 내부 오류 코드(예: SESSION_INVALID)를 쓴다.
fn failure_reason(code: &str, cafe: Option<&NaverCafeCommonErrorData>) -> String {
    let reason = resolve_failure_reason(code, cafe);
    let tag = cafe
        .and_then(|c| c.api_error_code.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or(code);
    format!("{reason} ({tag})")
}

/// 메인 라인의 한국어 사유 본문을 고른다(코드 태그 제외). 우선순위: 네이버 errorCode
/// 매핑(모호/영어 코드 치환) → 네이버 원문이 한국어면 그대로 → HTTP status(4xx/5xx)
/// 매핑 → 내부 코드 매핑 → 일반 폴백. 영어/기술 원문이나 개발자 `message`는 넣지 않고,
/// 디버그 원문은 `failure_trace`(자세히 보기)에만 보존한다.
fn resolve_failure_reason(code: &str, cafe: Option<&NaverCafeCommonErrorData>) -> String {
    let Some(cafe) = cafe else {
        return internal_code_reason(code)
            .unwrap_or(GENERIC_REASON)
            .to_owned();
    };
    // 1. 알려진 네이버 errorCode 매핑이 최우선 — 원문이 영어거나 한국어여도 모호한
    //    코드(0001 등)를 행동 가능한 문구로 덮어쓴다.
    if let Some(reason) = cafe.api_error_code.as_deref().and_then(naver_code_reason) {
        return reason.to_owned();
    }
    // 2. 매핑에 없으면, 네이버 원문이 한국어인 경우 그대로 노출(4003 등 이미 친절한 사유).
    //    개행·연속 공백 정리, 길이 가드로 파싱 실패 폴백의 원문 바디 방지.
    if let Some(msg) = cafe.api_error_message.as_deref() {
        let cleaned = msg.split_whitespace().collect::<Vec<_>>().join(" ");
        if contains_hangul(&cleaned) && cleaned.chars().count() <= 200 {
            return cleaned;
        }
    }
    // 3. HTTP 오류 status 기반 한국어. 2xx("200 OK + 에러 본문")는 status에 오류 의미가
    //    없으므로 건너뛴다.
    if let Some(status) = cafe.http_status {
        if status >= 400 {
            return status_reason(status).to_owned();
        }
    }
    // 4. HTTP 이전 내부 오류 코드 → 그조차 없으면 일반 폴백.
    internal_code_reason(code)
        .unwrap_or(GENERIC_REASON)
        .to_owned()
}

/// "자세히보기"용 개발자 trace를 만든다(#169). 코드·HTTP status·api 코드·원본
/// 메시지를 한 줄로 남겨, 사용자 사유와 별개로 실제 응답을 그대로 확인할 수 있게 한다.
fn failure_trace(code: &str, message: &str, cafe: Option<&NaverCafeCommonErrorData>) -> String {
    // 한 줄 헤더(코드·HTTP·errorCode)만 붙이고 message는 원본 그대로 둔다 — message에는 실패
    // 지점에서 캡처한 호출 스택이 그대로 들어있다(#199, 댓글 클라이언트 등). 스택은 구조화하지
    // 않고 원본 형식 그대로 노출한다.
    let header = match cafe {
        Some(c) => {
            let status = c
                .http_status
                .map_or_else(|| "-".to_owned(), |s| s.to_string());
            let ec = c
                .api_error_code
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("-");
            format!("{code} · HTTP {status} · errorCode {ec}")
        }
        None => code.to_owned(),
    };
    format!("{header}\n{message}")
}

/// 밴드 게시 실패의 사용자용 메인 라인 사유(#199). 기술 상세(원문/HTTP 바디)는 빼고 무엇이
/// 잘못됐는지만 짧게 — 카페 `failure_reason`과 같은 철학. 디버그 원문은 `band_failure_trace`로.
pub(crate) fn band_failure_reason(err: &BandPostError) -> String {
    match &err.kind {
        BandPostErrorKind::InvalidLink(_) => "밴드 링크가 올바르지 않습니다".to_owned(),
        BandPostErrorKind::NoSession => {
            "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요".to_owned()
        }
        BandPostErrorKind::NoSecretKey(_) => "밴드 서명 키 발급에 실패했습니다".to_owned(),
        BandPostErrorKind::Transport(_) => "네트워크 연결에 문제가 있습니다".to_owned(),
        BandPostErrorKind::Http { status, .. } => band_http_reason(*status).to_owned(),
        // band가 준 사유가 한국어면 그대로(이미 사람이 읽을 설명), 아니면 일반 문구.
        BandPostErrorKind::Api(api) if contains_hangul(&api.message) => api.message.clone(),
        BandPostErrorKind::Api(_) => "밴드에서 게시를 거부했습니다".to_owned(),
    }
}

/// 밴드 HTTP 오류 status를 사용자용 한국어 사유로(#199).
fn band_http_reason(status: u16) -> &'static str {
    match status {
        401 | 403 => "밴드 로그인이 만료되었거나 권한이 없습니다",
        429 => "요청이 너무 많습니다. 잠시 후 다시 시도해 주세요",
        500..=599 => "밴드 서버에 문제가 발생했습니다",
        _ => "밴드에서 요청을 거부했습니다",
    }
}

/// "자세히 보기"용 밴드 개발자 trace(#199). 변형·status·원문 등 기술 상세를 한 줄로 남겨
/// 사용자 사유(`band_failure_reason`)와 별개로 실제 오류를 확인할 수 있게 한다.
pub(crate) fn band_failure_trace(err: &BandPostError) -> String {
    let detail = match &err.kind {
        BandPostErrorKind::InvalidLink(link) => format!("code: BAND_INVALID_LINK\nlink: {link}"),
        BandPostErrorKind::NoSession => "code: BAND_NO_SESSION".to_owned(),
        BandPostErrorKind::NoSecretKey(detail) => {
            format!("code: BAND_NO_SECRET_KEY\ndetail: {detail}")
        }
        BandPostErrorKind::Transport(msg) => format!("code: BAND_TRANSPORT\ndetail: {msg}"),
        BandPostErrorKind::Http { status, body } => {
            format!(
                "code: BAND_HTTP\nHTTP status: {status}\nbody: {}",
                trace_snippet(body)
            )
        }
        BandPostErrorKind::Api(api) => format!(
            "code: BAND_API\nresult_code: {}\nmessage: {}",
            api.result_code
                .map_or_else(|| "-".to_owned(), |c| c.to_string()),
            api.message
        ),
    };
    // 기술 상세 + 에러 생성 지점에서 캡처한 런타임 호출 스택(#199, 카페/종토방과 동일). 실패
    // 지점 호출 경로는 backtrace 상위 프레임(BandPostError::new 직후)에 나온다.
    format!("{detail}\n\n{}", err.backtrace)
}

/// trace에 넣을 본문 스니펫 — 너무 길지 않게 문자 경계로 자른다(바이트 슬라이스 패닉 방지).
fn trace_snippet(s: &str) -> String {
    const MAX: usize = 300;
    if s.chars().count() <= MAX {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(MAX).collect::<String>())
    }
}

/// 카페 글·댓글·종목토론방·밴드 실행 결과를 알림 배치(`LogBatch`) 한 건으로 묶는다.
/// 본문/댓글 스냅샷은 실제 그 작업을 돌린 모드일 때만 남긴다(즉시게시 forum 경로와 동일).
// 플랫폼별 결과 슬라이스(글/댓글/forum/band)와 조회 실패·시각·seq를 그대로 받는다.
// 플랫폼이 늘며 인자가 7개를 넘지만, 한 곳에서 LogBatch로 합치는 평탄한 빌더라 묶음
// 구조체로 감싸기보다 인자로 두는 편이 읽기 쉽다.
#[allow(clippy::too_many_arguments)]
fn build_log_batch(
    plan: &PublishPlan,
    post_reports: &[JobReport],
    comment_reports: &[CommentJobReport],
    forum_outcomes: &[ForumOutcome],
    band_outcomes: &[BandOutcome],
    comment_fetch_failures: &[CommentFetchFailure],
    at: i64,
    seq: u64,
) -> LogBatch {
    use std::collections::HashMap;

    // 카페 ID(문자열) → 표시 이름. plan에 동결된 이름을 써서 로그에 ID 대신 카페 명을
    // 보여준다. 이름이 비었거나 매칭이 없으면 ID로 폴백한다.
    let cafe_names: HashMap<&str, &str> = plan
        .naver
        .iter()
        .filter(|t| !t.cafe_name.is_empty())
        .map(|t| (t.cafe.as_str(), t.cafe_name.as_str()))
        .collect();
    let cafe_label = |id: &str| -> String {
        cafe_names
            .get(id)
            .map_or_else(|| id.to_owned(), |n| n.to_string())
    };

    let mut items = Vec::new();

    for r in post_reports {
        items.push(BatchItem {
            platform: PlatformId::Naver,
            target: cafe_label(&r.cafe),
            code: None,
            board: Some(r.menu_id.to_string()),
            login_id: r.account_id.clone(),
            status: status_of(r.success),
            // msg/status를 같은 기준(success)으로 묶는다 — 실패인데 오류가 비면(불변식
            // 위반 시) "완료"로 오인되지 않게.
            msg: if r.success {
                "글 게시 완료".to_owned()
            } else {
                match r.error.as_ref() {
                    Some(e) => format!(
                        "글 게시 실패 — {}",
                        failure_reason(&e.code, e.error_data.as_ref().map(|d| &d.cafe))
                    ),
                    None => "글 게시 실패".to_owned(),
                }
            },
            trace: r.error.as_ref().map(|e| {
                failure_trace(&e.code, &e.message, e.error_data.as_ref().map(|d| &d.cafe))
            }),
        });
    }

    for r in comment_reports {
        items.push(BatchItem {
            platform: PlatformId::Naver,
            target: cafe_label(&r.cafe_id.to_string()),
            code: None,
            board: None,
            login_id: r.account_id.clone(),
            status: status_of(r.success),
            msg: if r.success {
                "댓글 게시 완료".to_owned()
            } else {
                match r.error.as_ref() {
                    Some(e) => format!(
                        "댓글 게시 실패 — {}",
                        failure_reason(&e.code, e.error_data.as_ref().map(|d| &d.cafe))
                    ),
                    None => "댓글 게시 실패".to_owned(),
                }
            },
            trace: r.error.as_ref().map(|e| {
                failure_trace(&e.code, &e.message, e.error_data.as_ref().map(|d| &d.cafe))
            }),
        });
    }

    // 글목록 조회 실패로 댓글을 시도조차 못 한 대상 — 조용히 빼지 않고 실패로 남긴다.
    for f in comment_fetch_failures {
        items.push(BatchItem {
            platform: PlatformId::Naver,
            target: cafe_label(&f.cafe_id.to_string()),
            code: None,
            board: None,
            login_id: f.account_id.clone(),
            status: BatchItemStatus::Fail,
            // 카페 글/댓글과 동일: 메인=친절 사유, 자세히=개발자 trace(#199).
            msg: format!(
                "댓글 대상 글 조회 실패 — {}",
                failure_reason(&f.code, f.cafe.as_ref())
            ),
            trace: Some(failure_trace(&f.code, &f.message, f.cafe.as_ref())),
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
            // 성공은 엔진 문구("게시 완료"), 실패는 일반 친절 문구. 캡처된 호출 스택은
            // ForumPublishResult.trace(자세히 보기)로 분리해 메인 라인엔 안 싣는다(#199).
            msg: if o.result.ok {
                o.result.message.clone()
            } else {
                "종목토론방 게시에 실패했습니다".to_owned()
            },
            trace: o.result.trace.clone(),
        });
    }

    for o in band_outcomes {
        // post/both는 새 글(+댓글), comment 전용은 기존 글 댓글. 둘 다 부분 실패를 드러낸다
        // (성공분이 모자라면 성공으로 묻지 않는다). 메인=친절 문구, 자세히=기술 trace로 나눈다
        // (실패만 trace; 부분 실패도 성공/시도 수를 trace로 남긴다)(#199).
        let (status, msg, trace) = match &o.result {
            Ok(BandJobResult::Published(out)) => {
                let ok = out.comment_total == 0 || out.commented_count >= out.comment_total;
                let msg = if out.comment_total > 0 {
                    format!(
                        "글·댓글 {}/{}개 게시 완료",
                        out.commented_count, out.comment_total
                    )
                } else {
                    "글 게시 완료".to_owned()
                };
                let trace = (!ok).then(|| {
                    format!(
                        "BAND_PARTIAL · 댓글 {}/{}건 게시",
                        out.commented_count, out.comment_total
                    )
                });
                (status_of(ok), msg, trace)
            }
            Ok(BandJobResult::Commented(out)) => {
                // 한 건도 못 달면(대상 글 없음/전부 실패) 실패로 둔다(즉시게시 판정과 동일).
                let ok = out.commented_count > 0;
                let msg = if out.target_count > 0 {
                    format!(
                        "댓글 {}/{}개 게시 완료",
                        out.commented_count, out.target_count
                    )
                } else {
                    "댓글 대상 글 없음".to_owned()
                };
                let trace = if ok {
                    None
                } else if out.target_count > 0 {
                    Some(format!(
                        "BAND_COMMENT_FAIL · 대상 {}건 중 0건 게시",
                        out.target_count
                    ))
                } else {
                    Some("BAND_NO_TARGET · 댓글 대상 글을 찾지 못함".to_owned())
                };
                (status_of(ok), msg, trace)
            }
            Err(e) => (
                BatchItemStatus::Fail,
                band_failure_reason(e),
                Some(band_failure_trace(e)),
            ),
        };
        items.push(BatchItem {
            platform: PlatformId::Band,
            target: o.band_name.clone(),
            code: None,
            board: None,
            login_id: o.account_id.clone(),
            status,
            msg,
            trace,
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

/// 완료 배치를 로그 스토어 맨 앞에 넣는다(최신순, 최대 MAX_LOG_BATCHES건 유지).
fn store_log_batch<R: Runtime>(app: &AppHandle<R>, batch: LogBatch) {
    app.state::<JsonStore<LogBatch>>().mutate(|mut v| {
        v.insert(0, batch);
        v.truncate(MAX_LOG_BATCHES);
        v
    });
}

/// 성공/전체 개수로 activity 타입을 정한다 — 전부 성공이면 Success, 전부 실패면 Error,
/// 일부 성공이면 Info.
fn activity_type_for(ok: usize, total: usize) -> ActivityType {
    if ok == total {
        ActivityType::Success
    } else if ok == 0 {
        ActivityType::Error
    } else {
        ActivityType::Info
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

    store_log_batch(app, batch);

    let activity = app.state::<JsonStore<ActivityItem>>();
    record(
        activity.inner(),
        activity_type_for(ok, total),
        format!("'{title}' 예약 게시 — {total}곳 중 {ok}곳 성공"),
    );

    // 트레이 상주(창 닫힘) 중에도 결과를 인지하도록 OS 토스트도 best-effort로 띄운다(#163).
    let (toast_title, toast_body) = super::notify::completion_message(&title, total, ok);
    super::notify::notify_desktop(app, &toast_title, &toast_body);
}

/// plan으로부터 진행률 total의 상한 추정치를 낸다. mark_running 시점에 `(0, total)`을
/// 미리 채워, 카페 글 게시·댓글 대상 조회(both/comment는 네트워크)로 실제 total이
/// 확정되기 전에도 "처리 중 N/N"이 빈칸("/")으로 보이지 않게 한다. 이후 execute_item이
/// 실제 작업 수로 정밀화한다(실패·빈 풀로 실제치가 더 작아질 수 있다).
fn estimate_total(plan: &PublishPlan) -> u32 {
    // 로그인 전용 아이템(#210)은 진행률 분모가 계정 수로 확정돼 있다(게시 추정과 별개).
    if let Some(login) = plan.login.as_ref().filter(|l| !l.is_empty()) {
        return login.len() as u32;
    }
    let posts = if runs_post(plan) { plan.naver.len() } else { 0 };
    let comments = if runs_comment(plan) {
        if matches!(plan.kind, ModeValue::Both) {
            // both: 글마다 비어있지 않은 댓글 전부 — 글 수 × 댓글 수.
            let n_comments = plan
                .comments
                .iter()
                .filter(|c| !c.trim().is_empty())
                .count();
            plan.naver.len() * n_comments
        } else {
            // comment 전용: 대상별 commentTarget(latest/popular=count, url=1) 합.
            plan.naver
                .iter()
                .filter_map(|t| t.comment_target.as_ref())
                .map(|s| match s.mode {
                    CommentTarget::Url => 1,
                    CommentTarget::Latest | CommentTarget::Popular => {
                        s.count.unwrap_or(1).max(1) as usize
                    }
                })
                .sum()
        }
    } else {
        0
    };
    (posts + comments + plan.forum.len() + plan.band.len()) as u32
}

/// 아이템을 `Running`으로 전이하고 진행률을 `(0, 추정 total)`로 초기화한다. execute_item이
/// 실제 작업 수로 total을 정밀화하기 전까지 진행률이 빈칸으로 보이지 않게 한다.
fn mark_running(mut items: Vec<QueueNowItem>, id: &str) -> Vec<QueueNowItem> {
    for item in &mut items {
        if item.id == id {
            item.state = QueueState::Running;
            let total = item.plan.as_ref().map_or(0, estimate_total);
            item.progress = Some((0, total));
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
    use crate::ipc::queue::{BandTarget, CommentTargetSpec, NaverTarget};
    use crate::naver_cafe::orchestrator::JobReport;
    use crate::naver_cafe::post::parser::ArticleRegisterResult;

    fn naver_target(account: &str) -> NaverTarget {
        NaverTarget {
            account_id: account.into(),
            cafe: "123".into(),
            cafe_name: "테스트카페".into(),
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
            band: vec![],
            login: None,
        }
    }

    fn login_target(account: &str, platform: PlatformId) -> LoginTarget {
        LoginTarget {
            account_id: account.into(),
            platform,
            headless: false,
            use_adb: false,
            force: true,
        }
    }

    #[test]
    fn resolve_login_status_maps_ok_resolution_and_infra_error() {
        // Ok(active) → Active, Ok(failure) → 세밀 상태+사유 보존, trace 없음.
        let ok = Ok(LoginResolution::active());
        assert_eq!(resolve_login_status(&ok).0, AccountStatus::Active);
        let bad: Result<LoginResolution, OrchestratorError> = Ok(LoginResolution::failure(
            AccountStatus::BadCredentials,
            "비밀번호 오류",
        ));
        let (status, msg, trace) = resolve_login_status(&bad);
        assert_eq!(status, AccountStatus::BadCredentials);
        assert_eq!(msg, "비밀번호 오류");
        assert_eq!(trace, None);
        // Err(인프라 오류) → Error + 오류 문자열, trace 없음.
        let err: Result<LoginResolution, OrchestratorError> =
            Err(OrchestratorError::AccountNotFound("user01".into()));
        let (status, msg, trace) = resolve_login_status(&err);
        assert_eq!(status, AccountStatus::Error);
        assert!(!msg.is_empty());
        assert_eq!(trace, None);
    }

    #[test]
    fn resolve_login_status_carries_trace_for_detail_view() {
        // CDP 실패에서 온 trace는 그대로 전달돼 알림 로그 "자세히 보기"에 노출된다(#210).
        let with_trace: Result<LoginResolution, OrchestratorError> =
            Ok(LoginResolution::failure_with_trace(
                AccountStatus::Error,
                "연결 실패",
                Some("at x.rs:1:1\n\nframe0".to_owned()),
            ));
        let (status, _msg, trace) = resolve_login_status(&with_trace);
        assert_eq!(status, AccountStatus::Error);
        assert_eq!(trace.as_deref(), Some("at x.rs:1:1\n\nframe0"));
    }

    #[test]
    fn estimate_total_uses_login_account_count() {
        // 로그인 전용 아이템의 진행률 분모는 계정 수다(게시 필드는 무시).
        let mut p = plan(ModeValue::Post, vec![naver_target("a"), naver_target("b")]);
        p.login = Some(vec![
            login_target("a", PlatformId::Naver),
            login_target("b", PlatformId::Band),
            login_target("c", PlatformId::Naver),
        ]);
        assert_eq!(estimate_total(&p), 3);
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
                trace: None,
            },
        }
    }

    fn forum_fail(account: &str, name: &str, code: &str, trace: &str) -> ForumOutcome {
        ForumOutcome {
            account_id: account.into(),
            result: ForumPublishResult {
                code: code.into(),
                name: name.into(),
                ok: false,
                message: "엔진 오류".into(),
                trace: Some(trace.into()),
            },
        }
    }

    fn band_target(account: &str, name: &str, link: &str) -> BandTarget {
        BandTarget {
            account_id: account.into(),
            name: name.into(),
            link: link.into(),
            comment_target: None,
        }
    }

    fn band_ok(
        account: &str,
        name: &str,
        commented_count: usize,
        comment_total: usize,
    ) -> BandOutcome {
        BandOutcome {
            account_id: account.into(),
            band_name: name.into(),
            result: Ok(BandJobResult::Published(BandPublishOutcome {
                joined: true,
                post_no: 100,
                web_url: "https://band.us/band/1/post/100".into(),
                commented_count,
                comment_total,
                band_name: Some(name.into()),
            })),
        }
    }

    fn band_commented(
        account: &str,
        name: &str,
        commented_count: usize,
        target_count: usize,
    ) -> BandOutcome {
        BandOutcome {
            account_id: account.into(),
            band_name: name.into(),
            result: Ok(BandJobResult::Commented(BandCommentOutcome {
                target_count,
                commented_count,
                band_name: Some(name.into()),
            })),
        }
    }

    fn band_fail(account: &str, name: &str) -> BandOutcome {
        BandOutcome {
            account_id: account.into(),
            band_name: name.into(),
            result: Err(BandPostError::no_session()),
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
    fn mark_running_sets_state_and_initial_progress() {
        // plan 없으면 total 0이라도 progress는 채워 빈칸("/")을 막는다.
        let next = mark_running(vec![now_item("a", QueueState::Waiting, None)], "a");
        assert_eq!(next[0].state, QueueState::Running);
        assert_eq!(next[0].progress, Some((0, 0)));
    }

    #[test]
    fn mark_running_seeds_total_from_plan() {
        // both + naver 2개 → 글 2 + self-댓글 2 = 4를 미리 채운다.
        let p = plan(
            ModeValue::Both,
            vec![naver_target("u0"), naver_target("u1")],
        );
        let next = mark_running(vec![now_item("a", QueueState::Waiting, Some(p))], "a");
        assert_eq!(next[0].progress, Some((0, 4)));
    }

    #[test]
    fn estimate_total_counts_posts_comments_forum() {
        use crate::ipc::queue::{CommentTargetSpec, ForumTarget};
        // post 전용: 글 대상 수.
        assert_eq!(
            estimate_total(&plan(ModeValue::Post, vec![naver_target("u0")])),
            1
        );
        // comment 전용 latest count=3 → 3.
        let mut t = naver_target("u0");
        t.comment_target = Some(CommentTargetSpec {
            mode: CommentTarget::Latest,
            count: Some(3),
            cafe_id: Some(1),
            article_id: None,
        });
        assert_eq!(estimate_total(&plan(ModeValue::Comment, vec![t])), 3);
        // forum 종목도 더한다.
        let mut p = plan(ModeValue::Post, vec![naver_target("u0")]);
        p.forum = vec![ForumTarget {
            account_id: "u0".into(),
            name: "삼성전자".into(),
            code: "005930".into(),
        }];
        assert_eq!(estimate_total(&p), 2); // 글 1 + 종목 1
                                           // 밴드 대상 수도 더한다(kind와 무관하게 항상 1 대상 = 1).
        p.band = vec![
            band_target("u0", "투자밴드", "https://band.us/band/1"),
            band_target("u0", "정보밴드", "https://band.us/band/2"),
        ];
        assert_eq!(estimate_total(&p), 4); // 글 1 + 종목 1 + 밴드 2
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
        let targets = collect_comment_targets(&p, &reports).await.targets;
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
        let targets = collect_comment_targets(&p, &[]).await.targets;
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
    fn build_self_comment_jobs_posts_all_comments_per_article() {
        // both: 글 1개에 댓글 풀 전체(3개, 공백 제외)를 단다.
        let targets = vec![CommentTargetEntry {
            account_id: "u0".into(),
            cafe_id: 1,
            article_id: 2,
        }];
        let jobs = build_self_comment_jobs(
            targets,
            &["댓글1".into(), "  ".into(), "댓글2".into(), "댓글3".into()],
        );
        assert_eq!(jobs.len(), 3); // 공백 1개 제외
        assert!(jobs.iter().all(|j| j.article_id == 2 && j.cafe_id == 1));
        let contents: Vec<&str> = jobs.iter().map(|j| j.content.as_str()).collect();
        assert_eq!(contents, vec!["댓글1", "댓글2", "댓글3"]);
    }

    #[test]
    fn build_self_comment_jobs_empty_when_no_nonempty_comments() {
        let targets = vec![CommentTargetEntry {
            account_id: "u0".into(),
            cafe_id: 1,
            article_id: 2,
        }];
        assert!(build_self_comment_jobs(targets, &["".into(), "   ".into()]).is_empty());
    }

    #[test]
    fn activity_type_reflects_success_ratio() {
        // 전부 성공 → Success, 전부 실패 → Error, 일부 성공 → Info.
        assert!(matches!(activity_type_for(3, 3), ActivityType::Success));
        assert!(matches!(activity_type_for(0, 3), ActivityType::Error));
        assert!(matches!(activity_type_for(1, 3), ActivityType::Info));
    }

    #[test]
    fn build_log_batch_maps_post_success_and_fail() {
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        let reports = vec![
            post_report("u0", 123, 999),
            post_fail("u1", "456", "NO_COOKIES", "쿠키 없음"),
        ];
        let b = build_log_batch(&p, &reports, &[], &[], &[], &[], 1_700_000_000_000, 0);
        assert_eq!(b.id, "lb-q-1700000000000-0");
        assert_eq!(b.title, "T");
        assert_eq!(b.body.as_deref(), Some("B")); // post 모드 → 본문 스냅샷
        assert!(b.comment.is_none()); // post 모드 → 댓글 스냅샷 없음
        assert_eq!(b.items.len(), 2);
        assert_eq!(b.items[0].platform, PlatformId::Naver);
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
        // 카페 ID("123")가 아니라 plan에 동결된 카페 명으로 로그를 남긴다.
        assert_eq!(b.items[0].target, "테스트카페");
        assert_eq!(b.items[0].board.as_deref(), Some("7"));
        assert_eq!(b.items[1].status, BatchItemStatus::Fail);
        // plan에 없는 카페("456")는 ID로 폴백.
        assert_eq!(b.items[1].target, "456");
        // error_data가 없으면 내부 코드(NO_COOKIES)를 한국어 사유로 치환하고 식별 코드를 붙인다.
        assert_eq!(
            b.items[1].msg,
            "글 게시 실패 — 로그인 정보가 없습니다. 먼저 로그인해 주세요 (NO_COOKIES)"
        );
        // 디버그 원문은 자세히 보기(trace)에 헤더+원본으로 보존.
        assert_eq!(b.items[1].trace.as_deref(), Some("NO_COOKIES\n쿠키 없음"));
    }

    /// `api_error_message`/`http_status`를 담은 게시 실패 리포트(REGISTER_HTTP_ERROR 형태).
    fn cafe_error(
        status: Option<u16>,
        api_code: Option<&str>,
        api_message: Option<&str>,
    ) -> NaverCafeCommonErrorData {
        NaverCafeCommonErrorData {
            target: None,
            http_status: status,
            api_error_code: api_code.map(str::to_owned),
            api_error_message: api_message.map(str::to_owned),
            retryable: false,
        }
    }

    fn post_fail_api(account: &str, cafe: &str, cafe_err: NaverCafeCommonErrorData) -> JobReport {
        use crate::naver_cafe::post::error::PostErrorData;
        use crate::naver_cafe::ErrorEnvelope;
        JobReport {
            account_id: account.into(),
            cafe: cafe.into(),
            menu_id: 7,
            success: false,
            result: None,
            error: Some(ErrorEnvelope {
                trace_id: "t".into(),
                code: "REGISTER_HTTP_ERROR".into(),
                message: "오류 응답은 apiErrorMessage를 확인하세요".into(),
                error_data: Some(PostErrorData {
                    cafe: cafe_err,
                    menu_id: None,
                    subject: None,
                    validation_errors: vec![],
                }),
            }),
        }
    }

    #[test]
    fn failure_reason_maps_naver_code_to_korean_without_english_or_http() {
        // 네이버 원문이 영어("Page Not Found")여도 errorCode(10404)로 한국어 치환하고
        // HTTP status는 메인 라인에 넣지 않는다.
        let cafe = cafe_error(Some(404), Some("10404"), Some("Page Not Found"));
        let reason = failure_reason("REGISTER_HTTP_ERROR", Some(&cafe));
        // 한국어 사유 + 식별 코드(네이버 errorCode 우선).
        assert_eq!(
            reason,
            "해당 게시판 또는 게시물이 존재하지 않습니다 (10404)"
        );
        assert!(!reason.contains("Page Not Found"));
        assert!(!reason.contains("HTTP"));
    }

    #[test]
    fn failure_reason_unknown_naver_code_falls_back_to_http_status() {
        // 매핑 안 된 errorCode면 HTTP status 기반 한국어 문구로.
        let cafe = cafe_error(Some(403), Some("88888"), Some("Forbidden"));
        assert_eq!(
            failure_reason("REGISTER_HTTP_ERROR", Some(&cafe)),
            "권한이 없거나 로그인이 만료되었습니다 (88888)"
        );
    }

    #[test]
    fn failure_reason_no_http_uses_internal_code() {
        // HTTP 단계 이전 실패(SESSION_INVALID)는 내부 코드로 한국어 치환(원문 message 미노출).
        let cafe = cafe_error(None, None, None);
        // api_error_code가 없으면 식별 코드로 내부 코드를 쓴다.
        assert_eq!(
            failure_reason("SESSION_INVALID", Some(&cafe)),
            "로그인이 만료되었습니다. 다시 로그인해 주세요 (SESSION_INVALID)"
        );
    }

    #[test]
    fn failure_reason_no_error_data_uses_internal_code() {
        assert_eq!(
            failure_reason("NO_COOKIES", None),
            "로그인 정보가 없습니다. 먼저 로그인해 주세요 (NO_COOKIES)"
        );
    }

    #[test]
    fn failure_reason_unknown_everything_uses_generic_hint() {
        // 코드도 status도 모르면 개발자 원문 대신 일반 안내 + 자세히 보기 유도 + 식별 코드.
        assert_eq!(
            failure_reason("WEIRD_UNKNOWN_CODE", None),
            format!("{GENERIC_REASON} (WEIRD_UNKNOWN_CODE)")
        );
    }

    #[test]
    fn failure_reason_never_leaks_developer_message() {
        // 개발자 message("contentJson…")가 메인 라인에 새지 않는다.
        let cafe = cafe_error(None, None, None);
        let reason = failure_reason("CONTENT_BUILD_FAILED", Some(&cafe));
        assert_eq!(
            reason,
            "글 내용을 구성하는 중 문제가 발생했습니다 (CONTENT_BUILD_FAILED)"
        );
        assert!(!reason.contains("contentJson"));
    }

    #[test]
    fn failure_reason_passes_through_unmapped_korean_naver_message() {
        // 매핑에 없는 코드는 네이버 한국어 원문을 그대로 노출(이미 친절). 4003 실측 케이스.
        let deleted = cafe_error(
            Some(200),
            Some("4003"),
            Some("삭제되었거나 존재하지 않는 게시글입니다."),
        );
        assert_eq!(
            failure_reason("COMMENT_HTTP_ERROR", Some(&deleted)),
            "삭제되었거나 존재하지 않는 게시글입니다. (4003)"
        );
    }

    #[test]
    fn failure_reason_overrides_vague_naver_code() {
        // 0001은 네이버가 "알 수 없는 오류"로만 답하는 모호 코드 — 한국어여도 우리 행동가능
        // 문구로 덮어쓴다(매핑이 한글 패스스루보다 우선). 댓글 "200 OK + 에러 본문" 실측.
        let vague = cafe_error(
            Some(200),
            Some("0001"),
            Some("알 수 없는 오류가 발생했습니다."),
        );
        assert_eq!(
            failure_reason("COMMENT_HTTP_ERROR", Some(&vague)),
            "댓글 대상을 찾을 수 없거나 잘못된 요청입니다 (0001)"
        );
    }

    #[test]
    fn failure_reason_2xx_non_korean_skips_status_and_uses_generic() {
        // 200(에러 본문) + 한글 아님 + 미매핑 코드 → status(200)는 오류의미 없어 건너뛰고 일반 폴백.
        let cafe = cafe_error(Some(200), Some("7777"), Some("Weird error"));
        assert_eq!(
            failure_reason("COMMENT_HTTP_ERROR", Some(&cafe)),
            format!("{GENERIC_REASON} (7777)")
        );
    }

    #[test]
    fn failure_reason_oversized_korean_body_falls_back_to_status() {
        // 파싱 실패 폴백의 긴 한국어 원문 바디는 메인 라인에 쏟지 않고 status 문구로.
        let huge = "가".repeat(300);
        let cafe = cafe_error(Some(500), None, Some(&huge));
        assert_eq!(
            failure_reason("REGISTER_HTTP_ERROR", Some(&cafe)),
            "네이버 서버에 문제가 발생했습니다 (REGISTER_HTTP_ERROR)"
        );
    }

    #[test]
    fn failure_trace_includes_status_code_and_api_detail() {
        let cafe = cafe_error(Some(403), Some("9999"), Some("권한이 없는 게시판입니다"));
        // 헤더 1줄(코드·HTTP·errorCode) + 원본 message(스택 포함 가능)를 그대로.
        assert_eq!(
            failure_trace("REGISTER_HTTP_ERROR", "일반 안내문", Some(&cafe)),
            "REGISTER_HTTP_ERROR · HTTP 403 · errorCode 9999\n일반 안내문"
        );
    }

    #[test]
    fn failure_trace_falls_back_to_code_message_without_error_data() {
        assert_eq!(
            failure_trace("NO_COOKIES", "쿠키 없음", None),
            "NO_COOKIES\n쿠키 없음"
        );
    }

    #[test]
    fn build_log_batch_post_fail_surfaces_korean_reason_in_msg_and_keeps_debug_trace() {
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        // 네이버 원문은 영어("Page Not Found"), errorCode 10404.
        let cafe = cafe_error(Some(404), Some("10404"), Some("Page Not Found"));
        let reports = vec![post_fail_api("u0", "123", cafe)];
        let b = build_log_batch(&p, &reports, &[], &[], &[], &[], 1, 0);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        // 메인 라인: 영어 원문·HTTP 없이 한국어 사유 + 식별 코드.
        assert_eq!(
            b.items[0].msg,
            "글 게시 실패 — 해당 게시판 또는 게시물이 존재하지 않습니다 (10404)"
        );
        // 자세히보기: 헤더 1줄 + 원본 message(실 경로에선 캡처된 스택이 message에 포함됨).
        assert_eq!(
            b.items[0].trace.as_deref(),
            Some("REGISTER_HTTP_ERROR · HTTP 404 · errorCode 10404\n오류 응답은 apiErrorMessage를 확인하세요")
        );
    }

    #[test]
    fn build_log_batch_comment_mode_snapshots_comment_not_body() {
        let mut p = plan(ModeValue::Comment, vec![naver_target("u0")]);
        p.comments = vec!["  ".into(), "좋은 글이네요".into()];
        let b = build_log_batch(&p, &[], &[], &[], &[], &[], 1, 0);
        assert!(b.body.is_none()); // comment 모드 → 본문 스냅샷 없음
        assert_eq!(b.comment.as_deref(), Some("좋은 글이네요")); // 공백 항목은 건너뜀
        assert!(b.items.is_empty());
    }

    #[test]
    fn build_log_batch_includes_forum_items_with_code_and_account() {
        let p = plan(ModeValue::Post, vec![]);
        let forum = vec![forum_ok("u0", "삼성전자", "005930")];
        let b = build_log_batch(&p, &[], &[], &forum, &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Forum);
        assert_eq!(b.items[0].target, "삼성전자");
        assert_eq!(b.items[0].code.as_deref(), Some("005930"));
        assert_eq!(b.items[0].login_id, "u0");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
    }

    #[test]
    fn build_log_batch_forum_fail_shows_friendly_msg_and_raw_trace() {
        // forum은 에러 코드가 없어 메인은 일반 친절 문구로, 원문은 자세히 보기(trace)로(#199).
        let p = plan(ModeValue::Post, vec![]);
        let forum = vec![forum_fail(
            "u0",
            "삼성전자",
            "005930",
            "Chrome 실행 실패: connect refused",
        )];
        let b = build_log_batch(&p, &[], &[], &forum, &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        assert_eq!(b.items[0].msg, "종목토론방 게시에 실패했습니다");
        assert_eq!(
            b.items[0].trace.as_deref(),
            Some("Chrome 실행 실패: connect refused")
        );
    }

    #[test]
    fn band_failure_reason_and_trace_split_user_and_debug() {
        // NoSession: 친절 메인 + 짧은 코드 trace.
        assert_eq!(
            band_failure_reason(&BandPostError::no_session()),
            "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요"
        );
        // trace는 코드 상세 뒤에 런타임 backtrace가 붙으므로(#199) 접두만 확인한다.
        assert!(
            band_failure_trace(&BandPostError::no_session()).starts_with("code: BAND_NO_SESSION")
        );

        // HTTP 403: status 기반 한국어 메인 + status·바디 여러 줄 trace.
        let http = BandPostError::http(403, "<html>forbidden</html>");
        assert_eq!(
            band_failure_reason(&http),
            "밴드 로그인이 만료되었거나 권한이 없습니다"
        );
        assert!(band_failure_trace(&http)
            .starts_with("code: BAND_HTTP\nHTTP status: 403\nbody: <html>forbidden</html>"));

        // InvalidLink: 링크는 메인엔 숨기고 trace에만 남긴다.
        let bad = BandPostError::invalid_link("not-a-band");
        assert_eq!(band_failure_reason(&bad), "밴드 링크가 올바르지 않습니다");
        assert!(band_failure_trace(&bad).starts_with("code: BAND_INVALID_LINK\nlink: not-a-band"));
    }

    #[test]
    fn band_failure_reason_passes_through_korean_api_message() {
        use crate::band_post::response::BandApiError;
        // band가 한국어 사유를 주면 메인에 그대로(이미 사람이 읽을 설명), trace엔 코드 동반.
        let api = BandPostError::from(BandApiError {
            result_code: Some(1003),
            message: "리더 승인 후 등록됩니다".into(),
        });
        assert_eq!(band_failure_reason(&api), "리더 승인 후 등록됩니다");
        assert!(band_failure_trace(&api)
            .starts_with("code: BAND_API\nresult_code: 1003\nmessage: 리더 승인 후 등록됩니다"));

        // 영어/기술 원문이면 메인은 일반 문구로 가리고, 원문은 trace로.
        let en = BandPostError::from(BandApiError {
            result_code: None,
            message: "forbidden".into(),
        });
        assert_eq!(band_failure_reason(&en), "밴드에서 게시를 거부했습니다");
        assert!(band_failure_trace(&en)
            .starts_with("code: BAND_API\nresult_code: -\nmessage: forbidden"));
    }

    #[test]
    fn build_log_batch_maps_band_success_with_comment_and_failure() {
        // 밴드는 forum과 별개 platform으로, 동결된 밴드명·계정과 함께 성공/실패를 남긴다.
        // 댓글 부분 실패(commented_count < comment_total)는 성공으로 묻지 않고 Fail로 둔다.
        let p = plan(ModeValue::Both, vec![]);
        let bands = vec![
            band_ok("u0", "투자밴드", 2, 2), // 글+댓글 전부 성공
            band_ok("u1", "정보밴드", 0, 0), // 글만 성공(댓글 미시도)
            band_ok("u3", "부분밴드", 1, 2), // 댓글 일부 실패 → Fail 표기
            band_fail("u2", "실패밴드"),     // 게시 실패
        ];
        let b = build_log_batch(&p, &[], &[], &[], &bands, &[], 1, 0);
        assert_eq!(b.items.len(), 4);

        assert_eq!(b.items[0].platform, PlatformId::Band);
        assert_eq!(b.items[0].target, "투자밴드");
        assert_eq!(b.items[0].login_id, "u0");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
        assert_eq!(b.items[0].msg, "글·댓글 2/2개 게시 완료");
        assert!(b.items[0].trace.is_none());

        assert_eq!(b.items[1].status, BatchItemStatus::Success);
        assert_eq!(b.items[1].msg, "글 게시 완료");

        // 댓글 일부 실패: 성공으로 묻지 않고 Fail + "N/M개"로 드러낸다(카페 commentsAllOk 동일).
        // 부분 실패도 자세히 보기에 성공/시도 수를 기술 trace로 남긴다(#199).
        assert_eq!(b.items[2].target, "부분밴드");
        assert_eq!(b.items[2].status, BatchItemStatus::Fail);
        assert_eq!(b.items[2].msg, "글·댓글 1/2개 게시 완료");
        assert_eq!(
            b.items[2].trace.as_deref(),
            Some("BAND_PARTIAL · 댓글 1/2건 게시")
        );

        assert_eq!(b.items[3].platform, PlatformId::Band);
        assert_eq!(b.items[3].target, "실패밴드");
        assert_eq!(b.items[3].status, BatchItemStatus::Fail);
        // 실패는 메인=친절 사유 / 자세히=기술 trace로 나눈다(#199).
        assert_eq!(
            b.items[3].msg,
            "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요"
        );
        // trace는 코드 상세 뒤에 런타임 backtrace가 붙으므로(#199) 접두만 확인한다.
        assert!(b.items[3]
            .trace
            .as_deref()
            .is_some_and(|t| t.starts_with("code: BAND_NO_SESSION")));
    }

    #[test]
    fn build_log_batch_maps_band_comment_only_outcomes() {
        // 댓글 전용(comment) 모드: 새 글을 쓰지 않고 기존 글에 댓글을 단 결과를 남긴다.
        // 한 건도 못 달면(대상 0/전부 실패) 성공으로 묻지 않고 Fail로 둔다(즉시게시 판정 동일).
        let p = plan(ModeValue::Comment, vec![]);
        let bands = vec![
            band_commented("u0", "투자밴드", 3, 3), // 대상 3글 전부 성공
            band_commented("u1", "정보밴드", 0, 2), // 대상 있었으나 전부 실패 → Fail
            band_commented("u2", "빈밴드", 0, 0),   // 댓글 대상 글 없음 → Fail
        ];
        let b = build_log_batch(&p, &[], &[], &[], &bands, &[], 1, 0);
        assert_eq!(b.items.len(), 3);

        assert_eq!(b.items[0].platform, PlatformId::Band);
        assert_eq!(b.items[0].target, "투자밴드");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
        assert_eq!(b.items[0].msg, "댓글 3/3개 게시 완료");
        assert!(b.items[0].trace.is_none());

        // 대상은 있었으나 전부 실패: Fail + "0/N개"로 드러낸다. 자세히엔 기술 trace.
        assert_eq!(b.items[1].status, BatchItemStatus::Fail);
        assert_eq!(b.items[1].msg, "댓글 0/2개 게시 완료");
        assert_eq!(
            b.items[1].trace.as_deref(),
            Some("BAND_COMMENT_FAIL · 대상 2건 중 0건 게시")
        );

        // 댓글 대상 글 자체가 없음: Fail + 전용 문구 + 전용 trace.
        assert_eq!(b.items[2].status, BatchItemStatus::Fail);
        assert_eq!(b.items[2].msg, "댓글 대상 글 없음");
        assert_eq!(
            b.items[2].trace.as_deref(),
            Some("BAND_NO_TARGET · 댓글 대상 글을 찾지 못함")
        );
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
        assert!(collect_comment_targets(&p, &[]).await.targets.is_empty());
    }

    #[test]
    fn build_log_batch_records_comment_fetch_failures_as_fail() {
        // 글목록 조회 실패 대상은 조용히 누락되지 않고 완료 로그에 Fail 항목으로 남는다.
        let p = plan(ModeValue::Comment, vec![naver_target("u0")]);
        let failures = vec![CommentFetchFailure {
            account_id: "u0".into(),
            cafe_id: 123,
            code: "ARTICLE_LIST_HTTP_ERROR".into(),
            message: "list fetch failed".into(),
            cafe: Some(cafe_error(Some(500), None, Some("Internal Server Error"))),
        }];
        let b = build_log_batch(&p, &[], &[], &[], &[], &failures, 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        assert_eq!(b.items[0].login_id, "u0");
        // plan에 동결된 카페명("테스트카페")으로 표시(cafe "123" 매칭).
        assert_eq!(b.items[0].target, "테스트카페");
        // 메인: 카페와 동일한 친절 사유(HTTP 500 → 한국어) + 식별 코드.
        assert_eq!(
            b.items[0].msg,
            "댓글 대상 글 조회 실패 — 네이버 서버에 문제가 발생했습니다 (ARTICLE_LIST_HTTP_ERROR)"
        );
        // 자세히보기: 헤더 1줄 + 원본 message를 메인과 별개로 보존.
        assert_eq!(
            b.items[0].trace.as_deref(),
            Some("ARTICLE_LIST_HTTP_ERROR · HTTP 500 · errorCode -\nlist fetch failed")
        );
    }
}
