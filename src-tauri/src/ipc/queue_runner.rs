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
use super::log_batches::{BatchItem, BatchItemStatus, LogBatch, PostedContent, MAX_LOG_BATCHES};
use super::posts::{CommentTarget, ModeValue};
use super::queue::{
    apply_cancel_now, apply_yield_now, item_priority, CommentTargetSpec, LoginTarget,
    PublishPlan, QueueNowItem, QueueState,
};
use crate::auth::outcome::LoginResolution;
use crate::auth::OrchestratorError;
use crate::band_post::error::{BandPostError, BandPostErrorKind};
use crate::band_post::{
    band_comment, band_comment_on_post, band_publish, BandCommentOutcome, BandFeedSort,
    BandPublishOutcome,
};
use crate::discussion_batch::{run_forum_publish, ForumPublishRequest, ForumPublishResult};
use crate::naver_automation::types::DiscussionStock;
use crate::naver_cafe::article_list::models::SortBy;
use crate::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};
use crate::naver_cafe::orchestrator::{CommentJob, CommentJobReport, JobReport, PostJob};
use crate::naver_cafe::{
    fetch_article_list_for_account, fetch_latest_articles_for_account_up_to,
    run_comment_jobs_with_events, run_post_jobs_with_progress, CommentEvent,
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
    /// 지금 동시에 돌고 있는 종목토론방 전용 아이템 수(#240). 종토방은 카페(9222)·밴드와
    /// 자원이 겹치지 않아 여러 아이템을 동시에 돌리는데, 사용자 설정 한도(#284, 0=무제한)로
    /// 제한해 자원 고갈을 막는다. 카페·밴드·로그인은 이 카운터를 쓰지 않고 기존대로 순차다.
    active_forum: usize,
    /// 지금 동시에 돌고 있는 "특정 게시글" 댓글 전용 아이템 수. 이런 아이템은 게시 시점에
    /// 저장 쿠키만 쓰고(재로그인·IP 회전 없음) 카페·밴드=HTTP, 종토방=계정별 전용 Chrome이라
    /// 자원이 겹치지 않아 동시에 돌려도 안전하다. 사용자 설정 한도(#284, 0=무제한)로 동시 수를 제한한다.
    active_comment: usize,
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

/// 네이버 블로그 댓글 게시 결과 1건(#271) — 어느 계정·블로그 글(표시 이름·URL)에 댓글을
/// 달았는지와 실행 결과를 묶는다. 블로그는 댓글 전용이라 결과는 [`BlogCommentResult`](성공)
/// 또는 [`BlogError`](실패)다. 성공/실패 모두 완료 로그(`PlatformId::Blog`)에 남긴다.
struct BlogOutcome {
    account_id: String,
    /// 표시 이름(동결). 완료 로그 라벨로 blogId 등을 보여준다.
    name: String,
    /// 댓글을 단(또는 달려던) 블로그 글 URL. 완료 로그의 "올라간 글 열기"용.
    link: String,
    /// 실제로 단 댓글 본문(토큰 치환 후). 완료 로그 posted.comment에 보존한다.
    contents: String,
    result: Result<crate::naver_blog::BlogCommentResult, crate::naver_blog::BlogError>,
}

/// 네이버 클립 댓글 게시 결과 1건(#클립) — 블로그(`BlogOutcome`)의 클립 버전. 어느 계정·미디어
/// (표시 이름·URL)에 댓글을 달았는지와 실행 결과를 묶는다. 결과는 [`ClipCommentResult`](성공)
/// 또는 [`ClipError`](실패). 성공/실패 모두 완료 로그(`PlatformId::Clip`)에 남긴다.
struct ClipOutcome {
    account_id: String,
    name: String,
    link: String,
    contents: String,
    result: Result<crate::naver_clip::ClipCommentResult, crate::naver_clip::ClipError>,
}

/// 대기(`Waiting`) 아이템 중 **우선순위가 가장 높은 것**을 고른다(`Running`은 건너뛴다).
/// 우선순위는 `item_priority`(로그인 0 > 종토 1 > 카페/밴드 2). 동순위면 `min_by_key`가
/// 먼저 나오는 것을 돌려주므로 들어온 순서(FIFO)가 보존된다(#229). 큐는 적재/재정렬 시
/// 이미 우선순위 순으로 정렬되지만, 픽에서도 우선순위를 직접 보장해 실행 순서를 못 박는다.
pub fn pick_next_waiting(items: &[QueueNowItem]) -> Option<QueueNowItem> {
    items
        .iter()
        .filter(|i| i.state == QueueState::Waiting)
        .min_by_key(|i| item_priority(i))
        .cloned()
}

/// plan의 네이버 카페 대상을 글 작성 작업(`PostJob`)으로 변환한다. 제목/본문은 예약
/// 시점에 동결된 plan 값을 쓴다(이슈 #142). 글을 쓰는 모드(post/both)에서만 의미가 있다.
pub fn plan_to_post_jobs(plan: &PublishPlan) -> Vec<PostJob> {
    // 카페는 종목이 없어 #{링크}만 치환한다(#{종목명}/#{종목코드}는 종토 전용이라 그대로 둠).
    // 링크값(linkOverride)이 있으면 그 값으로, 없으면 빈 문자열로.
    let link = crate::template_tokens::resolve_link(&plan.link_override, "");
    plan.naver
        .iter()
        .map(|t| PostJob {
            account_id: t.account_id.clone(),
            cafe: t.cafe.clone(),
            menu_id: t.menu_id,
            board_type: t.board_type.clone(),
            subject: crate::template_tokens::resolve_cafe_band(&plan.title, &link),
            body_text: crate::template_tokens::resolve_cafe_band(&plan.body_text, &link),
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

        // "특정 게시글" 댓글 전용 아이템: 게시 시점에 저장 쿠키만 쓰고(재로그인·IP 회전 없음)
        // 카페·밴드=HTTP, 종토방=계정별 전용 Chrome이라 자원이 겹치지 않는다. 종토방 레인과
        // 똑같이, 여기서 기다리지 않고 곧장 다음 아이템을 집어 댓글 아이템들이 동시에 돈다.
        // 동시 수는 사용자 설정 한도(#284, 0=무제한)로 제한한다. (종토방 전용 url 댓글도 이 레인으로
        // 와 forum 레인보다 먼저 잡히므로, 종토방 술어보다 앞서 검사한다.)
        if is_url_comment_only_item(&job) {
            // 한도는 claim 시점에 동적으로 읽는다(#284) — 사용자가 낮춰도 이미 돌고 있는
            // 작업(active에 이미 반영)은 멈추지 않고 새 claim만 active < limit까지 기다린다.
            let limit = read_concurrency_limit(&app);
            let claimed = {
                let Ok(mut inner) = runner.inner.lock() else {
                    return;
                };
                if may_claim(inner.active_comment, limit) {
                    inner.active_comment += 1;
                    true
                } else {
                    false
                }
            };
            if !claimed {
                // 동시 한도 도달 — 슬롯이 빌 때까지 잠깐 대기 후 같은 아이템을 다시 집는다.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            }
            now_store.mutate(|items| mark_running(items, &job.id));
            let app_bg = app.clone();
            let runner_bg = runner.clone();
            tauri::async_runtime::spawn(async move {
                finish_item(&app_bg, &job).await;
                if let Ok(mut inner) = runner_bg.inner.lock() {
                    inner.active_comment = inner.active_comment.saturating_sub(1);
                }
                // 남은 대기 아이템을 이어서 처리하도록 워커를 깨운다(이미 돌고 있으면 무시).
                start_if_idle(&runner_bg, app_bg);
            });
            // 기다리지 않고 곧장 다음 아이템을 집는다 → 댓글 아이템들이 동시에 진행된다.
            continue;
        }

        // 종목토론방 전용 아이템(#240): 카페(9222 공유)·밴드(HTTP)와 자원이 겹치지 않고 계정마다
        // 전용 헤드리스 Chrome을 쓰므로, 여러 아이템을 동시에 돌려도 안전하다. 우선순위 픽(#231)이
        // 그대로라 종토방이 카페·밴드보다 먼저 집히고(대기 중 밴드·카페보다 앞서 실행), 여기서
        // 기다리지 않고 곧장 다음 아이템을 집어 종토방 아이템들이 동시에 돈다. 동시 수는
        // 사용자 설정 한도(#284, 0=무제한)로 제한한다.
        if is_forum_only_item(&job) {
            // 한도는 claim 시점에 동적으로 읽는다(#284) — 종토방·댓글 레인에 같은 사용자 한도를
            // 적용한다. 낮춰도 이미 돌고 있는 작업은 멈추지 않고 새 claim만 active < limit까지 대기.
            let limit = read_concurrency_limit(&app);
            let claimed = {
                let Ok(mut inner) = runner.inner.lock() else {
                    return;
                };
                if may_claim(inner.active_forum, limit) {
                    inner.active_forum += 1;
                    true
                } else {
                    false
                }
            };
            if !claimed {
                // 동시 한도 도달 — 슬롯이 빌 때까지 잠깐 대기 후 같은 아이템을 다시 집는다
                // (바쁜 루프 방지). 우선순위 픽이라 카페·밴드보다 여전히 종토방이 먼저다.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                continue;
            }
            now_store.mutate(|items| mark_running(items, &job.id));
            let app_bg = app.clone();
            let runner_bg = runner.clone();
            tauri::async_runtime::spawn(async move {
                finish_item(&app_bg, &job).await;
                if let Ok(mut inner) = runner_bg.inner.lock() {
                    inner.active_forum = inner.active_forum.saturating_sub(1);
                }
                // 남은 대기 아이템을 이어서 처리하도록 워커를 깨운다(이미 돌고 있으면 무시).
                start_if_idle(&runner_bg, app_bg);
            });
            // 기다리지 않고 곧장 다음 아이템을 집는다 → 종토방 아이템들이 동시에 진행된다.
            continue;
        }

        // 그 외(카페·밴드·로그인)는 자원 공유(9222·폰 IP) 때문에 기존과 동일하게 하나씩 순차
        // 처리한다 — 이 경로의 동작은 한 글자도 바꾸지 않는다.
        now_store.mutate(|items| mark_running(items, &job.id));
        finish_item(&app, &job).await;
    }
}

/// 한 아이템 실행을 끝내고 결과를 큐에 반영한다 — 완료면 큐에서 제거(취소와 동일 경로),
/// 우선순위 양보(#232)면 잔여 plan으로 Waiting 복귀. worker_loop의 순차 경로(카페·밴드·
/// 로그인)와 종토방 동시 경로가 함께 쓴다(#240). 동작은 기존 worker_loop 본문 그대로다.
async fn finish_item<R: Runtime>(app: &AppHandle<R>, job: &QueueNowItem) {
    match execute_item(app, job).await {
        ItemOutcome::Completed => {
            // 완료/일반 실패/도중 차단을 가리지 않고 큐에서 **제거한다**(사용자 지시: 게시큐엔
            // 돌아가는 작업만 보이고, 성공/실패 결과는 알림에서 확인). 결과(성공·실패·도중 차단
            // 종목)는 실행 중 set_progress_and_items가 알림 로그(log_batches)·계정 상태에 이미
            // 기록하므로, 큐에 결과 카드를 남기지 않아도 알림에서 그대로 확인된다. 워커는 Waiting
            // 만 집으므로 제거해도 재실행되지 않는다.
            app.state::<JsonStore<QueueNowItem>>()
                .mutate(|items| apply_cancel_now(items, &job.id));
        }
        ItemOutcome::Yielded(remaining) => {
            // 삭제가 아니라 중지(#232): 잔여 plan(아직 안 한 그룹만)으로 Waiting 복귀 후 재정렬.
            // 완료 그룹은 plan에서 빠져 재개 시 중복게시 0.
            app.state::<JsonStore<QueueNowItem>>()
                .mutate(|items| apply_yield_now(items, &job.id, *remaining));
        }
    }
}

/// now 큐의 "최대 작동가능 작업 수"(#284) 사용자 설정. 0 = 무제한(기본값). N = 동시에 돌리는
/// 작업(종토방·"특정 게시글" 댓글 레인 공통)을 N개로 제한한다. 단일 원소 컬렉션으로 디스크에
/// 영속화한다(`JsonStore<ConcurrencyConfig>`) — 다른 도메인 스토어처럼 타입으로 키잉되므로
/// 기존 스토어와 충돌하지 않는다.
/// 기본값(`Default`)은 `limit: 0` = 무제한 — u32의 기본값 0이 그대로 "사용자가 숫자를 넣기
/// 전까지 상한 없이 돈다"는 의미라 파생(derive)으로 충분하다.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct ConcurrencyConfig {
    /// 동시 작업 상한. 0이면 무제한. 워커는 claim 시점마다 이 값을 동적으로 읽으므로,
    /// 한도를 **낮춰도** 이미 돌고 있는 작업은 멈추지 않고 새 claim만 active < limit까지 기다린다.
    pub limit: u32,
}

/// now 큐 동시 작업 한도 스토어의 seed(#284). 단일 원소(무제한=0)로 시작한다.
pub fn seed_concurrency() -> Vec<ConcurrencyConfig> {
    vec![ConcurrencyConfig::default()]
}

/// 새 작업을 claim해도 되는지 판정한다(#284, 순수). `limit==0`이면 무제한이라 항상 허용,
/// 아니면 현재 동시 작업 수(`active`)가 `limit` 미만일 때만 허용한다. 워커가 claim 시점에
/// 호출하므로, 한도를 낮춰도 이미 돌고 있는 작업(=active에 이미 반영)은 멈추지 않고 새 claim만 막힌다.
fn may_claim(active: usize, limit: u32) -> bool {
    limit == 0 || active < limit as usize
}

/// 영속화된 now 큐 동시 작업 한도를 읽는다(#284). 워커가 매 claim마다 호출하므로, 사용자가
/// 저장한 새 한도가 곧바로 반영된다. 단일 원소 스토어의 첫 값(없으면 무제한 0)을 돌려준다.
fn read_concurrency_limit<R: Runtime>(app: &AppHandle<R>) -> u32 {
    app.state::<JsonStore<ConcurrencyConfig>>()
        .snapshot()
        .first()
        .map(|c| c.limit)
        .unwrap_or(0)
}

/// 이 아이템이 **종목토론방 전용**인지 — forum 타깃만 있고 카페(naver)·밴드 게시가 없으면 true.
/// 이런 아이템은 전용 헤드리스 Chrome으로 게시해 카페(9222)·밴드(HTTP)와 자원이 겹치지 않아,
/// 여러 아이템을 동시에 돌려도 안전하다(#240). 로그인 동봉 여부는 무관하다 — 게시 시 저장된
/// 쿠키를 쓰고 재로그인하지 않으므로(#234). 카페가 섞이면(9222 공유) false → 순차 경로로 간다.
fn is_forum_only_item(item: &QueueNowItem) -> bool {
    item.plan
        .as_ref()
        .is_some_and(|p| !p.forum.is_empty() && p.naver.is_empty() && p.band.is_empty())
}

/// 이 plan이 **"특정 게시글"(url) 댓글 전용**인지 — 댓글 모드이고, 실린 모든 대상이 url 댓글
/// 대상(카페=commentTarget.mode Url, 종토방=comment_url 채워짐, 밴드=commentTarget.mode Url)일
/// 때 true. 이런 아이템은 새 글을 쓰지 않고(카페·밴드=HTTP, 종토방=계정별 전용 Chrome) 게시
/// 시점에 저장 쿠키만 쓰므로(재로그인·IP 회전 없음, 아래 그룹 루프에서 스킵) 자원이 겹치지
/// 않아 여러 아이템을 동시에 돌려도 안전하다. 대상이 하나도 없으면 false.
fn is_url_comment_only_plan(plan: &PublishPlan) -> bool {
    if !matches!(plan.kind, ModeValue::Comment) {
        return false;
    }
    if plan.naver.is_empty()
        && plan.forum.is_empty()
        && plan.band.is_empty()
        && plan.blog.is_empty()
        && plan.clip.is_empty()
    {
        return false;
    }
    let is_url_spec = |t: &Option<CommentTargetSpec>| matches!(t, Some(s) if matches!(s.mode, CommentTarget::Url));
    let naver_ok = plan.naver.iter().all(|t| is_url_spec(&t.comment_target));
    let forum_ok = plan.forum.iter().all(|t| !t.comment_url.trim().is_empty());
    let band_ok = plan.band.iter().all(|t| is_url_spec(&t.comment_target));
    // 블로그(#271)·클립(#클립)은 항상 댓글 전용(HTTP, 저장 쿠키)이라 url 댓글로 본다 — 동시 레인에서
    // 돌고 게시 시점 재로그인·IP 회전을 생략한다(자원 비공유라 안전).
    naver_ok && forum_ok && band_ok
}

/// 아이템이 "특정 게시글" 댓글 전용인지(위 plan 술어를 아이템에 적용).
fn is_url_comment_only_item(item: &QueueNowItem) -> bool {
    item.plan.as_ref().is_some_and(is_url_comment_only_plan)
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
async fn execute_item<R: Runtime>(app: &AppHandle<R>, item: &QueueNowItem) -> ItemOutcome {
    let Some(plan) = item.plan.as_ref() else {
        return ItemOutcome::Completed;
    };
    let id = item.id.as_str();

    // 로그인 전용 아이템(#210): 게시 타깃(naver/forum/band)이 하나도 없고 login만 있으면
    // 게시 경로를 타지 않고 계정별 로그인만 수행하고 종료한다(하위호환). login이 게시와 함께
    // 있으면 아래 그룹 게시 경로가 계정별로 [회전→로그인→그 계정 게시]를 원자화한다(#10004).
    let no_publish_targets = plan.naver.is_empty()
        && plan.forum.is_empty()
        && plan.band.is_empty()
        && plan.blog.is_empty()
        && plan.clip.is_empty();
    if let Some(login) = plan.login.as_ref().filter(|l| !l.is_empty()) {
        if no_publish_targets {
            run_login_targets(app, id, login).await;
            return ItemOutcome::Completed;
        }
    }

    // 계정 단위 게시 그룹(#10004): 키=(account_id, family). 각 그룹은 [회전→로그인→IP검증→그
    // 계정 게시]를 원자적으로 수행해, 게시 시점 egress IP가 로그인 IP와 같도록 보장한다.
    let groups = group_accounts_for_publish(plan);

    // 진행률 분모는 기존 estimate_total과 동일한 합(글+댓글추정+종토방+밴드). 모든 그룹의
    // 작업을 누적 버킷에 모아 build_items/build_log_batch가 기존과 같은 단일 plan 기준으로
    // 라이브/완료 로그를 만든다(호환).
    let total = estimate_total(plan);
    let mut done = 0u32;
    update_progress(app, id, done, total);

    // 모든 그룹의 결과를 누적하는 버킷. 라이브 표시·완료 로그가 이 누적분을 본다.
    let mut all_posts: Vec<JobReport> = Vec::new();
    let mut all_comments: Vec<CommentJobReport> = Vec::new();
    let mut all_fetch_failures: Vec<CommentFetchFailure> = Vec::new();
    let mut all_forum: Vec<ForumOutcome> = Vec::new();
    let mut all_band: Vec<BandOutcome> = Vec::new();
    let mut all_blog: Vec<BlogOutcome> = Vec::new();
    let mut all_clip: Vec<ClipOutcome> = Vec::new();

    let cafe_link = crate::template_tokens::resolve_link(&plan.link_override, "");
    let cafe_comments: Vec<String> = plan
        .comments
        .iter()
        .map(|c| crate::template_tokens::resolve_cafe_band(c, &cafe_link))
        .collect();

    for (gi, group) in groups.iter().enumerate() {
        // 협조적 취소: 그룹 시작 전 큐에서 빠졌으면(취소) 남은 그룹은 게시하지 않는다.
        if !item_present(app, id) {
            break;
        }
        // 우선순위 선점(#232): 더 높은 우선순위(로그인/종토방)가 대기하면, 이 계정 그룹을
        // **시작하기 전** 안전지점에서 양보한다. 그룹은 [회전→로그인→IP검증→게시]가 원자적이라
        // 이 경계가 "여기까지만 하고 멈춰도 무방"한 지점이다. 이미 끝난 그룹(groups[..gi])은
        // 완료 로그로 남기고, 남은 그룹(groups[gi..])만 잔여 plan으로 되돌려 재개 시 중복게시 0.
        if should_yield(app, id) {
            let (rem_naver, rem_band) = account_sets(&groups[gi..]);
            let (done_naver, done_band) = account_sets(&groups[..gi]);
            // [가시성] 양보로 뒤로 밀린 계정을 한 줄로 남긴다 — "증발"처럼 보이지 않게(사용자
            // 지적 2026-06-30). 이 계정들은 잔여 plan으로 Waiting 복귀해 제 차례에 다시 게시된다.
            let bumped: Vec<String> = rem_naver
                .iter()
                .chain(rem_band.iter())
                .map(|a| crate::auth::mask_id(a))
                .collect();
            tracing::info!(
                "[POST] 더 높은 우선순위 작업에 양보 — 밀린 계정 {}건은 잔여 plan으로 재대기: {}",
                bumped.len(),
                bumped.join(", ")
            );
            let completed = retain_plan_accounts(plan, &done_naver, &done_band);
            flush_completion_log(
                app,
                &completed,
                &all_posts,
                &all_comments,
                &all_forum,
                &all_band,
                &all_blog,
                &all_clip,
                &all_fetch_failures,
            );
            return ItemOutcome::Yielded(Box::new(retain_plan_accounts(
                plan, &rem_naver, &rem_band,
            )));
        }
        let acc = group.account_id.as_str();

        // (a)(b) 회전+로그인+IP검증. 단 **종목토론방 전용 그룹**은 큐 실행 시 재로그인하지 않고
        // 선택 로그인 때 저장된 쿠키(cookies/{loginId}.json)를 그대로 쓴다(사수 지침) — forum
        // 게시는 매번 그 쿠키 파일을 디스크에서 새로 읽으므로 1차 로그인분으로 충분하고, 2차
        // 로그인은 같은 파일을 덮어쓸 뿐 불필요하다. 카페가 섞인 그룹은 10004(IP check failure)
        // 때문에 기존대로 회전+로그인+IP검증을 유지한다. 실패하면 그룹 타깃 전부 합성 실패로 남긴다.
        // "특정 게시글" 댓글 전용 아이템도 종토방처럼 게시 시점 재로그인을 생략하고 저장
        // 쿠키(cookies/{loginId}.json)를 그대로 쓴다 — 이 아이템들은 병렬 레인에서 동시에
        // 도는데, 재로그인은 폰 IP 회전(airplane 토글)을 공유해 동시 실행 시 충돌하기 때문이다.
        // 카페·밴드 댓글은 HTTP라 선택 로그인 때 저장한 쿠키로 충분하다(종토방과 동일 원칙).
        let prep = if forum_only_group(plan, group) || is_url_comment_only_plan(plan) {
            Ok(())
        } else {
            prepare_group_login(app, group).await
        };
        if let Err(skip) = prep {
            match group.family {
                AccountFamily::Naver => {
                    if runs_post(plan) {
                        all_posts.extend(synth_post_failures(plan, acc, &skip));
                    }
                    // comment 전용 카페 댓글 대상도 조용히 누락하지 않고 Fail로 남긴다.
                    all_fetch_failures.extend(synth_comment_failures(plan, acc, &skip));
                    all_forum.extend(synth_forum_failures(plan, acc, &skip));
                    // 블로그 댓글 대상(#271)도 조용히 누락하지 않고 Fail로 남긴다.
                    all_blog.extend(synth_blog_failures(plan, acc, &skip));
                    // 클립 댓글 대상(#클립)도 조용히 누락하지 않고 Fail로 남긴다.
                    all_clip.extend(synth_clip_failures(plan, acc, &skip));
                }
                AccountFamily::Band => all_band.extend(synth_band_failures(plan, acc, &skip)),
            }
            done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
            set_progress_and_items(
                app,
                id,
                done,
                total,
                build_items(
                    plan,
                    &all_posts,
                    &all_comments,
                    &all_fetch_failures,
                    &all_forum,
                    &all_band,
                    &all_blog,
                    &all_clip,
                ),
            );
            continue;
        }

        // (c) 그 계정 범위로 게시.
        match group.family {
            AccountFamily::Naver => {
                // 1. 카페 글(post/both) — 이 계정 대상만.
                if runs_post(plan) && item_present(app, id) {
                    let jobs: Vec<PostJob> = plan_to_post_jobs(plan)
                        .into_iter()
                        .filter(|j| j.account_id == acc)
                        .collect();
                    if !jobs.is_empty() {
                        set_running_phase(
                            app,
                            id,
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                            running_post_items_for(plan, acc),
                        );
                        let base_done = done;
                        let reports = run_post_jobs_with_progress(&jobs, |c| {
                            update_progress(app, id, base_done + c as u32, total);
                        })
                        .await;
                        all_posts.extend(reports);
                        done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
                        set_progress_and_items(
                            app,
                            id,
                            done,
                            total,
                            build_items(
                                plan,
                                &all_posts,
                                &all_comments,
                                &all_fetch_failures,
                                &all_forum,
                                &all_band,
                                &all_blog,
                                &all_clip,
                            ),
                        );
                    }
                }

                // 2. 카페 댓글(comment/both) — 이 계정 대상만.
                if runs_comment(plan) && item_present(app, id) {
                    let collected = collect_comment_targets(plan, &all_posts, Some(acc)).await;
                    all_fetch_failures.extend(collected.fetch_failures);
                    let comment_jobs = if matches!(plan.kind, ModeValue::Both) {
                        build_self_comment_jobs(collected.targets, &cafe_comments)
                    } else {
                        build_comment_jobs(collected.targets, &cafe_comments)
                    };
                    if !comment_jobs.is_empty() {
                        // 댓글 1건 = 1행으로, 밴드/종토방(#219)과 동일한 라이브 단계 표시
                        // (게시 전 → 게시 중… → 게시 완료/실패)를 댓글에도 적용한다(#252).
                        let base_items = build_items(
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                        );
                        let mut live: Vec<BatchItem> = comment_jobs
                            .iter()
                            .map(|j| comment_skeleton(plan, j, BatchItemStatus::Waiting))
                            .collect();
                        let base_done = done;
                        let mut completed = 0u32;
                        // 시작 전 "게시 전" 스켈레톤을 먼저 깔아 진행 전 단계가 보이게 한다.
                        write_live_items(app, id, &base_items, &live, base_done, total);
                        let reports = run_comment_jobs_with_events(&comment_jobs, |ev| match ev {
                            CommentEvent::Started(i) => {
                                live[i] = comment_skeleton(
                                    plan,
                                    &comment_jobs[i],
                                    BatchItemStatus::Running,
                                );
                                write_live_items(
                                    app,
                                    id,
                                    &base_items,
                                    &live,
                                    base_done + completed,
                                    total,
                                );
                            }
                            CommentEvent::Finished(i, report) => {
                                live[i] = comment_report_to_item(plan, report);
                                completed += 1;
                                write_live_items(
                                    app,
                                    id,
                                    &base_items,
                                    &live,
                                    base_done + completed,
                                    total,
                                );
                            }
                        })
                        .await;
                        all_comments.extend(reports);
                    }
                    done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
                    set_progress_and_items(
                        app,
                        id,
                        done,
                        total,
                        build_items(
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                        ),
                    );
                }

                // 3. 네이버 블로그 댓글(#271) — 이 계정 대상만. 블로그는 댓글 전용이라 카페와 같은
                // 네이버 저장 쿠키를 재사용한다(별도 로그인 없음). 댓글 본문은 cafe와 동일하게
                // plan.comments(토큰 치환 후)에서 만든다.
                if !plan.blog.is_empty() && item_present(app, id) {
                    let base = build_items(
                        plan,
                        &all_posts,
                        &all_comments,
                        &all_fetch_failures,
                        &all_forum,
                        &all_band,
                        &all_blog,
                        &all_clip,
                    );
                    let outcomes =
                        run_blog_targets(app, plan, id, base, done, total, Some(acc)).await;
                    all_blog.extend(outcomes);
                    done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
                    set_progress_and_items(
                        app,
                        id,
                        done,
                        total,
                        build_items(
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                        ),
                    );
                }
                // 4. 네이버 클립 댓글(#클립) — 이 계정 대상만. 블로그처럼 네이버 저장 쿠키를 재사용
                // 하되, 게시 직전 계정마다 클립 프로필 생성을 보장한다(run_clip_targets 내부).
                if !plan.clip.is_empty() && item_present(app, id) {
                    let base = build_items(
                        plan,
                        &all_posts,
                        &all_comments,
                        &all_fetch_failures,
                        &all_forum,
                        &all_band,
                        &all_blog,
                        &all_clip,
                    );
                    let outcomes =
                        run_clip_targets(app, plan, id, base, done, total, Some(acc)).await;
                    all_clip.extend(outcomes);
                    done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
                    set_progress_and_items(
                        app,
                        id,
                        done,
                        total,
                        build_items(
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                        ),
                    );
                }
                // 종목토론방(forum)은 이 그룹 루프에서 처리하지 않는다 — 루프 종료 후 전 계정을
                // 한 번에 병렬 게시한다(#237). 카페(9222)·밴드(HTTP)와 자원이 안 겹쳐 병렬 안전.
            }
            AccountFamily::Band => {
                // 4. 밴드 게시 — 이 계정 대상만(account_filter).
                if !plan.band.is_empty() && item_present(app, id) {
                    let base = build_items(
                        plan,
                        &all_posts,
                        &all_comments,
                        &all_fetch_failures,
                        &all_forum,
                        &all_band,
                        &all_blog,
                        &all_clip,
                    );
                    let outcomes =
                        run_band_targets(app, plan, id, base, done, total, Some(acc)).await;
                    all_band.extend(outcomes);
                    done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
                    set_progress_and_items(
                        app,
                        id,
                        done,
                        total,
                        build_items(
                            plan,
                            &all_posts,
                            &all_comments,
                            &all_fetch_failures,
                            &all_forum,
                            &all_band,
                            &all_blog,
                            &all_clip,
                        ),
                    );
                }
            }
        }
    }

    // 종목토론방: 모든 계정을 한 번에 — run_forum_targets가 계정별로 전용 Chrome을 띄워
    // 동시에 게시한다(#237). 카페/밴드(위 계정 그룹 루프)와 자원이 겹치지 않아, 카페/밴드가
    // 도는 것과 무관하게 종토방만 병렬로 흐른다. account_filter=None으로 전 계정을 한 번에 넘긴다.
    if !plan.forum.is_empty() && item_present(app, id) {
        let base = build_items(
            plan,
            &all_posts,
            &all_comments,
            &all_fetch_failures,
            &all_forum,
            &all_band,
            &all_blog,
            &all_clip,
        );
        let outcomes = run_forum_targets(app, plan, id, base, done, total, None).await;
        all_forum.extend(outcomes);
        done = resolved_count(&all_posts, &all_comments, &all_forum, &all_band, &all_blog, &all_clip);
        set_progress_and_items(
            app,
            id,
            done,
            total,
            build_items(
                plan,
                &all_posts,
                &all_comments,
                &all_fetch_failures,
                &all_forum,
                &all_band,
                &all_blog,
                &all_clip,
            ),
        );
    }

    // 종목토론방(forum) 글 게시에 성공한 계정만 "대기"(노란색)로 전환한다(#267-3, 사수 요청: 카페·
    // 밴드는 제외 — forum만). 같은 계정으로 연속 게시되지 않게 게시 선택 목록에서 숨기기 위함.
    // 댓글만 성공한 경우는 제외하려고 글을 포함한 플랜(runs_post)일 때만 적용한다. 사용자가 계정
    // 화면에서 상태 배지를 누르면 다시 Active로 돌아간다.
    if runs_post(plan) {
        apply_waiting_for_successful_posts(app, &all_forum);
    }

    // 완료 로그(LogBatch)/activity: 누적된 카페 글·댓글·토론방·밴드 결과 + 댓글 조회 실패를
    // 알림에 남긴다. 실행한 작업이 하나도 없으면(빈 plan) 빈 배치는 만들지 않는다.
    flush_completion_log(
        app,
        plan,
        &all_posts,
        &all_comments,
        &all_forum,
        &all_band,
        &all_blog,
        &all_clip,
        &all_fetch_failures,
    );

    // 종목토론방(#양보누락): 선점 양보·취소 경계로 **게시를 시도조차 못 한** 계정(outcome 0건)은
    // 실패가 아니라 "아직 차례가 안 온" 것이므로 큐에서 빼지(완료하지) 않고 그 계정들의 잔여 종토
    // plan으로 재대기시킨다 — 한 건이라도 올릴 때까지 대기(사용자 요청 2026-06-30). 시도해서
    // 실패·대기초과·차단·건너뜀한 계정은 outcome가 있어 여기서 빠지므로(=완료) 무한 재대기는 없다.
    // 위 flush_completion_log/apply_waiting_for_successful_posts는 *시도된* 계정만 보고하므로
    // 미시도 계정은 로그·상태에 남지 않아(=활성 유지) 중복 보고가 없다. id가 이미 큐에서 빠졌으면
    // (사용자 취소) apply_yield_now가 no-op이라 되살아나지 않는다.
    let unattempted = forum_unattempted_accounts(plan, &all_forum);
    if !unattempted.is_empty() {
        let who: Vec<String> = unattempted.iter().map(|a| crate::auth::mask_id(a)).collect();
        tracing::info!(
            "[POST] 종목토론방 미시도 계정 {}건 — 큐에서 빼지 않고 차례 올 때까지 재대기: {}",
            unattempted.len(),
            who.join(", ")
        );
        return ItemOutcome::Yielded(Box::new(retain_forum_only(plan, &unattempted)));
    }
    ItemOutcome::Completed
}

/// 종목토론방(forum) 게시 결과를 보고 계정 상태를 갱신한다(#267-3 + 후속 #2, forum만 — 카페·
/// 밴드 제외). 두 갈래로 나뉜다:
/// - **게시 도중 차단**(`is_blocking_failure`)을 만난 계정은 `Blocked`로 둔다. 부분 성공이 있어도
///   차단이 우선이다 — 다시 써도 또 차단되므로 "대기"로 두면 안 된다(사용자 지시: 도중 차단된
///   계정은 전부 차단 상태로).
/// - 차단되지 않았고 글 게시에 **성공**한 계정만 `Waiting`(대기)으로 둔다(#267-3).
/// 호출부는 글을 포함한 플랜(runs_post)일 때만 부른다 — 댓글만 성공한 경우는 대기로 바꾸지 않는다.
/// account_id가 곧 loginId(쿠키 키)라 `apply_status_by_login_id`로 같은 loginId 모든 행을 함께 갱신.
fn apply_waiting_for_successful_posts<R: Runtime>(app: &AppHandle<R>, forum: &[ForumOutcome]) {
    use crate::ipc::accounts::{apply_status_by_login_id, Account};
    let blocked = blocked_post_login_ids(forum);
    // 대기초과(페이지 대기시간 초과·HTTP 500, #7)도 모은다. 차단보다 약한 종료성 실패라 차단
    // 계정은 뺀다(차단 우선) — 한 계정이 차단과 타임아웃을 모두 만나면 차단으로 본다.
    let timed_out: std::collections::BTreeSet<String> = timed_out_post_login_ids(forum)
        .into_iter()
        .filter(|id| !blocked.contains(id))
        .collect();
    // 차단도 대기초과도 아닌 "그 밖의 실패"(약관 동의하기 비활성·버튼 못찾음, 응답 읽기 IO 실패
    // 등)는 전부 `Error`로 칠한다(사용자 지시: 대기초과·보류·활성 기준이 아니면 전부 에러 —
    // 오류가 떠도 계정이 활성으로 남지 않게). 차단·대기초과는 각자 전용 상태가 우선이라 뺀다.
    let errored: std::collections::BTreeSet<String> = errored_post_login_ids(forum)
        .into_iter()
        .filter(|id| !blocked.contains(id) && !timed_out.contains(id))
        .collect();
    // 대기 후보(성공)에서 차단·대기초과·에러 계정은 뺀다 — 종료성/일시/그밖의 실패가 대기보다
    // 우선한다(#2/#7 + 후속). 같은 계정에 성공과 실패가 섞이면 실패를 표면화한다(기존 #7과 동일 철학).
    let waiting: Vec<String> = successful_post_login_ids(forum)
        .into_iter()
        .filter(|id| !blocked.contains(id) && !timed_out.contains(id) && !errored.contains(id))
        .collect();
    if blocked.is_empty() && timed_out.is_empty() && errored.is_empty() && waiting.is_empty() {
        return;
    }
    app.state::<JsonStore<Account>>().mutate(|list| {
        // 우선순위로 칠한다: 차단(종료) → 대기초과(일시 실패) → 에러(그밖의 실패) → 대기(성공).
        let list = blocked.iter().fold(list, |acc, id| {
            apply_status_by_login_id(
                acc,
                id,
                AccountStatus::Blocked,
                Some(
                    "글 게시 도중 차단되어 큐가 멈췄습니다. 계정이 차단 상태로 전환되었어요."
                        .to_owned(),
                ),
            )
        });
        let list = timed_out.iter().fold(list, |acc, id| {
            apply_status_by_login_id(
                acc,
                id,
                AccountStatus::TimedOut,
                Some(
                    "페이지 대기시간 초과 또는 네이버 서버 오류(HTTP 500)로 게시가 실패했습니다. 잠시 후 다시 시도하세요."
                        .to_owned(),
                ),
            )
        });
        let list = errored.iter().fold(list, |acc, id| {
            apply_status_by_login_id(
                acc,
                id,
                AccountStatus::Error,
                Some(
                    "글 게시에 실패해 '에러' 상태로 전환했습니다(약관 동의·세션 등). 자세한 원인은 완료 로그의 '자세히 보기'에서 확인한 뒤, 상태를 눌러 다시 시도하세요."
                        .to_owned(),
                ),
            )
        });
        waiting.iter().fold(list, |acc, id| {
            apply_status_by_login_id(
                acc,
                id,
                AccountStatus::Waiting,
                Some(
                    "글 게시 완료 — 대기 상태입니다. 상태를 눌러 다시 활성으로 바꿀 수 있어요."
                        .to_owned(),
                ),
            )
        })
    });
}

/// 게시 **도중 차단**(`is_blocking_failure`)을 만난 계정(loginId) 집합(#2, 순수). 부분 성공
/// 여부와 무관하게, 차단성 실패가 하나라도 있으면 그 계정은 차단으로 본다. skip(앞 글 차단으로
/// 건너뛴 글)은 그 자체가 차단 사유가 아니므로 제외하고, 429(일시적 과다요청)도 차단으로 치지
/// 않는다(`is_blocking_failure`가 429를 제외). 카페·밴드는 대상이 아니다(forum 결과만 본다).
fn blocked_post_login_ids(forum: &[ForumOutcome]) -> std::collections::BTreeSet<String> {
    use crate::discussion_batch::is_blocking_failure;
    let mut ids = std::collections::BTreeSet::new();
    for o in forum
        .iter()
        .filter(|o| !o.result.ok && !o.result.skipped && is_blocking_failure(&o.result.message))
    {
        ids.insert(o.account_id.clone());
    }
    ids
}

/// 게시 **대기초과**(`is_timed_out_failure`: 페이지 대기시간 초과·HTTP 500 네이버 서버 오류)를
/// 만난 계정(loginId) 집합(#7, 순수). 차단(`blocked_post_login_ids`)과 같은 구조지만, 일시적
/// 서버/타이밍 실패라 별도 `TimedOut` 상태로 둬 게시 목록에서만 숨기고(대기와 동일) 재시도할 수
/// 있게 한다. skip(앞 글 차단으로 건너뜀)은 그 자체가 대기초과 사유가 아니므로 제외한다. 카페·
/// 밴드는 대상이 아니다(forum 결과만 본다). 호출부에서 차단이 대기초과보다 우선한다.
fn timed_out_post_login_ids(forum: &[ForumOutcome]) -> std::collections::BTreeSet<String> {
    use crate::discussion_batch::is_timed_out_failure;
    let mut ids = std::collections::BTreeSet::new();
    for o in forum
        .iter()
        .filter(|o| !o.result.ok && !o.result.skipped && is_timed_out_failure(&o.result.message))
    {
        ids.insert(o.account_id.clone());
    }
    ids
}

/// 종목토론방(forum) 글 게시에 성공한 계정(loginId) 집합(#267-3, 순수). forum 글(ok && !skip)의
/// 성공만 모은다 — 카페·밴드 글은 대기 대상이 아니다(사수 요청). 호출부의 runs_post 게이트가
/// 댓글 전용 플랜을 걸러, 여기 들어온 forum 성공은 글(또는 글+댓글) 성공이다.
fn successful_post_login_ids(forum: &[ForumOutcome]) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    for o in forum.iter().filter(|o| o.result.ok && !o.result.skipped) {
        ids.insert(o.account_id.clone());
    }
    ids
}

/// 종목토론방(forum) 게시가 **차단도 대기초과도 아닌 "그 밖의 실패"**로 끝난 계정(loginId)
/// 집합(순수). 약관 동의하기 비활성/버튼 못찾음, 약관 동의 처리 실패, CDP 응답 읽기 IO 실패처럼
/// `is_blocking_failure`·`is_timed_out_failure` 어느 마커에도 안 걸리는 실패가 대상이다. 사용자
/// 지시(후속): 대기초과·보류·활성(성공) 기준이 아닌 실패는 전부 `Error`로 칠해 계정이 활성으로
/// 남지 않게 한다. skip(앞 글 차단으로 건너뜀)은 그 자체가 실패 사유가 아니므로 제외한다. 차단·
/// 대기초과는 전용 상태가 우선이므로 호출부에서 그 계정을 뺀다(차단 > 대기초과 > 에러 > 대기).
fn errored_post_login_ids(forum: &[ForumOutcome]) -> std::collections::BTreeSet<String> {
    use crate::discussion_batch::{is_blocking_failure, is_timed_out_failure};
    let mut ids = std::collections::BTreeSet::new();
    for o in forum.iter().filter(|o| {
        !o.result.ok
            && !o.result.skipped
            && !is_blocking_failure(&o.result.message)
            && !is_timed_out_failure(&o.result.message)
    }) {
        ids.insert(o.account_id.clone());
    }
    ids
}

/// 누적 결과로 완료 로그(LogBatch)/activity를 남긴다. 정상 종료 시 **전체 plan**으로, 우선순위
/// 양보(#232) 시 **완료분 plan**으로 호출한다 — `build_log_batch`가 plan 스켈레톤으로 항목을
/// 만들므로 결과와 일치하는 plan을 넘겨야 미게시 대상이 유령 항목으로 새지 않는다. 실행한 작업이
/// 하나도 없으면(빈 결과) 빈 배치는 만들지 않는다.
#[allow(clippy::too_many_arguments)]
fn flush_completion_log<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    posts: &[JobReport],
    comments: &[CommentJobReport],
    forum: &[ForumOutcome],
    band: &[BandOutcome],
    blog: &[BlogOutcome],
    clip: &[ClipOutcome],
    fetch_failures: &[CommentFetchFailure],
) {
    let batch = build_log_batch(
        plan,
        posts,
        comments,
        forum,
        band,
        blog,
        clip,
        fetch_failures,
        now_ms(),
        LB_SEQ.fetch_add(1, Ordering::Relaxed),
    );
    if !batch.items.is_empty() {
        record_completion(app, batch);
    }
}

/// 완료(성공/실패로 확정)된 작업 수 — 진행률 done에 쓴다. "처리 중"은 세지 않는다.
fn resolved_count(
    posts: &[JobReport],
    comments: &[CommentJobReport],
    forum: &[ForumOutcome],
    band: &[BandOutcome],
    blog: &[BlogOutcome],
    clip: &[ClipOutcome],
) -> u32 {
    (posts.len() + comments.len() + forum.len() + band.len() + blog.len() + clip.len()) as u32
}

/// 한 계정의 카페 글 대상만 "처리 중" BatchItem으로 만든다(계정 그룹 게시 라이브 표시).
fn running_post_items_for(plan: &PublishPlan, account_id: &str) -> Vec<BatchItem> {
    running_post_items(plan)
        .into_iter()
        .zip(plan.naver.iter())
        .filter(|(_, t)| t.account_id == account_id)
        .map(|(item, _)| item)
        .collect()
}

/// 누적 결과(base) + 이 그룹의 "처리 중" 항목을 이어 붙여 라이브 큐 상태를 갱신한다.
#[allow(clippy::too_many_arguments)]
fn set_running_phase<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    plan: &PublishPlan,
    posts: &[JobReport],
    comments: &[CommentJobReport],
    fetch_failures: &[CommentFetchFailure],
    forum: &[ForumOutcome],
    band: &[BandOutcome],
    blog: &[BlogOutcome],
    clip: &[ClipOutcome],
    running: Vec<BatchItem>,
) {
    let mut items = build_items(plan, posts, comments, fetch_failures, forum, band, blog, clip);
    items.extend(running);
    set_queue_items(app, id, items);
}

/// 댓글 대상을 모은다. both 모드는 방금 게시에 성공한 글(self)에, comment 전용은
/// 각 naver 대상의 `commentTarget`(url 직접 / latest·popular 글목록 조회)에 단다.
/// `account_filter`가 Some(acc)면 그 계정 대상만 모은다(계정 그룹 게시, #10004). None이면
/// 전체(기존 동작). both는 `post_reports`가 이미 해당 그룹 글이므로 추가 필터가 무해하다.
async fn collect_comment_targets(
    plan: &PublishPlan,
    post_reports: &[JobReport],
    account_filter: Option<&str>,
) -> CommentCollect {
    let mut out = CommentCollect::default();

    if matches!(plan.kind, ModeValue::Both) {
        // self-comment: 방금 게시에 성공한 글에 단다(즉시게시 both 동작과 동일). 게시에
        // 실패한 글은 여기 없다 — 그 실패는 post_reports(글 게시 실패)로 이미 로그에 남으므로
        // "글이 없어 댓글도 못 달았다"는 별도 항목은 만들지 않는다.
        for report in post_reports {
            if account_filter.is_some_and(|acc| report.account_id != acc) {
                continue;
            }
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
        if account_filter.is_some_and(|acc| t.account_id != acc) {
            continue;
        }
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
                let Some(cafe_id) = spec.cafe_id else {
                    continue;
                };
                let count = spec.count.unwrap_or(1).max(1) as usize;
                let cafe_str = cafe_id.to_string();
                // 실행 시점에 상위 N개를 다시 조회한다(예약과 실행 사이 새 글 반영).
                // 최신글은 페이지당 15개라 N이 많으면 다음 페이지를 이어 조회하고,
                // 인기글(주간 단일 API)은 페이징이 없어 상위 N개만 취한다.
                let fetched = match spec.mode {
                    CommentTarget::Latest => {
                        fetch_latest_articles_for_account_up_to(&cafe_str, &t.account_id, count)
                            .await
                    }
                    _ => {
                        fetch_article_list_for_account(&cafe_str, SortBy::Popular, 1, &t.account_id)
                            .await
                            .map(|resp| resp.articles.into_iter().take(count).collect())
                    }
                };
                match fetched {
                    Ok(articles) => {
                        for article in articles {
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

    let run_post = runs_post(plan);
    let run_comment = runs_comment(plan);
    let comment = plan.comments.first().cloned().unwrap_or_default();

    // "특정 게시글" 댓글(comment_url 지정) 대상은 종목별 랜덤 글이 아니라 그 글 하나에만
    // 댓글을 단다. 계정 묶음 없이 대상 1건 = 요청 1건으로 만들고(글 1개 단위 댓글), 강제로
    // 댓글 전용(run_post=false)으로 둔다. comment_url 없는 일반 대상은 기존처럼 계정별로
    // 종목을 묶어 한 요청에 싣는다(per-종목 동작 무변경).
    let mut url_reqs: Vec<ForumPublishRequest> = Vec::new();
    let mut by_account: BTreeMap<String, Vec<DiscussionStock>> = BTreeMap::new();
    for f in &plan.forum {
        let url = f.comment_url.trim();
        let stock = DiscussionStock {
            name: f.name.clone(),
            code: f.code.clone(),
            link: String::new(),
        };
        if url.is_empty() {
            by_account
                .entry(f.account_id.clone())
                .or_default()
                .push(stock);
        } else {
            url_reqs.push(ForumPublishRequest {
                host: "127.0.0.1".to_owned(),
                port: 0,
                account_id: f.account_id.clone(),
                run_post: false,
                run_comment: true,
                title: plan.title.clone(),
                body: plan.body_text.clone(),
                comment: comment.clone(),
                stocks: vec![stock],
                link_override: plan.link_override.clone(),
                comment_url: Some(url.to_owned()),
            });
        }
    }

    let regular = by_account
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
            link_override: plan.link_override.clone(),
            comment_url: None,
        });

    url_reqs.into_iter().chain(regular).collect()
}

/// 게시 그룹의 "계정 패밀리". 같은 패밀리는 같은 로그인 쿠키를 공유한다 — 네이버 카페와
/// 종목토론방은 같은 네이버 쿠키(Naver), 밴드는 별도 쿠키(Band)다. (instagram/threads 등
/// 미게시 플랫폼은 현재 게시 타깃이 없어 그룹화 대상이 아니다.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AccountFamily {
    Naver,
    Band,
}

/// LoginTarget의 플랫폼을 게시 패밀리로 접는다. Band만 Band, 그 외(naver/forum 등)는 Naver.
/// 로그인 처리 분기(`process_account` vs `process_band_account`)와 동일한 기준이다.
fn family_of(platform: &PlatformId) -> AccountFamily {
    match platform {
        PlatformId::Band => AccountFamily::Band,
        _ => AccountFamily::Naver,
    }
}

/// 한 계정(=account_id)+패밀리 단위의 원자적 게시 그룹. [회전→로그인→그 계정 게시]를
/// 한 단위로 묶어, 게시 시점의 egress IP가 로그인 IP와 같도록 보장한다(#10004 IP check
/// failure 예방). 타깃은 account_id로 다시 필터해 꺼내므로(인덱스 대신 키 보관) 직렬화
/// 스키마를 건드리지 않는다.
struct PublishGroup {
    account_id: String,
    family: AccountFamily,
    /// 이 그룹의 로그인 대상(있으면 회전+로그인 후 게시, 없으면 저장 쿠키로 바로 게시).
    login: Option<LoginTarget>,
}

/// plan을 계정 단위 게시 그룹으로 묶는다. 키=(account_id, family). 같은 계정의 카페·종토방은
/// 한 Naver 그룹으로, 밴드는 별도 Band 그룹으로 모인다. plan.login 순서를 우선해 그룹 순서를
/// 결정적으로 만들고(같은 키의 LoginTarget을 그 그룹에 붙인다), login에 없는 게시 타깃만
/// 있는 계정은 login=None 그룹으로 뒤에 잇는다. 게시 타깃이 하나도 없는 키(로그인만 있는
/// 계정)는 게시 그룹을 만들지 않는다 — 로그인 전용 아이템은 별도 경로(run_login_targets)다.
fn group_accounts_for_publish(plan: &PublishPlan) -> Vec<PublishGroup> {
    // 게시 타깃이 존재하는 (account_id, family) 키 집합.
    let mut has_naver: std::collections::BTreeSet<String> = Default::default();
    let mut has_band: std::collections::BTreeSet<String> = Default::default();
    for t in &plan.naver {
        has_naver.insert(t.account_id.clone());
    }
    for f in &plan.forum {
        has_naver.insert(f.account_id.clone());
    }
    // 블로그(#271)는 카페·종토방과 같은 Naver 패밀리(같은 네이버 쿠키)라 Naver 그룹으로 묶는다.
    for bl in &plan.blog {
        has_naver.insert(bl.account_id.clone());
    }
    // 클립(#클립)도 같은 네이버 쿠키 재사용이라 Naver 패밀리로 묶는다.
    for c in &plan.clip {
        has_naver.insert(c.account_id.clone());
    }
    for b in &plan.band {
        has_band.insert(b.account_id.clone());
    }

    let mut out: Vec<PublishGroup> = Vec::new();
    let mut seen: std::collections::HashSet<(String, AccountFamily)> = Default::default();

    // 1. login 순서 우선: 게시 타깃이 있는 로그인 대상만 그룹으로 만든다(결정성).
    if let Some(login) = plan.login.as_ref() {
        for t in login {
            let family = family_of(&t.platform);
            let present = match family {
                AccountFamily::Naver => has_naver.contains(&t.account_id),
                AccountFamily::Band => has_band.contains(&t.account_id),
            };
            if !present {
                continue;
            }
            let key = (t.account_id.clone(), family);
            if seen.insert(key) {
                out.push(PublishGroup {
                    account_id: t.account_id.clone(),
                    family,
                    login: Some(t.clone()),
                });
            }
        }
    }

    // 2. 로그인에 없는 게시 타깃 계정은 login=None 그룹으로 뒤에 잇는다(저장 쿠키로 바로 게시).
    for account_id in &has_naver {
        let key = (account_id.clone(), AccountFamily::Naver);
        if seen.insert(key) {
            out.push(PublishGroup {
                account_id: account_id.clone(),
                family: AccountFamily::Naver,
                login: None,
            });
        }
    }
    for account_id in &has_band {
        let key = (account_id.clone(), AccountFamily::Band);
        if seen.insert(key) {
            out.push(PublishGroup {
                account_id: account_id.clone(),
                family: AccountFamily::Band,
                login: None,
            });
        }
    }

    out
}

/// `execute_item`의 결과(#232). 워커가 아이템을 큐에서 **제거**(완료)할지, 잔여 plan으로
/// **Waiting 복귀**(중지/양보)할지 결정한다.
enum ItemOutcome {
    /// 아이템 전체를 끝까지 처리했다 → finish_item이 `apply_cancel_now`로 큐에서 제거한다
    /// (사용자 지시: 게시큐엔 돌아가는 작업만, 성공/실패 결과는 알림에서 확인). 성공·일반 실패·
    /// 도중 차단을 가리지 않으며, 결과는 실행 중 알림 로그·계정 상태에 이미 기록돼 있다.
    Completed,
    /// 더 높은 우선순위 작업(로그인/종토방)에 자리를 내주려 **안전지점(계정 그룹 경계)에서**
    /// 멈췄다 → 아직 게시하지 않은 그룹만 담은 잔여 plan으로 Waiting 복귀. 완료 그룹은 plan에서
    /// 빠져 재개 시 중복게시가 0이다. (drop 비용 큰 plan은 박싱 — 양보는 드물어 무해.)
    Yielded(Box<PublishPlan>),
}

/// 실행 중 아이템이 더 높은 우선순위 작업에 자리를 내줘야 하는지 판정한다(#232 순수 로직).
/// 현재 실행 중(`running_id`) 아이템의 우선순위보다 **엄격히 높은(값이 작은)** Waiting 아이템이
/// 하나라도 있으면 true.
/// - 로그인(0)은 어떤 작업(종토 1·카페/밴드 2)도 선점한다.
/// - 종토방(1)은 카페/밴드(2)를 선점하지만 다른 종토방(1)은 동순위라 선점하지 않는다(계속 진행).
/// - 동순위는 선점하지 않는다 = FIFO 유지. `running_id`가 큐에 없으면(이미 제거) false.
fn should_yield_now(items: &[QueueNowItem], running_id: &str) -> bool {
    let Some(running) = items.iter().find(|i| i.id == running_id) else {
        return false;
    };
    let p_run = item_priority(running);
    items
        .iter()
        .any(|i| i.state == QueueState::Waiting && item_priority(i) < p_run)
}

/// 큐 스냅샷을 떠 `should_yield_now`을 평가한다(#232). 더 높은 우선순위 Waiting 아이템이 있으면
/// true → 워커가 다음 계정 그룹을 시작하지 않고 안전지점에서 양보한다.
fn should_yield<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    let items = app.state::<JsonStore<QueueNowItem>>().snapshot();
    should_yield_now(&items, id)
}

/// 게시 그룹들을 패밀리별 계정 집합으로 가른다(#232). Naver 그룹의 account_id는 첫 집합,
/// Band 그룹은 둘째 집합으로 모인다. 잔여/완료 plan을 `retain_plan_accounts`로 복원할 때 쓴다.
fn account_sets(
    groups: &[PublishGroup],
) -> (
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    let mut naver = std::collections::BTreeSet::new();
    let mut band = std::collections::BTreeSet::new();
    for g in groups {
        match g.family {
            AccountFamily::Naver => naver.insert(g.account_id.clone()),
            AccountFamily::Band => band.insert(g.account_id.clone()),
        };
    }
    (naver, band)
}

/// plan을 계정 집합으로 필터링한 축소 plan을 만든다(#232). naver·forum 타깃은 `keep_naver`에
/// 속한 account_id만, band 타깃은 `keep_band`만, login 타깃은 그 패밀리의 keep 집합에 속한
/// account_id만 남긴다. 스칼라 필드(post_id/kind/title/body/comments/link)는 그대로 복사한다.
/// 완료분·잔여분처럼 **서로소** 집합으로 두 번 호출하면 합집합이 원본과 같아 누락·중복이 0이다.
fn retain_plan_accounts(
    plan: &PublishPlan,
    keep_naver: &std::collections::BTreeSet<String>,
    keep_band: &std::collections::BTreeSet<String>,
) -> PublishPlan {
    let login = plan.login.as_ref().map(|targets| {
        targets
            .iter()
            .filter(|t| match family_of(&t.platform) {
                AccountFamily::Naver => keep_naver.contains(&t.account_id),
                AccountFamily::Band => keep_band.contains(&t.account_id),
            })
            .cloned()
            .collect::<Vec<_>>()
    });
    PublishPlan {
        post_id: plan.post_id.clone(),
        kind: plan.kind.clone(),
        title: plan.title.clone(),
        body_text: plan.body_text.clone(),
        comments: plan.comments.clone(),
        link_override: plan.link_override.clone(),
        naver: plan
            .naver
            .iter()
            .filter(|t| keep_naver.contains(&t.account_id))
            .cloned()
            .collect(),
        forum: plan
            .forum
            .iter()
            .filter(|t| keep_naver.contains(&t.account_id))
            .cloned()
            .collect(),
        band: plan
            .band
            .iter()
            .filter(|t| keep_band.contains(&t.account_id))
            .cloned()
            .collect(),
        // 블로그(#271)는 카페와 같은 Naver 패밀리라 keep_naver로 거른다(카페·종토방과 동일).
        blog: plan
            .blog
            .iter()
            .filter(|t| keep_naver.contains(&t.account_id))
            .cloned()
            .collect(),
        // 클립(#클립)도 Naver 패밀리라 keep_naver로 거른다.
        clip: plan
            .clip
            .iter()
            .filter(|t| keep_naver.contains(&t.account_id))
            .cloned()
            .collect(),
        login,
    }
}

/// plan.forum의 종목토론방 게시 대상 계정(account_id) 집합 — 중복 제거(순수). 어떤 계정이
/// 종토방 게시 대상인지 가려, 시도조차 못 한 계정을 찾는 데 쓴다(`forum_unattempted_accounts`).
fn forum_target_accounts(plan: &PublishPlan) -> std::collections::BTreeSet<String> {
    plan.forum.iter().map(|f| f.account_id.clone()).collect()
}

/// 종목토론방 게시 대상 계정 중 **결과(`ForumOutcome`)가 하나도 없는 계정** 집합(순수, #양보누락).
/// `run_forum_targets`는 게시를 *시작한* 계정엔 성공·실패·차단·건너뜀·대기초과 어느 경우든
/// (spawn_blocking 패닉 시 합성 실패까지) outcome을 반드시 남긴다. 따라서 outcome이 0건인
/// 계정은 선점 양보(#232)나 취소 경계(`item_present`로 `run_forum_targets`가 break)로 **게시를
/// 시작조차 못 한** 계정이다 — 실패가 아니라 "아직 차례가 안 온" 것이므로 `execute_item`이
/// 큐에서 빼지(완료하지) 않고 이 계정들의 잔여 종토 plan으로 재대기시킨다(사용자 요청 2026-06-30).
fn forum_unattempted_accounts(
    plan: &PublishPlan,
    all_forum: &[ForumOutcome],
) -> std::collections::BTreeSet<String> {
    let attempted: std::collections::BTreeSet<&str> =
        all_forum.iter().map(|o| o.account_id.as_str()).collect();
    forum_target_accounts(plan)
        .into_iter()
        .filter(|acc| !attempted.contains(acc.as_str()))
        .collect()
}

/// plan을 **그 계정들의 종목토론방 타깃만** 담은 축소 plan으로 만든다(순수, #양보누락). 카페·
/// 밴드·블로그·클립 타깃과 login은 모두 비운다 — 종토방은 저장 쿠키로 게시하므로(#234) login
/// 없이 재대기해도 무방하고(login=None이라도 forum이 있어 우선순위는 종토(1)로 유지돼 대기 중
/// 로그인(0) 뒤에서 제 차례를 기다린다), 이미 끝났거나 다른 레인의 카페·밴드 작업을 재대기로
/// 다시 돌려 **중복 게시하지 않게** 한다(사수 지침: 카페·밴드 무손상). 스칼라(제목/본문/댓글/
/// 링크)는 종토 게시에 그대로 쓰므로 보존한다.
fn retain_forum_only(
    plan: &PublishPlan,
    accounts: &std::collections::BTreeSet<String>,
) -> PublishPlan {
    PublishPlan {
        post_id: plan.post_id.clone(),
        kind: plan.kind.clone(),
        title: plan.title.clone(),
        body_text: plan.body_text.clone(),
        comments: plan.comments.clone(),
        link_override: plan.link_override.clone(),
        naver: Vec::new(),
        forum: plan
            .forum
            .iter()
            .filter(|t| accounts.contains(&t.account_id))
            .cloned()
            .collect(),
        band: Vec::new(),
        blog: Vec::new(),
        clip: Vec::new(),
        login: None,
    }
}

/// 로그인 IP와 게시 직전 egress IP가 같은지 본다. 동일하면 true, 다르면 false. 단, 어느
/// 한쪽이 best-effort IP 조회 실패(`(확인 실패)`로 시작)면 검증을 막지 않으려 fail-open으로
/// true를 돌려준다 — IP를 못 읽었다는 이유로 정상 게시를 막지 않는다.
fn ip_matches(login_ip: &str, egress_ip: &str) -> bool {
    if login_ip.starts_with('(') || egress_ip.starts_with('(') {
        return true;
    }
    login_ip == egress_ip
}

/// 그룹 게시를 건너뛰게 된 사유 코드+메시지(로그인 실패 또는 IP 불일치). 합성 실패
/// BatchItem의 사유로 흘러, 조용한 누락 대신 명시적 Fail로 남는다.
struct GroupSkip {
    code: String,
    message: String,
    /// CDP/자동화 오류(`LoginOutcome::Error`)로 게시 전 로그인이 죽었을 때 캡처된 백트레이스
    /// (자세히 보기용). 비번오류·차단·IP 불일치 등 일반 실패는 None — backtrace가 없는 정상
    /// 분기라 찍어봐야 분류 코드 위치만 가리켜 무의미하다.
    trace: Option<String>,
}

impl GroupSkip {
    /// 자세히 보기 trace 본문: 원본 사유 메시지 + (CDP 오류면) 캡처된 백트레이스. 각 플랫폼의
    /// trace 빌더(`failure_trace`/`band_failure_trace` 등)가 이 문자열을 message/detail로 받아
    /// "자세히 보기"에 backtrace까지 노출한다. 메인 사유는 code 기반(`failure_reason`)이라 영향 없다.
    fn trace_body(&self) -> String {
        match &self.trace {
            Some(bt) => format!("{}\n\n{}", self.message, bt),
            None => self.message.clone(),
        }
    }
}

/// 한 그룹의 네이버 카페 글 대상을 합성 실패 리포트로 만든다(로그인/IP 실패로 게시조차 못 함).
/// `failure_reason`이 code(IP_MISMATCH 등)를 한국어 사유로 치환하고, 원본 message는 trace로.
fn synth_post_failures(plan: &PublishPlan, account_id: &str, skip: &GroupSkip) -> Vec<JobReport> {
    plan.naver
        .iter()
        .filter(|t| t.account_id == account_id)
        .map(|t| JobReport {
            account_id: t.account_id.clone(),
            cafe: t.cafe.clone(),
            menu_id: t.menu_id,
            success: false,
            result: None,
            error: Some(crate::naver_cafe::ErrorEnvelope {
                trace_id: String::new(),
                code: skip.code.clone(),
                // message는 메인 사유가 아니라 failure_trace(자세히 보기)로만 흐르므로, CDP
                // 오류면 여기에 backtrace를 실어 자세히 보기에 노출한다.
                message: skip.trace_body(),
                error_data: None,
            }),
        })
        .collect()
}

/// 한 그룹의 카페 댓글 대상을 합성 실패 항목으로 만든다(로그인/IP 실패로 댓글조차 못 함).
/// comment 전용 모드에서 댓글 대상은 글이 아니라 `comment_target` 스펙에서 나오므로, 글
/// 합성 실패(`synth_post_failures`)로는 덮이지 않는다 — 조용한 누락을 막으려 별도로 남긴다.
/// both 모드의 self-comment는 글에 의존하므로(글이 합성 실패로 이미 남음) 여기서 만들지 않는다.
/// 글목록 조회 실패(`CommentFetchFailure`)와 같은 항목 타입을 써서 완료 로그·라이브에 Fail로 뜬다.
fn synth_comment_failures(
    plan: &PublishPlan,
    account_id: &str,
    skip: &GroupSkip,
) -> Vec<CommentFetchFailure> {
    if matches!(plan.kind, ModeValue::Both) {
        return Vec::new();
    }
    plan.naver
        .iter()
        .filter(|t| t.account_id == account_id)
        .filter_map(|t| {
            let spec = t.comment_target.as_ref()?;
            let cafe_id = spec.cafe_id?;
            Some(CommentFetchFailure {
                account_id: t.account_id.clone(),
                cafe_id,
                code: skip.code.clone(),
                // 글 합성 실패와 동일: message는 자세히 보기 trace로만 흐르므로 backtrace를 싣는다.
                message: skip.trace_body(),
                cafe: None,
            })
        })
        .collect()
}

/// 한 그룹의 종목토론방 대상을 합성 실패 결과로 만든다.
fn synth_forum_failures(
    plan: &PublishPlan,
    account_id: &str,
    skip: &GroupSkip,
) -> Vec<ForumOutcome> {
    plan.forum
        .iter()
        .filter(|f| f.account_id == account_id)
        .map(|f| ForumOutcome {
            account_id: f.account_id.clone(),
            result: ForumPublishResult {
                code: f.code.clone(),
                name: f.name.clone(),
                ok: false,
                message: failure_reason(&skip.code, None),
                trace: Some(format!("{}\n{}", skip.code, skip.trace_body())),
                posted: None,
                skipped: false,
            },
        })
        .collect()
}

/// 한 그룹의 밴드 대상을 합성 실패 결과로 만든다. 밴드는 `BandPostError`만 실어 나르므로
/// IP/로그인 실패를 전송 오류(Transport)로 감싸 친절 사유+trace를 만든다.
fn synth_band_failures(plan: &PublishPlan, account_id: &str, skip: &GroupSkip) -> Vec<BandOutcome> {
    plan.band
        .iter()
        .filter(|b| b.account_id == account_id)
        .map(|b| BandOutcome {
            account_id: b.account_id.clone(),
            band_name: b.name.clone(),
            // transport 오류의 detail로 흐른다(band_failure_trace가 detail+생성지점 backtrace를
            // 실음). CDP 오류면 trace_body가 detail에 그 backtrace를 끼워 자세히 보기에 노출한다.
            result: Err(BandPostError::transport(format!(
                "{}: {}",
                skip.code,
                skip.trace_body()
            ))),
        })
        .collect()
}

/// 한 그룹의 블로그 댓글 대상을 합성 실패 결과로 만든다(#271, 로그인/IP 실패로 댓글조차 못 함).
/// `failure_reason`이 code를 한국어 사유로 치환하고, 원본 message(+backtrace)는 trace로 흐른다.
fn synth_blog_failures(plan: &PublishPlan, account_id: &str, skip: &GroupSkip) -> Vec<BlogOutcome> {
    plan.blog
        .iter()
        .filter(|b| b.account_id == account_id)
        .map(|b| BlogOutcome {
            account_id: b.account_id.clone(),
            name: b.name.clone(),
            link: b.link.clone(),
            contents: String::new(),
            result: Err(crate::naver_blog::BlogError::new(format!(
                "{} — {}",
                failure_reason(&skip.code, None),
                skip.trace_body()
            ))),
        })
        .collect()
}

/// 한 그룹의 로그인+IP 검증을 수행한다(#10004). login이 있으면 [회전→로그인]으로 IP를
/// 회전하고 로그인 IP를 캡처한 뒤, 게시 직전 egress IP가 같은지 본다 — 다르면 1회
/// 재로그인(회전 포함)하고 그래도 다르면 `IP_MISMATCH`로 실패한다. login이 None이면
/// (기존 즉시/예약 게시) 회전·로그인·IP검증을 건너뛰고 저장 쿠키로 바로 게시한다(하위호환).
/// 성공 시 `Ok(())`, 게시를 건너뛰어야 하면 `Err(GroupSkip)`.
/// 이 그룹이 **종목토론방 전용**(카페 글/댓글 대상이 없고 forum만)인지. 이런 Naver 그룹은
/// 큐 실행 시 재로그인하지 않고 선택 로그인 때 저장된 쿠키를 그대로 쓴다(사수 지침). 카페가
/// 한 대상이라도 섞이면 false → 10004(IP check) 때문에 회전+로그인+IP검증을 유지한다. 밴드
/// 그룹은 항상 false(밴드는 별도 쿠키·경로). account_id로 카페(plan.naver) 대상 유무만 본다.
fn forum_only_group(plan: &PublishPlan, group: &PublishGroup) -> bool {
    matches!(group.family, AccountFamily::Naver)
        && !plan.naver.iter().any(|t| t.account_id == group.account_id)
}

async fn prepare_group_login<R: Runtime>(
    app: &AppHandle<R>,
    group: &PublishGroup,
) -> Result<(), GroupSkip> {
    let Some(login) = group.login.as_ref() else {
        // 저장 쿠키로 바로 게시(기존 동작) — 회전/로그인/IP검증 없음.
        return Ok(());
    };

    // 1. 회전+로그인(use_adb/force는 LoginTarget 값). 실패하면 그룹 전체를 그 사유로 실패.
    let login_ip = do_login_and_capture_ip(app, login).await?;

    // 2. 게시 직전 egress IP 검증. 일치하면 게시 진행.
    let egress = crate::auth::fetch_external_ip().await;
    if ip_matches(&login_ip, &egress) {
        return Ok(());
    }

    // 3. 불일치 → 1회 재로그인(회전 포함) 후 재검증.
    tracing::warn!(
        account = %crate::auth::mask_id(&group.account_id),
        "게시 직전 IP 불일치 — 1회 재로그인 후 재검증"
    );
    let login_ip = do_login_and_capture_ip(app, login).await?;
    let egress = crate::auth::fetch_external_ip().await;
    if ip_matches(&login_ip, &egress) {
        return Ok(());
    }

    Err(GroupSkip {
        code: "IP_MISMATCH".to_owned(),
        message: "로그인 IP와 게시 IP가 끝내 일치하지 않아 게시를 건너뜀".to_owned(),
        // IP 불일치는 자동화 오류가 아니라 정상 판정이라 backtrace가 없다.
        trace: None,
    })
}

/// 한 계정을 회전+로그인하고 로그인 직후의 외부 IP를 캡처한다. 로그인이 실패(쿠키 저장
/// 실패)면 그룹을 그 사유로 건너뛰도록 `Err(GroupSkip)`을 돌려준다.
async fn do_login_and_capture_ip<R: Runtime>(
    app: &AppHandle<R>,
    login: &LoginTarget,
) -> Result<String, GroupSkip> {
    let result = if matches!(login.platform, PlatformId::Band) {
        crate::band_auth::process_band_account(
            app,
            &login.account_id,
            login.headless,
            login.use_adb,
            login.force,
        )
        .await
    } else {
        crate::auth::process_account(
            app,
            &login.account_id,
            login.headless,
            login.use_adb,
            login.force,
        )
        .await
    };
    // 게시 직전 로그인 결과를 계정 상태 배지에 반영한다(게시 경로에서 상태가 갱신되지 않던
    // 문제 수정). 성공이면 Active(정상), 실패면 BadCredentials/Challenge/Blocked/Error로 바뀐다.
    // loginId가 같은 모든 행을 함께 갱신한다(run_login_targets와 동일). 실패 사유 자체는
    // 완료 로그(LogBatch)가 이미 보여주므로 여기선 상태만 갱신해 활동 피드 스팸을 피한다.
    // trace는 CDP/자동화 오류에서만 채워진다(비번오류·차단은 None) — 게시 그룹 경로도
    // 순수 로그인 아이템처럼 그 backtrace를 자세히 보기에 보존한다.
    let (status, msg, trace) = resolve_login_status(&result);
    app.state::<JsonStore<crate::ipc::accounts::Account>>()
        .mutate(|list| {
            crate::ipc::accounts::apply_status_by_login_id(
                list,
                &login.account_id,
                status.clone(),
                Some(msg.clone()),
            )
        });
    let succeeded = matches!(&result, Ok(res) if res.succeeded);
    if !succeeded {
        return Err(GroupSkip {
            code: "LOGIN_FAILED".to_owned(),
            message: msg,
            trace,
        });
    }
    Ok(crate::auth::fetch_external_ip().await)
}


/// 종목토론방 대상을 **계정별로 동시에** 게시한다(#237). 계정마다 디버그 포트 Chrome을 직접
/// 띄우는데(`launch_debug_chrome` = 빈 포트 자동배정 + 고유 프로필), 포트·프로필이 모두 달라
/// 여러 개를 동시에 띄워도 충돌이 없다. 카페(9222)·밴드(HTTP)와도 자원이 겹치지 않아, 카페/밴드가
/// 도는 중에도 종토방은 병렬로 흐른다. 동시 수는 사용자 설정 "최대 작동 가능 작업 수"(#284, 0=무제한)를 따른다. 계정·종목별
/// 결과를 돌려준다(완료 로그용). Chrome 기동/태스크 실패 시 그 계정의 종목들을 실패 결과로
/// 합성해 진행률·로그가 조용히 누락되지 않게 한다(거짓 100% 방지). 각 묶음 시작 전 협조적
/// 취소(item_present)를 확인해, 취소된 아이템의 남은 계정은 게시하지 않는다.
async fn run_forum_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    id: &str,
    base_items: Vec<BatchItem>,
    base_done: u32,
    total: u32,
    account_filter: Option<&str>,
) -> Vec<ForumOutcome> {
    // 계정 그룹 게시(#10004)에서는 한 계정의 종토방만 처리하도록 필터링한다. None이면 전체.
    // skeleton·outcomes·콜백 인덱스가 모두 이 필터된 요청 목록(reqs)을 단일 기준으로 쓴다.
    let reqs: Vec<ForumPublishRequest> = plan_to_forum_requests(plan)
        .into_iter()
        .filter(|r| account_filter.is_none_or(|acc| r.account_id == acc))
        .collect();
    // 모든 종목을 "대기 중"으로 미리 깔고(스켈레톤), 종목이 시작/완료될 때마다 그 자리만
    // 진행 중→완료/실패로 바꾼다(로그인 화면과 동일 UX, #219 — 완료된 것만 보이던 문제 해결).
    // blocking 스레드의 콜백과 공유하므로 Arc<Mutex>로 들고 다닌다. 인덱스 순서는
    // reqs 순서(=skeleton·outcomes 순서)와 일치한다.
    // [가시성] 어떤 계정들이 몇 종목씩, 몇 개씩 동시에 게시되는지 시작 시 한 줄로 남긴다 — 묶음
    // 대기 중이라 아직 글 로그가 없는 계정도 "무엇을 기다리는지" 보이게(사용자 지적 2026-06-30).
    {
        let who_list: Vec<String> = reqs
            .iter()
            .map(|r| {
                format!(
                    "{}({}종목)",
                    crate::auth::mask_id(&r.account_id),
                    r.stocks.len()
                )
            })
            .collect();
        let limit = read_concurrency_limit(app);
        let cap_label = if limit == 0 {
            "무제한(전부 동시)".to_owned()
        } else {
            format!("{limit}개씩")
        };
        tracing::info!(
            "[POST] 종목토론방 게시 시작 — {}계정을 묶음당 최대 {} 동시 게시(최대 작동 작업 수 설정): {}",
            reqs.len(),
            cap_label,
            who_list.join(", ")
        );
    }
    let forum_live = Arc::new(Mutex::new(forum_skeleton_items(&reqs)));
    {
        let live = lock_or_poisoned(&forum_live);
        write_live_phase(app, id, &base_items, &live, base_done, total);
    }
    // 계정별 스켈레톤 오프셋을 미리 계산한다 — 각 계정은 자기 슬롯(off+local)만 갱신하므로
    // 동시에 돌려도 forum_live(Arc<Mutex>) 충돌이 없다(#237).
    let mut starts = Vec::with_capacity(reqs.len());
    let mut acc_off = 0usize;
    for req in &reqs {
        starts.push(acc_off);
        acc_off += req.stocks.len();
    }
    // 계정마다 전용 디버그 Chrome을 띄워 동시에 게시한다. 한 묶음에 띄울 계정 수는 사용자 설정
    // "최대 작동 가능 작업 수"(#284)를 따른다 — 0이면 무제한(남은 전 계정을 한 묶음에 동시 게시).
    // 예전엔 FORUM_PARALLEL_CAP(4)로 하드코딩돼, 사용자가 30~40계정을 넣어도 4개씩만 돌던 문제를
    // 고친다(사용자 지적 2026-06-30: #284로 제한을 푼 뒤에도 이 내부 캡이 남아 따로 놀았다).
    // 한도는 묶음마다 다시 읽어, 사용자가 도중에 한도를 바꿔도 다음 묶음부터 반영된다. 결과는
    // 계정(req) 순서대로 모은다(#237).
    let mut outcomes = Vec::new();
    let mut req_iter = reqs.into_iter().enumerate();
    loop {
        // 묶음 시작 전 취소 확인 — 취소됐으면 남은 계정은 게시하지 않는다.
        if !item_present(app, id) {
            break;
        }
        let limit = read_concurrency_limit(app);
        let batch_size = if limit == 0 {
            usize::MAX
        } else {
            limit as usize
        };
        let mut handles = Vec::new();
        for _ in 0..batch_size {
            let Some((idx, req)) = req_iter.next() else {
                break;
            };
            let account_id = req.account_id.clone();
            // spawn_blocking 태스크가 패닉(JoinError)하면 결과를 잃으므로, 합성 실패에 쓸
            // 종목 목록을 미리 복제해 둔다(누락 대신 명시 실패).
            let stocks_for_panic = req.stocks.clone();
            let app_for_job = app.clone();
            // 종목 시작/완료마다(blocking 스레드) 스켈레톤의 해당 칸만 바꾸기 위한 캡처들.
            let off = starts[idx];
            let app_start = app.clone();
            let id_start = id.to_owned();
            let base_start = base_items.clone();
            let live_start = Arc::clone(&forum_live);
            let app_done = app.clone();
            let id_done = id.to_owned();
            let base_done_items = base_items.clone();
            let account_done = account_id.clone();
            let live_done = Arc::clone(&forum_live);
            let app_retry = app.clone();
            let id_retry = id.to_owned();
            let base_retry = base_items.clone();
            let live_retry = Arc::clone(&forum_live);
            let handle = tauri::async_runtime::spawn_blocking(move || {
                // 종목 게시 시작 직전: 그 종목을 "게시 중"으로(진행률은 그대로 — 완료분만 센다).
                let on_start = move |local: usize| {
                    let mut live = lock_or_poisoned(&live_start);
                    if let Some(it) = live.get_mut(off + local) {
                        it.status = BatchItemStatus::Running;
                        it.msg = "게시 중…".to_owned();
                    }
                    write_live_phase(&app_start, &id_start, &base_start, &live, base_done, total);
                };
                // 종목 완료 직후: 그 자리만 성공/실패로 교체(60초 대기 전에 갱신).
                let on_result = move |local: usize, result: &ForumPublishResult| {
                    let mut live = lock_or_poisoned(&live_done);
                    if let Some(slot) = live.get_mut(off + local) {
                        *slot = forum_result_to_item(&account_done, result);
                    }
                    write_live_phase(
                        &app_done,
                        &id_done,
                        &base_done_items,
                        &live,
                        base_done,
                        total,
                    );
                };
                // 종목 재시도마다: 그 칸을 "재시도중 N/M (대기초과)"으로 — 오래 걸리는 종목이
                // "게시 중…"으로 멈춘 듯/사라진 듯 보이지 않게 한다(사용자 지적 2026-06-30).
                let on_retry = move |local: usize, attempt: usize, max: usize| {
                    let mut live = lock_or_poisoned(&live_retry);
                    if let Some(it) = live.get_mut(off + local) {
                        it.status = BatchItemStatus::Running;
                        it.msg = format!("재시도중 {attempt}/{max} (대기초과)");
                    }
                    write_live_phase(&app_retry, &id_retry, &base_retry, &live, base_done, total);
                };
                match crate::auth::launch_debug_chrome(true) {
                    Ok(chrome) => {
                        let mut req = req;
                        // host는 plan_to_forum_requests에서 이미 127.0.0.1; 포트만 띄운 Chrome 값으로.
                        req.port = chrome.port;
                        // [가시성] 이 계정의 게시가 "지금 시작됐다"를 남긴다 — 종목 글 로그가 나오기
                        // 전(전용 Chrome 띄우고 첫 글 여는 동안)에도 어느 계정이 도는지 보이게 한다.
                        tracing::info!(
                            "[POST] {} 종목토론방 게시 시작 — 전용 Chrome(포트 {}) · {}종목",
                            crate::auth::mask_id(&req.account_id),
                            chrome.port,
                            req.stocks.len()
                        );
                        let results =
                            run_forum_publish(req, app_for_job, on_start, on_result, on_retry);
                        drop(chrome);
                        results
                    }
                    // Chrome 기동 실패 → 이 계정 종목 전부 실패로 기록(누락 대신 명시). on_result로
                    // 흘려 그 자리들을 즉시 실패로 바꾼다(다음 계정까지 대기 중으로 멈춰 보이지 않게).
                    Err(error) => {
                        // 인프라 실패(엔진 진입 전)는 backtrace가 없어 메시지를 trace로도 쓴다.
                        let synth: Vec<ForumPublishResult> = req
                            .stocks
                            .iter()
                            .map(|s| {
                                let message = format!("Chrome 실행 실패: {error}");
                                ForumPublishResult {
                                    code: s.code.clone(),
                                    name: s.name.clone(),
                                    ok: false,
                                    trace: Some(message.clone()),
                                    message,
                                    posted: None,
                                    skipped: false,
                                }
                            })
                            .collect();
                        for (i, result) in synth.iter().enumerate() {
                            on_result(i, result);
                        }
                        synth
                    }
                }
            });
            handles.push((account_id, stocks_for_panic, handle));
        }
        if handles.is_empty() {
            break;
        }
        // 묶음 내 계정들을 동시에 기다린다(각자 자기 Chrome). 결과는 spawn 순서대로 모은다.
        for (account_id, stocks_for_panic, handle) in handles {
            let results = match handle.await {
                Ok(results) => results,
                // 블로킹 태스크 패닉 → 그 계정 종목 전부 실패로 합성(누락 방지).
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
                            posted: None,
                            skipped: false,
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
    base_items: Vec<BatchItem>,
    base_done: u32,
    total: u32,
    account_filter: Option<&str>,
) -> Vec<BandOutcome> {
    // 계정 그룹 게시(#10004)에서는 한 계정의 밴드만 처리한다. None이면 전체. skeleton·live·
    // outcomes 인덱스가 모두 이 필터된 대상 목록(targets)을 단일 기준으로 쓴다.
    let targets: Vec<&crate::ipc::queue::BandTarget> = plan
        .band
        .iter()
        .filter(|t| account_filter.is_none_or(|acc| t.account_id == acc))
        .collect();
    // 밴드는 종목이 없어 #{링크}만 치환한다(링크값 있으면 그 값, 없으면 빈 문자열).
    let band_link = crate::template_tokens::resolve_link(&plan.link_override, "");
    let band_title = crate::template_tokens::resolve_cafe_band(&plan.title, &band_link);
    let band_body = crate::template_tokens::resolve_cafe_band(&plan.body_text, &band_link);
    // comment/both면 댓글 풀 전체를 넘긴다. post/both는 새 글에, comment 전용은 기존
    // 글(최신/인기)에 같은 풀을 분배해 단다. post 전용 모드면 빈 슬라이스라 댓글 없음.
    let resolved_comments: Vec<String> = if runs_comment(plan) {
        plan.comments
            .iter()
            .map(|c| crate::template_tokens::resolve_cafe_band(c, &band_link))
            .collect()
    } else {
        Vec::new()
    };
    let comments: &[String] = &resolved_comments;
    // 댓글 전용 모드는 새 글을 쓰지 않는다. band_publish(create_post)는 리더 승인제
    // 밴드에서 result_code=1003("리더 승인 후 등록")을 부르므로, 기존 글에 댓글을 다는
    // band_comment로 간다(즉시게시 runNow의 댓글 전용 경로와 동일).
    let comment_only = matches!(plan.kind, ModeValue::Comment);
    // 모든 밴드를 "대기 중"으로 미리 깔고(스켈레톤), 대상이 시작/완료될 때마다 그 자리만
    // 게시 중→완료/실패로 바꾼다(종토방·로그인과 동일 UX, #219).
    let mut live = band_skeleton_items(&targets);
    write_live_phase(app, id, &base_items, &live, base_done, total);
    let mut outcomes = Vec::new();
    for (i, t) in targets.iter().enumerate() {
        // 밴드 게시도 비가역적이라, 시작 전마다 취소를 확인해 멈춘다(forum과 동일).
        if !item_present(app, id) {
            break;
        }
        // 게시 시작 표시(진행률은 완료분만 세므로 그대로).
        if let Some(it) = live.get_mut(i) {
            it.status = BatchItemStatus::Running;
            it.msg = "게시 중…".to_owned();
        }
        write_live_phase(app, id, &base_items, &live, base_done, total);
        let is_url_comment =
            matches!(&t.comment_target, Some(spec) if matches!(spec.mode, CommentTarget::Url));
        let result = if comment_only && is_url_comment {
            // "특정 게시글" 댓글: link가 글 URL(band_no+post_no 포함)이라 피드 조회 없이 그
            // 글 하나에 직접 댓글을 단다(카페·종토방 url 댓글의 밴드판).
            band_comment_on_post(&t.account_id, &t.link, comments)
                .await
                .map(BandJobResult::Commented)
        } else if comment_only {
            // 대상 spec에서 정렬·개수를 꺼낸다(최신/인기 글목록). spec이 없으면(비정상)
            // 최신글 1개 기본 — 어떤 경우에도 새 글은 쓰지 않는다.
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
            band_publish(&t.account_id, &t.link, &band_title, &band_body, comments)
                .await
                .map(BandJobResult::Published)
        };
        let outcome = BandOutcome {
            account_id: t.account_id.clone(),
            band_name: t.name.clone(),
            result,
        };
        // 대상 1건 완료마다 그 자리만 성공/실패로 교체하고 진행률·라이브 상태를 갱신한다(#219).
        if let Some(slot) = live.get_mut(i) {
            *slot = band_outcome_to_item(
                &outcome,
                &band_title,
                &band_body,
                resolved_comments
                    .iter()
                    .find(|c| !c.trim().is_empty())
                    .map(String::as_str),
            );
        }
        write_live_phase(app, id, &base_items, &live, base_done, total);
        outcomes.push(outcome);
    }
    outcomes
}

/// 네이버 블로그 댓글 대상을 순차로 게시한다(#271). 블로그는 댓글 전용이라 저장된 네이버
/// 쿠키(카페와 공유)로 각 글에 댓글을 단다. 댓글 본문은 cafe/band와 동일하게 plan.comments를
/// (토큰 치환 후) 합쳐 만든다 — 비어있지 않은 댓글을 줄바꿈으로 이어 한 댓글로 단다. 밴드와
/// 똑같이 모든 대상을 "대기 중"으로 깔고, 대상이 시작/완료될 때마다 그 자리만 게시 중→완료/실패로
/// 바꾼다(#219). 성공/실패 결과를 계정·표시 이름과 묶어 돌려준다(완료 로그용). 각 대상 시작 전
/// 협조적 취소(item_present)를 확인한다.
async fn run_blog_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    id: &str,
    base_items: Vec<BatchItem>,
    base_done: u32,
    total: u32,
    account_filter: Option<&str>,
) -> Vec<BlogOutcome> {
    let targets: Vec<&crate::ipc::queue::BlogTarget> = plan
        .blog
        .iter()
        .filter(|t| account_filter.is_none_or(|acc| t.account_id == acc))
        .collect();
    // 블로그는 종목이 없어 #{링크}만 치환한다(링크값 있으면 그 값, 없으면 빈 문자열). cafe와
    // 동일하게 비어있지 않은 댓글을 줄바꿈으로 합쳐 한 댓글 본문으로 만든다.
    let blog_link = crate::template_tokens::resolve_link(&plan.link_override, "");
    let contents: String = plan
        .comments
        .iter()
        .map(|c| crate::template_tokens::resolve_cafe_band(c, &blog_link))
        .filter(|c| !c.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // "최신 N개" 모드(count=Some) 대상은 실행 시점에 글 목록을 조회해 글마다 댓글 대상 1건으로
    // 펼친다(카페 collect_comment_targets 미러). URL 모드(count=None)는 그 글 1건이 곧 대상이다.
    // 글이 모자라면(전체 글 수 < N) 있는 만큼만 댓글을 달고, 부족분만큼 "글이 없습니다" 실패를
    // 즉시 만든다(대기/재시도 없이). 조회 자체가 실패하면 그 대상은 댓글을 못 달므로 실패로 남긴다.
    let mut work: Vec<BlogWorkItem> = Vec::new();
    for t in &targets {
        match t.count {
            None => work.push(BlogWorkItem::Comment(BlogCommentJob {
                account_id: t.account_id.clone(),
                name: t.name.clone(),
                link: t.link.clone(),
                blog_id: t.blog_id.clone(),
                log_no: t.log_no.clone(),
            })),
            Some(n) => {
                let want = (n.max(1)) as usize;
                let category_no = t.category_no.unwrap_or(0);
                match crate::naver_blog::fetch_latest_blog_posts_for_account(
                    &t.account_id,
                    &t.blog_id,
                    category_no,
                    want,
                )
                .await
                {
                    Ok(list) => {
                        for post in &list.posts {
                            work.push(BlogWorkItem::Comment(BlogCommentJob {
                                account_id: t.account_id.clone(),
                                name: t.name.clone(),
                                link: format!(
                                    "https://blog.naver.com/{}/{}",
                                    t.blog_id, post.log_no
                                ),
                                blog_id: t.blog_id.clone(),
                                log_no: post.log_no.clone(),
                            }));
                        }
                        // 글이 모자라면(있는 글 < N) 부족분만큼 즉시 실패로 남긴다("글이 없습니다").
                        // 클라이언트가 이미 N·totalCount까지만 모으므로, 실제로 댓글을 달 수 있는
                        // 글 수는 곧 모은 글 수다.
                        let available = list.posts.len() as u32;
                        let shortfall = n.max(1).saturating_sub(available);
                        for _ in 0..shortfall {
                            work.push(BlogWorkItem::Shortfall {
                                account_id: t.account_id.clone(),
                                name: t.name.clone(),
                                link: t.link.clone(),
                            });
                        }
                    }
                    // 글 목록 조회 실패 → 이 대상은 댓글을 못 단다. 조용히 누락하지 않고 실패로 남긴다.
                    Err(error) => work.push(BlogWorkItem::FetchFailure {
                        account_id: t.account_id.clone(),
                        name: t.name.clone(),
                        link: t.link.clone(),
                        error,
                    }),
                }
            }
        }
    }

    let mut live = blog_work_skeleton_items(&work);
    write_live_phase(app, id, &base_items, &live, base_done, total);
    let mut outcomes = Vec::new();
    // 도배 방지: 한 댓글을 올린 뒤 다음 댓글까지 10초 텀을 둔다. 첫 댓글은 즉시 단다.
    let mut posted_any = false;
    for (i, w) in work.into_iter().enumerate() {
        let outcome = match w {
            // 조회 실패/글 부족은 네트워크 호출 없이 곧장 실패 결과로 굳힌다(대기/재시도 없음).
            BlogWorkItem::FetchFailure {
                account_id,
                name,
                link,
                error,
            } => BlogOutcome {
                account_id,
                name,
                link,
                contents: String::new(),
                result: Err(error),
            },
            BlogWorkItem::Shortfall {
                account_id,
                name,
                link,
            } => BlogOutcome {
                account_id,
                name,
                link,
                contents: String::new(),
                result: Err(crate::naver_blog::BlogError::new("글이 없습니다")),
            },
            BlogWorkItem::Comment(job) => {
                if !item_present(app, id) {
                    break;
                }
                // 직전 댓글이 올라갔으면 다음 댓글까지 10초 대기(도배 방지). 정지를 빠르게
                // 반영하려고 100ms씩 쪼개 대기하고, 도중 큐에서 빠지면 멈춘다.
                if posted_any {
                    for _ in 0..100 {
                        if !item_present(app, id) {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    if !item_present(app, id) {
                        break;
                    }
                }
                if let Some(it) = live.get_mut(i) {
                    it.status = BatchItemStatus::Running;
                    it.msg = "댓글 게시 중…".to_owned();
                }
                write_live_phase(app, id, &base_items, &live, base_done, total);
                let result = crate::naver_blog::create_blog_comment_for_account(
                    &job.account_id,
                    &job.blog_id,
                    &job.log_no,
                    &contents,
                )
                .await;
                // 성공/실패와 무관하게 "한 댓글 시도"가 끝났으므로 다음부터 10초 텀을 적용한다.
                posted_any = true;
                BlogOutcome {
                    account_id: job.account_id,
                    name: job.name,
                    link: job.link,
                    contents: contents.clone(),
                    result,
                }
            }
        };
        if let Some(slot) = live.get_mut(i) {
            *slot = blog_outcome_to_item(&outcome);
        }
        write_live_phase(app, id, &base_items, &live, base_done, total);
        outcomes.push(outcome);
    }
    outcomes
}

/// `run_blog_targets`가 펼친 블로그 댓글 작업 1건. URL/최신 N개 모드를 한 목록으로 합친다(#279).
enum BlogWorkItem {
    /// 실제로 댓글을 달 글 1건(URL 모드의 그 글, 또는 최신 N개로 펼친 글 1개).
    Comment(BlogCommentJob),
    /// 글이 모자라(전체 글 < N) 댓글을 못 다는 자리 — "글이 없습니다" 실패로 남긴다.
    Shortfall {
        account_id: String,
        name: String,
        link: String,
    },
    /// 글 목록 조회 자체가 실패한 대상 — 그 오류(backtrace 포함)를 실패로 남긴다.
    FetchFailure {
        account_id: String,
        name: String,
        link: String,
        error: crate::naver_blog::BlogError,
    },
}

/// 댓글을 달 블로그 글 1건의 동결된 실행 정보.
struct BlogCommentJob {
    account_id: String,
    name: String,
    link: String,
    blog_id: String,
    log_no: String,
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

/// 로그인 계정 1건의 대상별 라이브 상태(#219). target/login_id 모두 계정 ID를 쓴다(사용자
/// 본인 계정이라 마스킹 불필요). 완료 로그(LogBatch)는 만들지 않으므로 큐 펼침 전용이다.
fn login_item(
    t: &LoginTarget,
    status: BatchItemStatus,
    msg: String,
    trace: Option<String>,
) -> BatchItem {
    BatchItem {
        platform: t.platform.clone(),
        target: t.account_id.clone(),
        code: None,
        board: None,
        login_id: t.account_id.clone(),
        status,
        msg,
        trace,
        posted: None,
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
    // 계정별 라이브 상태(#219): 처음엔 모두 "대기 중", 처리 중인 계정은 "로그인 중…",
    // 끝나면 성공/실패로 교체한다(펼침 시 어느 계정이 진행 중인지 보이게).
    let mut items: Vec<BatchItem> = targets
        .iter()
        .map(|t| login_item(t, BatchItemStatus::Waiting, "대기 중".to_owned(), None))
        .collect();
    set_queue_items(app, id, items.clone());
    let mut done = 0u32;
    let mut ok = 0u32;
    for (i, t) in targets.iter().enumerate() {
        // 로그인은 비싸고(브라우저 기동) 비가역적이라 시작 전마다 취소를 확인한다.
        if !item_present(app, id) {
            break;
        }
        items[i] = login_item(t, BatchItemStatus::Running, "로그인 중…".to_owned(), None);
        set_queue_items(app, id, items.clone());
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
        items[i] = login_item(t, status_of(succeeded), msg.clone(), trace.clone());
        set_progress_and_items(app, id, done, total, items.clone());
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
        // 10004: "IP check failure" — 로그인한 IP와 게시 요청 IP가 달라 네이버가 거부.
        // 원문("NaverUser 인증 실패 - loginStat…")이 혼란스러워 행동 가능한 문구로 덮어쓴다.
        "10004" => Some(
            "로그인한 IP와 게시 IP가 달라 인증에 실패했습니다. 해당 계정을 다시 로그인해 주세요",
        ),
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
        // 로그인 IP와 게시 IP가 끝내 일치하지 않아 그 계정 게시를 건너뛴 경우(#10004 예방).
        "IP_MISMATCH" => Some(
            "로그인 IP와 게시 IP가 달라 게시를 건너뛰었습니다(IP가 계속 변동). 잠시 후 다시 시도해 주세요",
        ),
        // 게시 직전 계정 로그인이 실패해 그 계정 게시를 건너뛴 경우(#10004 원자화).
        "LOGIN_FAILED" => Some("게시 전 로그인에 실패해 이 계정의 게시를 건너뛰었습니다"),
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

/// 카페 ID(문자열) → 표시 이름. plan에 동결된 이름을 써서 로그/큐 표시에 ID 대신 카페 명을
/// 보여준다. 이름이 비었거나 매칭이 없으면 ID로 폴백한다.
fn cafe_label(plan: &PublishPlan, cafe: &str) -> String {
    plan.naver
        .iter()
        .find(|t| t.cafe == cafe && !t.cafe_name.is_empty())
        .map_or_else(|| cafe.to_owned(), |t| t.cafe_name.clone())
}

/// 카페 글 게시 결과 1건 → BatchItem.
/// 카페 글/댓글의 읽기 URL을 cafe_id+article_id로 조립한다(글쓰기 엔드포인트와 동일
/// 계열의 `ca-fe/cafes/{cafeId}/articles/{articleId}`). 완료 로그·게시 큐의
/// "올라간 글 열기"가 이 URL을 외부 브라우저로 연다.
fn cafe_article_url(cafe_id: u64, article_id: u64) -> String {
    format!("https://cafe.naver.com/ca-fe/cafes/{cafe_id}/articles/{article_id}")
}

fn post_report_to_item(plan: &PublishPlan, r: &JobReport) -> BatchItem {
    BatchItem {
        platform: PlatformId::Naver,
        target: cafe_label(plan, &r.cafe),
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
        trace: r
            .error
            .as_ref()
            .map(|e| failure_trace(&e.code, &e.message, e.error_data.as_ref().map(|d| &d.cafe))),
        // 성공 시 작성 내용(제목/본문)과 등록 결과(cafe_id/article_id)로 만든 글 URL을 채운다.
        // 카페 엔진은 제목/본문을 결과로 돌려주지 않지만, 게시한 내용은 plan에 그대로 있고
        // 대상별로 동일하다(plan_to_post_jobs와 같은 #{링크} 치환). 밴드·종토방처럼 작성 내용을
        // 실어, 빛삭돼도 무엇을 보냈는지 + "올라간 글 열기"가 되게 한다(#219).
        posted: r.result.as_ref().map(|res| {
            let link = crate::template_tokens::resolve_link(&plan.link_override, "");
            PostedContent {
                title: crate::template_tokens::resolve_cafe_band(&plan.title, &link),
                body: crate::template_tokens::resolve_cafe_band(&plan.body_text, &link),
                // 카페 댓글은 별도 BatchItem(comment_report_to_item)으로 표시하므로 비운다.
                comment: None,
                url: Some(cafe_article_url(res.cafe_id, res.article_id)),
            }
        }),
    }
}

/// 댓글 1건을 "대기/진행 중" BatchItem으로 만든다(라이브 스켈레톤, #252). 결과가 나오면
/// [`comment_report_to_item`]으로 교체된다.
fn comment_skeleton(plan: &PublishPlan, job: &CommentJob, status: BatchItemStatus) -> BatchItem {
    let msg = if matches!(status, BatchItemStatus::Running) {
        "댓글 게시 중…"
    } else {
        "댓글 게시 전"
    };
    BatchItem {
        platform: PlatformId::Naver,
        target: cafe_label(plan, &job.cafe_id.to_string()),
        code: None,
        board: None,
        login_id: job.account_id.clone(),
        status,
        msg: msg.to_owned(),
        trace: None,
        posted: None,
    }
}

/// 카페 댓글 게시 결과 1건 → BatchItem.
fn comment_report_to_item(plan: &PublishPlan, r: &CommentJobReport) -> BatchItem {
    BatchItem {
        platform: PlatformId::Naver,
        target: cafe_label(plan, &r.cafe_id.to_string()),
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
        trace: r
            .error
            .as_ref()
            .map(|e| failure_trace(&e.code, &e.message, e.error_data.as_ref().map(|d| &d.cafe))),
        // 성공 시 단 댓글 본문과, 댓글을 단 대상 글로 이동할 수 있게 그 글 URL을 채운다(#219).
        posted: r.success.then(|| PostedContent {
            comment: Some(r.content.clone()),
            url: Some(cafe_article_url(r.cafe_id, r.article_id)),
            ..Default::default()
        }),
    }
}

/// 글목록 조회 실패로 댓글을 시도조차 못 한 대상 1건 → 실패 BatchItem.
fn fetch_failure_to_item(plan: &PublishPlan, f: &CommentFetchFailure) -> BatchItem {
    BatchItem {
        platform: PlatformId::Naver,
        target: cafe_label(plan, &f.cafe_id.to_string()),
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
        posted: None,
    }
}

/// 종목토론방 게시 실패의 기술 메시지(AutomationError)를 비개발자용 한국어 사유로 바꾼다(#243).
/// 우선순위: (1) 메시지에 HTTP 상태코드가 있으면 카페와 동일한 [`status_reason`] 매핑 재사용
/// (예: 403 → "권한이 없거나 로그인이 만료되었습니다"), (2) 매크로가 만든 이미-친절한 한국어
/// 안내(로그인 미확인/입력 누락 등)는 그대로 노출, (3) 그 외 개발 용어가 섞인 기술 원문
/// ("…패킷 HTTP 실패: HTTP status …", "txId를 찾지 못했습니다" 등)은 일반 폴백으로 가린다.
/// 원문 기술 메시지는 호출부가 trace(자세히 보기)에 보존해 개발자가 확인할 수 있게 한다.
fn forum_failure_reason(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return "종목토론방 게시에 실패했습니다".to_owned();
    }
    if let Some(status) = parse_http_status(trimmed) {
        return status_reason(status).to_owned();
    }
    // 전송 계층(연결/DNS/타임아웃) 실패는 HTTP 상태가 없어 위 매핑에 안 걸린다. 이를 잠금 폴백으로
    // 흘리면 "로그인·잠금 확인"이라는 틀린 안내가 떠(잠긴 게 아니라 망이 끊긴 것) — #330과 같은
    // 부류의 오안내. 네트워크 끊김으로 명확히 분류해 "잠시 후 재시도" 안내를 준다(재시도로 풀린다).
    if is_network_transport_failure(trimmed) {
        return "잠시 인터넷 연결이 끊겨 게시에 실패했습니다. 잠시 후 다시 시도해 주세요".to_owned();
    }
    // 개발 용어가 섞이지 않은 순수 안내문이면 사용자 친화로 보고 그대로 노출한다.
    if contains_tech_jargon(trimmed) {
        "게시에 실패했습니다. 계정 로그인·잠금 상태를 확인한 뒤 다시 시도해 주세요".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// 메시지에 섞인 HTTP 상태코드(3자리, "status" 토큰 뒤 첫 정수)를 추출한다(#243). packet_client가
/// 만드는 "… HTTP status 403 Forbidden …" / "status=500, …" / reqwest "HTTP status: 403 …"
/// 형식을 모두 잡는다.
fn parse_http_status(message: &str) -> Option<u16> {
    let lower = message.to_ascii_lowercase();
    let after = &message[lower.find("status")? + "status".len()..];
    let digits: String = after
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse::<u16>()
        .ok()
        .filter(|n| (100..=599).contains(n))
}

/// 메시지가 HTTP 전송 계층(연결/DNS/타임아웃) 실패인지 식별한다(#330 후속). packet_client가
/// send 실패에 붙이는 한국어 접두어("전송 실패")와, reqwest가 남기는 영어 표식(connect/dns/
/// timeout 등)을 함께 본다. HTTP status가 붙는 응답 단계 실패와 달리 상태코드가 없어, 잠금
/// 폴백으로 새기 전에 여기서 '네트워크 끊김'으로 분리한다.
fn is_network_transport_failure(message: &str) -> bool {
    if message.contains("전송 실패") {
        return true;
    }
    let lower = message.to_ascii_lowercase();
    const MARKERS: [&str; 6] = [
        "error sending request",
        "tcp connect",
        "dns error",
        "timed out",
        "timeout",
        "connection refused",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

/// 사용자에게 그대로 보여주면 안 되는 개발 용어가 들어 있는지(#243). 종토방 매크로/패킷
/// 클라이언트의 기술 원문을 거르는 데 쓴다.
fn contains_tech_jargon(message: &str) -> bool {
    const JARGON: [&str; 9] = [
        "패킷",
        "txId",
        "HTTP",
        "POST",
        "PUT",
        "DevTools",
        "소켓",
        "응답 읽기",
        "JSON",
    ];
    JARGON.iter().any(|j| message.contains(j))
}

/// 종목토론방 게시 결과 1건(계정+종목별) → BatchItem. 라이브 갱신과 최종 로그가 공유한다.
fn forum_result_to_item(account_id: &str, result: &ForumPublishResult) -> BatchItem {
    BatchItem {
        platform: PlatformId::Forum,
        target: result.name.clone(),
        code: Some(result.code.clone()),
        board: None,
        login_id: account_id.to_owned(),
        // 차단 계정으로 건너뛴 글(#267-9)은 X(실패)가 아니라 "건너뜀(Skip)"으로 구분한다.
        status: if result.skipped {
            BatchItemStatus::Skip
        } else {
            status_of(result.ok)
        },
        // 성공은 엔진 문구("게시 완료"+URL). 실패는 비개발자용 한국어 사유로 변환해 "왜
        // 실패했는지"를 한눈에 보이게 한다(#243: 카페 failure_reason과 동일 철학). 건너뜀은
        // run_forum_publish가 만든 안내문("앞선 글이 …건너뜀")을 그대로 보여준다.
        msg: if result.skipped || result.ok {
            result.message.clone()
        } else {
            forum_failure_reason(&result.message)
        },
        // 실패 시 친절 사유로 가려진 원본 기술 메시지를 자세히 보기 맨 위에 보존한다(#243). 성공·
        // 건너뜀은 그대로(없음). AutomationError::trace()는 위치+백트레이스만 담아 message가 빠지므로 합친다.
        trace: if result.skipped || result.ok {
            result.trace.clone()
        } else {
            Some(match &result.trace {
                Some(t) => format!("{}\n\n{}", result.message, t),
                None => result.message.clone(),
            })
        },
        // master #218: 종목별 게시 내용(제목/본문/댓글/URL)을 완료 로그·라이브 표시에 보존한다.
        posted: result.posted.clone(),
    }
}

fn forum_outcome_to_item(o: &ForumOutcome) -> BatchItem {
    forum_result_to_item(&o.account_id, &o.result)
}

/// 종목토론방 전체 종목을 "대기 중" BatchItem으로(라이브 스켈레톤). 순서는 호출부가 넘긴
/// 요청 목록(계정 그룹) × 종목 순서 = `outcomes`/콜백 인덱스 순서와 일치한다. 계정 필터링은
/// 호출부가 요청 목록을 거르며 끝내므로, 여기서는 받은 목록을 그대로 펼친다.
fn forum_skeleton_items(reqs: &[ForumPublishRequest]) -> Vec<BatchItem> {
    let mut out = Vec::new();
    for req in reqs {
        for s in &req.stocks {
            out.push(BatchItem {
                platform: PlatformId::Forum,
                target: s.name.clone(),
                code: Some(s.code.clone()),
                board: None,
                login_id: req.account_id.clone(),
                status: BatchItemStatus::Waiting,
                msg: "대기 중".to_owned(),
                trace: None,
                posted: None,
            });
        }
    }
    out
}

/// 밴드 게시 결과 1건 → BatchItem.
fn band_outcome_to_item(
    o: &BandOutcome,
    title: &str,
    body: &str,
    comment: Option<&str>,
) -> BatchItem {
    // post/both는 새 글(+댓글), comment 전용은 기존 글 댓글. 둘 다 부분 실패를 드러낸다
    // (성공분이 모자라면 성공으로 묻지 않는다). 메인=친절 문구, 자세히=기술 trace로 나눈다
    // (실패만 trace; 부분 실패도 성공/시도 수를 trace로 남긴다)(#199).
    let (status, msg, trace, posted) = match &o.result {
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
            // 글은 올라갔으므로(댓글 부분 실패여도) 밴드가 준 글 URL을 채워 "올라간 글 열기"를
            // 띄운다. 댓글 전용 모드(Commented)는 여러 글 대상이라 단일 URL이 없어 비운다(#219).
            // 글은 올라갔으므로 작성 내용(제목/본문)과 밴드가 준 글 URL을 채워, 빛삭돼도
            // 무엇을 보냈는지 + "올라간 글 열기"가 되게 한다. 댓글은 글+댓글일 때만.
            let posted = Some(PostedContent {
                title: title.to_owned(),
                body: body.to_owned(),
                comment: if out.comment_total > 0 {
                    comment.map(str::to_owned)
                } else {
                    None
                },
                url: Some(out.web_url.clone()),
            });
            (status_of(ok), msg, trace, posted)
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
            // 댓글 전용은 여러 글 대상이라 단일 글 URL이 없어 url은 비우되, 무엇을
            // 보냈는지 보이도록 성공 시 단 댓글 본문은 채운다(#245).
            let posted = ok.then(|| PostedContent {
                comment: comment.map(str::to_owned),
                ..Default::default()
            });
            (status_of(ok), msg, trace, posted)
        }
        Err(e) => (
            BatchItemStatus::Fail,
            band_failure_reason(e),
            Some(band_failure_trace(e)),
            None,
        ),
    };
    BatchItem {
        platform: PlatformId::Band,
        target: o.band_name.clone(),
        code: None,
        board: None,
        login_id: o.account_id.clone(),
        status,
        msg,
        trace,
        posted,
    }
}

/// 밴드 대상을 "대기 중" BatchItem으로(라이브 스켈레톤). 순서는 호출부가 넘긴 대상 목록 =
/// `run_band_targets` 루프 순서와 일치해, 인덱스로 그 자리만 갱신할 수 있다.
fn band_skeleton_items(targets: &[&crate::ipc::queue::BandTarget]) -> Vec<BatchItem> {
    targets
        .iter()
        .map(|t| BatchItem {
            platform: PlatformId::Band,
            target: t.name.clone(),
            code: None,
            board: None,
            login_id: t.account_id.clone(),
            status: BatchItemStatus::Waiting,
            msg: "대기 중".to_owned(),
            trace: None,
            posted: None,
        })
        .collect()
}

/// 블로그 댓글 게시 결과 1건(#271) → BatchItem. 성공이면 댓글 본문·글 URL을 posted에 채워
/// "올라간 글 열기"가 되게 하고, 실패면 메인=친절 사유, 자세히=BlogError trace(backtrace 포함).
fn blog_outcome_to_item(o: &BlogOutcome) -> BatchItem {
    let (status, msg, trace, posted) = match &o.result {
        Ok(_) => (
            BatchItemStatus::Success,
            "댓글 게시 완료".to_owned(),
            None,
            Some(PostedContent {
                comment: Some(o.contents.clone()),
                url: Some(o.link.clone()),
                ..Default::default()
            }),
        ),
        Err(e) => (
            BatchItemStatus::Fail,
            format!("댓글 게시 실패 — {}", e.message()),
            Some(e.trace().to_owned()),
            None,
        ),
    };
    BatchItem {
        platform: PlatformId::Blog,
        target: o.name.clone(),
        code: None,
        board: None,
        login_id: o.account_id.clone(),
        status,
        msg,
        trace,
        posted,
    }
}

/// 펼친 블로그 댓글 작업을 "대기 중" BatchItem으로(라이브 스켈레톤, #279). 순서는 호출부가
/// 넘긴 작업 목록 = `run_blog_targets` 루프 순서와 일치해, 인덱스로 그 자리만 갱신할 수 있다.
fn blog_work_skeleton_items(work: &[BlogWorkItem]) -> Vec<BatchItem> {
    work.iter()
        .map(|w| {
            let (name, account_id) = match w {
                BlogWorkItem::Comment(job) => (&job.name, &job.account_id),
                BlogWorkItem::Shortfall {
                    name, account_id, ..
                }
                | BlogWorkItem::FetchFailure {
                    name, account_id, ..
                } => (name, account_id),
            };
            BatchItem {
                platform: PlatformId::Blog,
                target: name.clone(),
                code: None,
                board: None,
                login_id: account_id.clone(),
                status: BatchItemStatus::Waiting,
                msg: "대기 중".to_owned(),
                trace: None,
                posted: None,
            }
        })
        .collect()
}

// ───────────────────────────── 네이버 클립(#클립) ─────────────────────────────

/// `run_clip_targets`가 펼친 클립 댓글 작업 1건(블로그 BlogWorkItem의 클립 버전).
enum ClipWorkItem {
    /// 실제로 댓글을 달 미디어 1건(최신 N개로 펼친 미디어 1개).
    Comment(ClipCommentJob),
    /// 미디어가 모자라(전체 < N) 댓글을 못 다는 자리 — "영상이 없습니다" 실패로 남긴다.
    Shortfall {
        account_id: String,
        name: String,
        link: String,
    },
    /// 프로필 보장/핸들 해석/목록 조회 자체가 실패한 대상 — 그 오류(backtrace)를 실패로 남긴다.
    FetchFailure {
        account_id: String,
        name: String,
        link: String,
        error: crate::naver_clip::ClipError,
    },
}

/// 댓글을 달 클립 미디어 1건의 동결된 실행 정보. `media_id`=cbox objectId, `profile_id`=창작자.
struct ClipCommentJob {
    account_id: String,
    name: String,
    link: String,
    media_id: String,
    profile_id: String,
}

/// ClipTarget.media_type(Option<String>)을 [`ClipMediaType`]으로 해석한다. "video"=영상만, 그 외=전체.
fn clip_media_type_of(target: &crate::ipc::queue::ClipTarget) -> crate::naver_clip::ClipMediaType {
    match target.media_type.as_deref() {
        Some(v) if v.eq_ignore_ascii_case("video") => crate::naver_clip::ClipMediaType::Video,
        _ => crate::naver_clip::ClipMediaType::All,
    }
}

/// 한 그룹의 클립 댓글 대상을 합성 실패 결과로 만든다(로그인/IP 실패로 댓글조차 못 함).
fn synth_clip_failures(plan: &PublishPlan, account_id: &str, skip: &GroupSkip) -> Vec<ClipOutcome> {
    plan.clip
        .iter()
        .filter(|c| c.account_id == account_id)
        .map(|c| ClipOutcome {
            account_id: c.account_id.clone(),
            name: c.name.clone(),
            link: c.link.clone(),
            contents: String::new(),
            result: Err(crate::naver_clip::ClipError::new(format!(
                "{} — {}",
                failure_reason(&skip.code, None),
                skip.trace_body()
            ))),
        })
        .collect()
}

/// 클립 댓글 게시 결과 1건 → BatchItem(블로그 blog_outcome_to_item의 클립 버전).
fn clip_outcome_to_item(o: &ClipOutcome) -> BatchItem {
    let (status, msg, trace, posted) = match &o.result {
        Ok(_) => (
            BatchItemStatus::Success,
            "댓글 게시 완료".to_owned(),
            None,
            Some(PostedContent {
                comment: Some(o.contents.clone()),
                url: Some(o.link.clone()),
                ..Default::default()
            }),
        ),
        Err(e) => (
            BatchItemStatus::Fail,
            format!("댓글 게시 실패 — {}", e.message()),
            Some(e.trace().to_owned()),
            None,
        ),
    };
    BatchItem {
        platform: PlatformId::Clip,
        target: o.name.clone(),
        code: None,
        board: None,
        login_id: o.account_id.clone(),
        status,
        msg,
        trace,
        posted,
    }
}

/// 펼친 클립 댓글 작업을 "대기 중" BatchItem으로(라이브 스켈레톤). 순서는 run_clip_targets 루프와 일치.
fn clip_work_skeleton_items(work: &[ClipWorkItem]) -> Vec<BatchItem> {
    work.iter()
        .map(|w| {
            let (name, account_id) = match w {
                ClipWorkItem::Comment(job) => (&job.name, &job.account_id),
                ClipWorkItem::Shortfall {
                    name, account_id, ..
                }
                | ClipWorkItem::FetchFailure {
                    name, account_id, ..
                } => (name, account_id),
            };
            BatchItem {
                platform: PlatformId::Clip,
                target: name.clone(),
                code: None,
                board: None,
                login_id: account_id.clone(),
                status: BatchItemStatus::Waiting,
                msg: "대기 중".to_owned(),
                trace: None,
                posted: None,
            }
        })
        .collect()
}

/// 클립 댓글 대상을 실행한다(#클립, 블로그 run_blog_targets 미러). 계정마다 게시 직전 **클립
/// 프로필 생성을 보장**하고, 창작자 핸들→profileId 해석 후 최신 N개 미디어에 댓글을 단다. 도배
/// 방지로 한 댓글 뒤 다음 댓글까지 10초 텀(첫 댓글 즉시, 정지 즉시 반영). 블로그와 동일하게
/// 프로필/해석/조회 실패는 그 대상을 합성 실패로 남기고(조용히 누락하지 않음) 진행한다.
async fn run_clip_targets<R: Runtime>(
    app: &AppHandle<R>,
    plan: &PublishPlan,
    id: &str,
    base_items: Vec<BatchItem>,
    base_done: u32,
    total: u32,
    account_filter: Option<&str>,
) -> Vec<ClipOutcome> {
    let targets: Vec<&crate::ipc::queue::ClipTarget> = plan
        .clip
        .iter()
        .filter(|t| account_filter.is_none_or(|acc| t.account_id == acc))
        .collect();
    // 클립도 종목이 없어 #{링크}만 치환한다. 카페/블로그처럼 비어있지 않은 댓글을 줄바꿈으로 합친다.
    let clip_link = crate::template_tokens::resolve_link(&plan.link_override, "");
    let contents: String = plan
        .comments
        .iter()
        .map(|c| crate::template_tokens::resolve_cafe_band(c, &clip_link))
        .filter(|c| !c.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // 실행 시점에 (프로필 보장 → 핸들 해석 → 최신 N개 조회)로 작업을 펼친다. 어느 단계든 실패하면
    // 그 대상의 want개를 합성 실패로 남긴다(조용히 누락 금지). 프로필 보장은 계정당 1회만 한다.
    let mut work: Vec<ClipWorkItem> = Vec::new();
    let mut profile_ready: std::collections::HashMap<String, Result<(), String>> = Default::default();
    for t in &targets {
        let want = t.count.unwrap_or(1).max(1) as usize;
        let media_type = clip_media_type_of(t);
        let fail_all = |work: &mut Vec<ClipWorkItem>, error: crate::naver_clip::ClipError| {
            // 같은 오류 메시지를 want개에 복제하면 backtrace 캡처가 반복되니, 첫 건만 실오류로 두고
            // 나머지는 같은 사유 문자열로 남긴다(대상이 want개 실패로 보이게).
            let msg = error.message().to_owned();
            work.push(ClipWorkItem::FetchFailure {
                account_id: t.account_id.clone(),
                name: t.name.clone(),
                link: t.link.clone(),
                error,
            });
            for _ in 1..want {
                work.push(ClipWorkItem::FetchFailure {
                    account_id: t.account_id.clone(),
                    name: t.name.clone(),
                    link: t.link.clone(),
                    error: crate::naver_clip::ClipError::new(msg.clone()),
                });
            }
        };

        // (a) 프로필 보장(계정당 1회 캐시).
        let ensured = match profile_ready.get(&t.account_id) {
            Some(r) => r.clone(),
            None => {
                let r = crate::naver_clip::ensure_clip_profile_for_account(&t.account_id)
                    .await
                    .map_err(|e| e.message().to_owned());
                profile_ready.insert(t.account_id.clone(), r.clone());
                r
            }
        };
        if let Err(msg) = ensured {
            fail_all(&mut work, crate::naver_clip::ClipError::new(msg));
            continue;
        }

        // (b) 핸들 → profileId.
        let profile_id = match crate::naver_clip::resolve_clip_profile_id_for_account(
            &t.account_id,
            &t.handle,
        )
        .await
        {
            Ok(pid) => pid,
            Err(e) => {
                fail_all(&mut work, e);
                continue;
            }
        };

        // (c) 최신 N개 미디어 조회 → 미디어마다 댓글 작업 1건.
        match crate::naver_clip::fetch_latest_clips_for_account(
            &t.account_id,
            &profile_id,
            media_type,
            want,
        )
        .await
        {
            Ok(clips) => {
                for media in &clips {
                    work.push(ClipWorkItem::Comment(ClipCommentJob {
                        account_id: t.account_id.clone(),
                        name: t.name.clone(),
                        // 표시용 링크는 실제로 열리는 contents 형식(/shorts/는 "페이지 없음").
                        link: crate::naver_clip::clip_view_url(&t.handle, &media.media_id),
                        media_id: media.media_id.clone(),
                        profile_id: profile_id.clone(),
                    }));
                }
                let shortfall = want.saturating_sub(clips.len());
                for _ in 0..shortfall {
                    work.push(ClipWorkItem::Shortfall {
                        account_id: t.account_id.clone(),
                        name: t.name.clone(),
                        link: t.link.clone(),
                    });
                }
            }
            Err(e) => fail_all(&mut work, e),
        }
    }

    let mut live = clip_work_skeleton_items(&work);
    write_live_phase(app, id, &base_items, &live, base_done, total);
    let mut outcomes = Vec::new();
    // 도배 방지: 한 댓글 뒤 다음 댓글까지 10초 텀(첫 댓글 즉시).
    let mut posted_any = false;
    for (i, w) in work.into_iter().enumerate() {
        let outcome = match w {
            ClipWorkItem::FetchFailure {
                account_id,
                name,
                link,
                error,
            } => ClipOutcome {
                account_id,
                name,
                link,
                contents: String::new(),
                result: Err(error),
            },
            ClipWorkItem::Shortfall {
                account_id,
                name,
                link,
            } => ClipOutcome {
                account_id,
                name,
                link,
                contents: String::new(),
                result: Err(crate::naver_clip::ClipError::new("영상이 없습니다")),
            },
            ClipWorkItem::Comment(job) => {
                if !item_present(app, id) {
                    break;
                }
                if posted_any {
                    for _ in 0..100 {
                        if !item_present(app, id) {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    if !item_present(app, id) {
                        break;
                    }
                }
                if let Some(it) = live.get_mut(i) {
                    it.status = BatchItemStatus::Running;
                    it.msg = "댓글 게시 중…".to_owned();
                }
                write_live_phase(app, id, &base_items, &live, base_done, total);
                let result = crate::naver_clip::create_clip_comment_for_account(
                    &job.account_id,
                    &job.media_id,
                    &job.profile_id,
                    &contents,
                )
                .await;
                posted_any = true;
                ClipOutcome {
                    account_id: job.account_id,
                    name: job.name,
                    link: job.link,
                    contents: contents.clone(),
                    result,
                }
            }
        };
        if let Some(slot) = live.get_mut(i) {
            *slot = clip_outcome_to_item(&outcome);
        }
        write_live_phase(app, id, &base_items, &live, base_done, total);
        outcomes.push(outcome);
    }
    outcomes
}

/// 누적된 플랫폼별 결과를 대상별 BatchItem 목록으로 합친다(라이브 큐 상태·최종 알림 로그
/// 공용). 순서는 글→댓글→조회 실패→종토방→밴드→블로그→클립으로 고정한다.
#[allow(clippy::too_many_arguments)]
fn build_items(
    plan: &PublishPlan,
    post_reports: &[JobReport],
    comment_reports: &[CommentJobReport],
    comment_fetch_failures: &[CommentFetchFailure],
    forum_outcomes: &[ForumOutcome],
    band_outcomes: &[BandOutcome],
    blog_outcomes: &[BlogOutcome],
    clip_outcomes: &[ClipOutcome],
) -> Vec<BatchItem> {
    let mut items = Vec::new();
    items.extend(post_reports.iter().map(|r| post_report_to_item(plan, r)));
    items.extend(
        comment_reports
            .iter()
            .map(|r| comment_report_to_item(plan, r)),
    );
    items.extend(
        comment_fetch_failures
            .iter()
            .map(|f| fetch_failure_to_item(plan, f)),
    );
    items.extend(forum_outcomes.iter().map(forum_outcome_to_item));
    // 밴드 완료 로그에 작성 내용(#{링크} 치환 후)을 싣는다(URL은 엔진 web_url 사용).
    let band_link = crate::template_tokens::resolve_link(&plan.link_override, "");
    let band_title = crate::template_tokens::resolve_cafe_band(&plan.title, &band_link);
    let band_body = crate::template_tokens::resolve_cafe_band(&plan.body_text, &band_link);
    let band_comment = plan
        .comments
        .iter()
        .find(|c| !c.trim().is_empty())
        .map(|c| crate::template_tokens::resolve_cafe_band(c, &band_link));
    items.extend(
        band_outcomes
            .iter()
            .map(|o| band_outcome_to_item(o, &band_title, &band_body, band_comment.as_deref())),
    );
    items.extend(blog_outcomes.iter().map(blog_outcome_to_item));
    items.extend(clip_outcomes.iter().map(clip_outcome_to_item));
    items
}

/// 카페 글 게시 대상을 "처리 중" 상태의 BatchItem으로(펼침 시 진행 중 표시). 실제 결과가
/// 나오면 `post_report_to_item` 결과로 교체된다.
fn running_post_items(plan: &PublishPlan) -> Vec<BatchItem> {
    plan.naver
        .iter()
        .map(|t| BatchItem {
            platform: PlatformId::Naver,
            target: if t.cafe_name.is_empty() {
                t.cafe.clone()
            } else {
                t.cafe_name.clone()
            },
            code: None,
            board: Some(t.menu_id.to_string()),
            login_id: t.account_id.clone(),
            status: BatchItemStatus::Running,
            msg: "글 게시 중…".to_owned(),
            trace: None,
            posted: None,
        })
        .collect()
}

/// base 항목 뒤에 라이브 항목을 이어 붙여 큐 아이템·진행률을 갱신한다(#252). 진행률 done은
/// 호출부가 직접 넘긴다 — 댓글은 1건마다 단계 표시를 갱신하되 done은 완료 건수로 세기에,
/// [`write_live_phase`](완료 칸 수로 done 계산)와 달리 명시적으로 받는다.
fn write_live_items<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    base_items: &[BatchItem],
    live: &[BatchItem],
    done: u32,
    total: u32,
) {
    let mut items = base_items.to_vec();
    items.extend(live.iter().cloned());
    set_progress_and_items(app, id, done, total, items);
}

/// 큐 아이템의 대상별 라이브 상태(items)만 교체한다(진행률은 유지).
fn set_queue_items<R: Runtime>(app: &AppHandle<R>, id: &str, items: Vec<BatchItem>) {
    app.state::<JsonStore<QueueNowItem>>()
        .mutate(|list| apply_queue_items(list, id, items));
}

fn apply_queue_items(
    mut list: Vec<QueueNowItem>,
    id: &str,
    items: Vec<BatchItem>,
) -> Vec<QueueNowItem> {
    if let Some(it) = list.iter_mut().find(|i| i.id == id) {
        it.items = items;
    }
    list
}

/// 진행률 카운트와 대상별 라이브 상태를 한 번에 갱신한다(대상 1건 완료 지점에서 사용).
fn set_progress_and_items<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    done: u32,
    total: u32,
    items: Vec<BatchItem>,
) {
    app.state::<JsonStore<QueueNowItem>>()
        .mutate(|list| apply_progress_and_items(list, id, done, total, items));
}

fn apply_progress_and_items(
    mut list: Vec<QueueNowItem>,
    id: &str,
    done: u32,
    total: u32,
    items: Vec<BatchItem>,
) -> Vec<QueueNowItem> {
    if let Some(it) = list.iter_mut().find(|i| i.id == id) {
        it.progress = Some((done, total));
        it.items = items;
    }
    list
}

/// 뮤텍스가 poison돼도(다른 스레드 패닉) 라이브 상태 갱신은 계속한다 — 진행률·표시 데이터일
/// 뿐이라 오염된 값을 그대로 이어 써도 안전하다.
fn lock_or_poisoned<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// 종토방/밴드 라이브 스켈레톤(`live`)을 base 항목 뒤에 이어 붙여 큐 아이템에 반영한다.
/// 진행률 done은 base_done + (성공/실패로 확정된 칸 수)로 잡아, "진행 중"은 카운트하지 않는다.
fn write_live_phase<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
    base_items: &[BatchItem],
    live: &[BatchItem],
    base_done: u32,
    total: u32,
) {
    let resolved = live
        .iter()
        // Skip(차단으로 건너뜀, #267-9)도 더는 시도하지 않으므로 "확정"으로 세어, 건너뛴 글
        // 때문에 진행률이 100%에 못 미치고 멈춰 보이지 않게 한다.
        .filter(|i| {
            matches!(
                i.status,
                BatchItemStatus::Success | BatchItemStatus::Fail | BatchItemStatus::Skip
            )
        })
        .count() as u32;
    let mut items = base_items.to_vec();
    items.extend(live.iter().cloned());
    set_progress_and_items(app, id, base_done + resolved, total, items);
}

/// 카페 글·댓글·종목토론방·밴드 실행 결과를 알림 배치(`LogBatch`) 한 건으로 묶는다.
/// 대상별 항목(`items`)은 `build_items`로 만들어 라이브 큐 상태와 동일 매핑을 공유한다.
/// 본문/댓글 스냅샷은 실제 그 작업을 돌린 모드일 때만 남긴다(즉시게시 forum 경로와 동일).
#[allow(clippy::too_many_arguments)]
fn build_log_batch(
    plan: &PublishPlan,
    post_reports: &[JobReport],
    comment_reports: &[CommentJobReport],
    forum_outcomes: &[ForumOutcome],
    band_outcomes: &[BandOutcome],
    blog_outcomes: &[BlogOutcome],
    clip_outcomes: &[ClipOutcome],
    comment_fetch_failures: &[CommentFetchFailure],
    at: i64,
    seq: u64,
) -> LogBatch {
    let items = build_items(
        plan,
        post_reports,
        comment_reports,
        comment_fetch_failures,
        forum_outcomes,
        band_outcomes,
        blog_outcomes,
        clip_outcomes,
    );

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
    // 로그인 **전용** 아이템(#210)만 진행률 분모 = 계정 수. 게시 타깃(naver/forum/band)이
    // 동봉돼 있으면(종토방 선택 로그인 등) 분모는 아래 실제 게시 작업 수로 잡는다 — 안 그러면
    // 3계정×3글이 0/9가 아니라 0/3으로 시작하고 done이 분모를 넘어(5/3) 보인다.
    let no_publish_targets = plan.naver.is_empty()
        && plan.forum.is_empty()
        && plan.band.is_empty()
        && plan.blog.is_empty()
        && plan.clip.is_empty();
    if no_publish_targets {
        if let Some(login) = plan.login.as_ref().filter(|l| !l.is_empty()) {
            return login.len() as u32;
        }
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
    // 블로그(#271)는 댓글 전용 — 대상 1건당 댓글 1개로 센다(글당 한 댓글).
    // 클립(#클립)은 "최신 N개" 모드라 대상별 count(없으면 1)의 합으로 센다.
    let clip: usize = plan
        .clip
        .iter()
        .map(|t| t.count.unwrap_or(1).max(1) as usize)
        .sum();
    (posts + comments + plan.forum.len() + plan.band.len() + plan.blog.len() + clip) as u32
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

/// 종목토론방 게시 도중 차단된 아이템을 **종료성 "차단" 카드**로 정착시킨다(#REQ1, 순수). 완료처럼
/// 큐에서 빼지 않고 남기되, 진행률을 N/N으로 채워 "다 돌고 멈춤"을 나타낸다. 종목별 차단/건너뜀
/// 행(`items`)은 set_progress_and_items가 이미 채워 둔 그대로 보존해, 사용자가 알림을 열지 않고도
/// 무엇이 차단됐는지 큐 창에서 본다(#REQ1). 사용자가 X로 닫으면 사라진다.
///
/// **state는 `Running`으로 유지한다(Waiting으로 바꾸지 않음)** — `pick_next_waiting`는 Waiting
/// 아이템만 집어 워커가 **재실행**하는데, 이 아이템엔 아직 전체 plan이 남아 있어 Waiting으로
/// 두면 차단된 글을 통째로 **재게시**해 버린다(치명적). Running은 재픽되지 않아 안전하고, 워커
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
            link_override: String::new(),
            naver,
            forum: vec![],
            band: vec![],
            blog: vec![],
            clip: vec![],
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
        // 사용자 사유가 백트레이스 위에 먼저 붙는다(자세히 보기 전문 노출).
        assert_eq!(
            trace.as_deref(),
            Some("연결 실패\n\nat x.rs:1:1\n\nframe0")
        );
    }

    #[test]
    fn estimate_total_login_only_uses_account_count() {
        // 로그인 **전용** 아이템(게시 타깃 없음)만 분모 = 계정 수.
        let mut p = plan(ModeValue::Post, vec![]);
        p.login = Some(vec![
            login_target("a", PlatformId::Naver),
            login_target("b", PlatformId::Band),
            login_target("c", PlatformId::Naver),
        ]);
        assert_eq!(estimate_total(&p), 3);
    }

    #[test]
    fn estimate_total_counts_publish_work_even_with_login_attached() {
        // 게시 타깃이 동봉되면(종토방 선택 로그인 등) 분모는 실제 게시 작업 수 — 로그인 계정
        // 수가 아니다. 3계정×글이 0/3으로 시작하던 버그 수정: 글 2개면 2(로그인 3개여도).
        let mut p = plan(ModeValue::Post, vec![naver_target("a"), naver_target("b")]);
        p.login = Some(vec![
            login_target("a", PlatformId::Naver),
            login_target("b", PlatformId::Naver),
            login_target("c", PlatformId::Naver),
        ]);
        assert_eq!(estimate_total(&p), 2); // 로그인 3개가 아니라 글 2개

        // 종토방도 동일: 종목 수가 분모(로그인 동봉돼도).
        let mut f = plan(ModeValue::Post, vec![]);
        f.forum = vec![
            forum_target("a", "삼성", "005930"),
            forum_target("a", "현대", "005380"),
            forum_target("b", "네이버", "035420"),
        ];
        f.login = Some(vec![login_target("a", PlatformId::Naver)]);
        assert_eq!(estimate_total(&f), 3); // 종목 3개
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
            items: Vec::new(),
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
                posted: None,
                skipped: false,
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
                posted: None,
                skipped: false,
            },
        }
    }

    // 게시 도중 "차단"(is_blocking_failure가 참인 메시지)으로 실패한 결과(#2). 권한 만료(403)
    // 처럼 같은 계정의 남은 글이 전부 실패할 종료성 실패다.
    fn forum_blocked(account: &str, name: &str, code: &str) -> ForumOutcome {
        ForumOutcome {
            account_id: account.into(),
            result: ForumPublishResult {
                code: code.into(),
                name: name.into(),
                ok: false,
                message: "글쓰기 form 패킷 HTTP 실패: HTTP status 403 Forbidden".into(),
                trace: None,
                posted: None,
                skipped: false,
            },
        }
    }

    // 앞 글이 차단돼 시도하지 않고 건너뛴 결과(#267-9). 그 자체는 차단 사유가 아니다(skipped=true).
    fn forum_skipped(account: &str, name: &str, code: &str) -> ForumOutcome {
        ForumOutcome {
            account_id: account.into(),
            result: ForumPublishResult {
                code: code.into(),
                name: name.into(),
                ok: false,
                message: "앞선 글이 로그인/권한 오류로 실패해 건너뜀".into(),
                trace: None,
                posted: None,
                skipped: true,
            },
        }
    }

    // 페이지 대기시간 초과·네이버 서버 오류(HTTP 500)로 실패한 결과(#7). 차단이 아닌 일시적 실패다.
    fn forum_timed_out(account: &str, name: &str, code: &str, message: &str) -> ForumOutcome {
        ForumOutcome {
            account_id: account.into(),
            result: ForumPublishResult {
                code: code.into(),
                name: name.into(),
                ok: false,
                message: message.into(),
                trace: None,
                posted: None,
                skipped: false,
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

    // --- may_claim: now 큐 동시 작업 한도(#284) ---------------------------------

    #[test]
    fn may_claim_limit_zero_is_unlimited() {
        // 0 = 무제한: active가 아무리 커도 항상 claim 허용.
        assert!(may_claim(0, 0));
        assert!(may_claim(5, 0));
        assert!(may_claim(1000, 0));
    }

    #[test]
    fn may_claim_caps_at_limit() {
        // limit=5: active가 5 미만이면 허용, 5 이상이면 차단.
        assert!(may_claim(0, 5));
        assert!(may_claim(4, 5));
        assert!(!may_claim(5, 5));
        assert!(!may_claim(6, 5));
    }

    #[test]
    fn may_claim_running_tasks_unaffected_by_lowered_limit() {
        // 이미 active=5인데 사용자가 한도를 3으로 낮춰도, may_claim은 claim(신규 시작)만
        // 막는다 — 돌고 있는 5개는 active 카운터에 이미 반영돼 멈추지 않고, 새 claim만 차단된다.
        assert!(!may_claim(5, 3));
        // active가 다시 한도 미만으로 내려오면 새 claim이 재개된다.
        assert!(may_claim(2, 3));
    }

    #[test]
    fn concurrency_config_default_is_unlimited() {
        assert_eq!(ConcurrencyConfig::default().limit, 0);
        assert_eq!(seed_concurrency().first().map(|c| c.limit), Some(0));
    }

    fn login_only_plan() -> PublishPlan {
        let mut p = plan(ModeValue::Post, vec![]);
        p.login = Some(vec![login_target("u", PlatformId::Naver)]);
        p
    }

    fn forum_plan() -> PublishPlan {
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![forum_target("u", "삼성전자", "005930")];
        p
    }

    #[test]
    fn pick_next_waiting_prefers_login_then_forum_then_fifo() {
        let cafe = plan(ModeValue::Post, vec![naver_target("u")]);
        // 들어온 순서: 카페 → 종토 → 로그인. 픽은 로그인(1순위)부터.
        let items = vec![
            now_item("c1", QueueState::Waiting, Some(cafe.clone())),
            now_item("f1", QueueState::Waiting, Some(forum_plan())),
            now_item("l1", QueueState::Waiting, Some(login_only_plan())),
        ];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "l1");
        // 로그인이 빠지면 종토(2순위).
        let items = vec![
            now_item("c1", QueueState::Waiting, Some(cafe.clone())),
            now_item("f1", QueueState::Waiting, Some(forum_plan())),
        ];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "f1");
        // 종토까지 빠지면 카페(FIFO).
        let items = vec![now_item("c1", QueueState::Waiting, Some(cafe))];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "c1");
    }

    #[test]
    fn pick_next_waiting_same_priority_is_fifo() {
        // 같은 등급(카페)끼리는 먼저 들어온 것을 집는다(안정 선택).
        let cafe = plan(ModeValue::Post, vec![naver_target("u")]);
        let items = vec![
            now_item("c1", QueueState::Waiting, Some(cafe.clone())),
            now_item("c2", QueueState::Waiting, Some(cafe)),
        ];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "c1");
    }

    #[test]
    fn pick_next_waiting_skips_running_and_picks_by_priority() {
        // Running은 건너뛰고, 대기 중에서 우선순위 높은 종토를 카페보다 먼저 집는다.
        let cafe = plan(ModeValue::Post, vec![naver_target("u")]);
        let items = vec![
            now_item("r1", QueueState::Running, Some(cafe.clone())),
            now_item("c1", QueueState::Waiting, Some(cafe)),
            now_item("f1", QueueState::Waiting, Some(forum_plan())),
        ];
        assert_eq!(pick_next_waiting(&items).unwrap().id, "f1");
    }

    // --- 종토방 아이템 동시 실행 분류(#240) ---

    #[test]
    fn is_url_comment_only_item_true_for_url_comment_targets() {
        use crate::ipc::queue::{CommentTargetSpec, ForumTarget};
        let url_spec = || {
            Some(CommentTargetSpec {
                mode: CommentTarget::Url,
                count: None,
                cafe_id: Some(123),
                article_id: Some(9),
            })
        };
        // 카페 url 댓글만 → 동시 실행 경로(true).
        let mut cafe = plan(ModeValue::Comment, vec![naver_target("u")]);
        cafe.naver[0].comment_target = url_spec();
        assert!(is_url_comment_only_item(&now_item(
            "c",
            QueueState::Waiting,
            Some(cafe)
        )));

        // 종토방 url 댓글만(comment_url 채움) → true.
        let mut forum = plan(ModeValue::Comment, vec![]);
        forum.forum = vec![ForumTarget {
            account_id: "u".into(),
            name: "글 #1".into(),
            code: "005930".into(),
            comment_url: "https://stock.naver.com/domestic/stock/005930/discussion/1".into(),
        }];
        assert!(is_url_comment_only_item(&now_item(
            "f",
            QueueState::Waiting,
            Some(forum)
        )));

        // 밴드 url 댓글만 → true. 카페·종토방·밴드 url이 섞여도 모두 url이면 true.
        let mut band = plan(ModeValue::Comment, vec![]);
        band.band = vec![BandTarget {
            account_id: "u".into(),
            name: "밴드 글 #2".into(),
            link: "https://www.band.us/band/1/post/2".into(),
            comment_target: Some(CommentTargetSpec {
                mode: CommentTarget::Url,
                count: None,
                cafe_id: None,
                article_id: None,
            }),
        }];
        assert!(is_url_comment_only_item(&now_item(
            "b",
            QueueState::Waiting,
            Some(band)
        )));

        // 최신글 댓글(mode=Latest)은 url 전용이 아니다 → false.
        let mut latest = plan(ModeValue::Comment, vec![naver_target("u")]);
        latest.naver[0].comment_target = Some(CommentTargetSpec {
            mode: CommentTarget::Latest,
            count: Some(3),
            cafe_id: Some(123),
            article_id: None,
        });
        assert!(!is_url_comment_only_item(&now_item(
            "l",
            QueueState::Waiting,
            Some(latest)
        )));

        // 글 게시(Post) 모드, comment_url 없는 forum, plan 없음 → 모두 false.
        let mut post = plan(ModeValue::Post, vec![naver_target("u")]);
        post.naver[0].comment_target = url_spec(); // 모드가 Comment가 아니면 무조건 false
        assert!(!is_url_comment_only_item(&now_item(
            "p",
            QueueState::Waiting,
            Some(post)
        )));
        assert!(!is_url_comment_only_item(&now_item(
            "rf",
            QueueState::Waiting,
            Some(forum_plan()) // 종토방 새 글(comment_url 없음)
        )));
        assert!(!is_url_comment_only_item(&now_item(
            "x",
            QueueState::Waiting,
            None
        )));
    }

    #[test]
    fn is_forum_only_item_true_only_for_forum_targets() {
        // 종토방만 있는 아이템 → 동시 실행 경로(true).
        assert!(is_forum_only_item(&now_item(
            "f",
            QueueState::Waiting,
            Some(forum_plan())
        )));

        // 카페가 섞이면 9222 공유라 false → 순차 경로.
        let mut cafe_and_forum = plan(ModeValue::Post, vec![naver_target("u")]);
        cafe_and_forum.forum = forum_plan().forum;
        assert!(!is_forum_only_item(&now_item(
            "m",
            QueueState::Waiting,
            Some(cafe_and_forum)
        )));

        // 카페만, 밴드만, 로그인 전용, plan 없음 → 모두 false.
        assert!(!is_forum_only_item(&now_item(
            "c",
            QueueState::Waiting,
            Some(plan(ModeValue::Post, vec![naver_target("u")]))
        )));
        let mut band = plan(ModeValue::Post, vec![]);
        band.band = vec![band_target("u", "밴드", "https://band.us/band/1")];
        assert!(!is_forum_only_item(&now_item(
            "b",
            QueueState::Waiting,
            Some(band)
        )));
        assert!(!is_forum_only_item(&now_item(
            "l",
            QueueState::Waiting,
            Some(login_only_plan())
        )));
        assert!(!is_forum_only_item(&now_item(
            "x",
            QueueState::Waiting,
            None
        )));
    }

    // --- 우선순위 선점 중지/재개(#232) ---

    #[test]
    fn should_yield_when_higher_priority_waiting_exists() {
        let cafe = plan(ModeValue::Post, vec![naver_target("u")]);
        // 실행 중 카페(2)는 대기 종토(1)·로그인(0)에 양보한다.
        assert!(should_yield_now(
            &[
                now_item("c1", QueueState::Running, Some(cafe.clone())),
                now_item("f1", QueueState::Waiting, Some(forum_plan())),
            ],
            "c1"
        ));
        assert!(should_yield_now(
            &[
                now_item("c1", QueueState::Running, Some(cafe.clone())),
                now_item("l1", QueueState::Waiting, Some(login_only_plan())),
            ],
            "c1"
        ));
        // 실행 중 종토(1)는 로그인(0)에만 양보한다.
        assert!(should_yield_now(
            &[
                now_item("f1", QueueState::Running, Some(forum_plan())),
                now_item("l1", QueueState::Waiting, Some(login_only_plan())),
            ],
            "f1"
        ));
    }

    #[test]
    fn should_not_yield_to_same_or_lower_priority_or_when_idle() {
        let cafe = plan(ModeValue::Post, vec![naver_target("u")]);
        // 동순위(카페↔카페)는 선점 안 함 = FIFO 유지.
        assert!(!should_yield_now(
            &[
                now_item("c1", QueueState::Running, Some(cafe.clone())),
                now_item("c2", QueueState::Waiting, Some(cafe.clone())),
            ],
            "c1"
        ));
        // 실행 중 종토(1)는 동순위 종토(1)·더 낮은 카페(2)에 양보 안 함(계속 진행).
        assert!(!should_yield_now(
            &[
                now_item("f1", QueueState::Running, Some(forum_plan())),
                now_item("f2", QueueState::Waiting, Some(forum_plan())),
                now_item("c1", QueueState::Waiting, Some(cafe.clone())),
            ],
            "f1"
        ));
        // 대기 아이템이 없거나 running_id가 큐에 없으면 false.
        let only = [now_item("c1", QueueState::Running, Some(cafe))];
        assert!(!should_yield_now(&only, "c1"));
        assert!(!should_yield_now(&only, "missing"));
    }

    #[test]
    fn retain_plan_accounts_splits_without_loss_or_duplication() {
        // 2계정 plan: 카페+종토(계정 a), 밴드(계정 b).
        let mut p = plan(ModeValue::Post, vec![naver_target("a")]);
        p.forum = vec![forum_target("a", "삼성전자", "005930")];
        p.band = vec![band_target("b", "밴드B", "https://band.us/band/1")];
        p.login = Some(vec![
            login_target("a", PlatformId::Naver),
            login_target("b", PlatformId::Band),
        ]);

        let a: std::collections::BTreeSet<String> = ["a".to_owned()].into_iter().collect();
        let b: std::collections::BTreeSet<String> = ["b".to_owned()].into_iter().collect();
        let empty = std::collections::BTreeSet::new();

        // 완료분 = 계정 a(naver), 잔여분 = 계정 b(band).
        let done = retain_plan_accounts(&p, &a, &empty);
        let rest = retain_plan_accounts(&p, &empty, &b);

        assert_eq!(done.naver.len(), 1);
        assert_eq!(done.forum.len(), 1);
        assert!(done.band.is_empty());
        assert_eq!(done.login.as_deref().unwrap().len(), 1); // a/Naver 로그인만
        assert!(rest.naver.is_empty());
        assert!(rest.forum.is_empty());
        assert_eq!(rest.band.len(), 1);
        assert_eq!(rest.login.as_deref().unwrap().len(), 1); // b/Band 로그인만

        // 서로소 분할 → 합집합이 원본과 동일(누락·중복 0).
        assert_eq!(done.naver.len() + rest.naver.len(), p.naver.len());
        assert_eq!(done.forum.len() + rest.forum.len(), p.forum.len());
        assert_eq!(done.band.len() + rest.band.len(), p.band.len());
        // 스칼라 필드는 보존.
        assert_eq!(rest.title, p.title);
        assert_eq!(rest.body_text, p.body_text);
    }

    #[test]
    fn forum_unattempted_accounts_keeps_only_zero_outcome_accounts() {
        // 종토 게시 대상 3계정. a=성공·b=대기초과(시도함)는 outcome가 있고, c는 outcome 0건
        // (선점 양보/취소로 시도조차 못 함) → c만 미시도로 잡힌다.
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![
            forum_target("a", "삼성", "1"),
            forum_target("b", "엘지", "2"),
            forum_target("c", "현대", "3"),
        ];
        let all_forum = vec![
            forum_ok("a", "삼성", "1"),
            forum_timed_out("b", "엘지", "2", "페이지 로드 대기 시간이 초과되었습니다."),
        ];
        let missing = forum_unattempted_accounts(&p, &all_forum);
        assert_eq!(missing.into_iter().collect::<Vec<_>>(), vec!["c".to_owned()]);
    }

    #[test]
    fn forum_unattempted_empty_when_every_account_attempted() {
        // 시도 후 실패(outcome 있음)는 미시도가 아니다 → 빈 집합 → execute_item이 Completed로 뺀다
        // (무한 재대기 방지). 차단·건너뜀도 outcome가 있어 마찬가지.
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![forum_target("a", "삼성", "1"), forum_target("b", "엘지", "2")];
        let all_forum = vec![
            forum_fail("a", "삼성", "1", "trace"),
            forum_blocked("b", "엘지", "2"),
        ];
        assert!(forum_unattempted_accounts(&p, &all_forum).is_empty());
    }

    #[test]
    fn retain_forum_only_keeps_just_those_forum_targets_and_clears_other_lanes() {
        // 카페(a) + 종토(a,b) + 로그인이 섞인 plan에서 b의 종토만 남긴다 — 카페·login은 비워져
        // 재대기 시 카페 중복게시·재로그인이 없다(사수 지침: 카페·밴드 무손상).
        let mut p = plan(ModeValue::Post, vec![naver_target("a")]);
        p.forum = vec![forum_target("a", "삼성", "1"), forum_target("b", "엘지", "2")];
        p.login = Some(vec![login_target("a", PlatformId::Naver)]);
        let keep: std::collections::BTreeSet<String> = ["b".to_owned()].into_iter().collect();

        let sub = retain_forum_only(&p, &keep);
        assert!(sub.naver.is_empty(), "카페 비움 → 중복게시 없음");
        assert!(sub.band.is_empty());
        assert!(sub.blog.is_empty());
        assert!(sub.clip.is_empty());
        assert_eq!(sub.login, None, "login 비움 → 재로그인·IP회전 없음(저장 쿠키 게시)");
        assert_eq!(sub.forum.len(), 1);
        assert_eq!(sub.forum[0].account_id, "b");
        // 스칼라(제목/본문/댓글/링크)는 종토 게시에 그대로 쓰므로 보존.
        assert_eq!(sub.title, p.title);
        assert_eq!(sub.body_text, p.body_text);
        assert_eq!(sub.comments, p.comments);
    }

    #[test]
    fn retain_forum_only_item_stays_forum_priority_and_concurrent_lane() {
        // 재대기된 forum-only 아이템은 종토(우선순위 1)라 대기 중 로그인(0) 뒤에서 제 차례를
        // 기다리고, is_forum_only_item=true라 동시 종토 레인으로 흐른다.
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![forum_target("a", "삼성", "1")];
        p.login = Some(vec![login_target("a", PlatformId::Naver)]);
        let keep: std::collections::BTreeSet<String> = ["a".to_owned()].into_iter().collect();
        let item = now_item("re", QueueState::Waiting, Some(retain_forum_only(&p, &keep)));
        assert_eq!(item_priority(&item), 1, "종토 우선순위 유지");
        assert!(is_forum_only_item(&item), "동시 종토 레인으로 처리");
    }

    #[test]
    fn account_sets_partitions_groups_by_family() {
        let mut p = plan(ModeValue::Post, vec![naver_target("a")]);
        p.band = vec![band_target("b", "밴드B", "https://band.us/band/1")];
        let groups = group_accounts_for_publish(&p);
        let (naver, band) = account_sets(&groups);
        assert!(naver.contains("a"));
        assert!(band.contains("b"));
        assert!(!naver.contains("b"));
        assert!(!band.contains("a"));
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

    // #2: 도중 차단(blocking failure)이 하나라도 있으면 그 계정은 차단으로 본다 —
    // 성공·일반 실패·건너뜀만이면 차단이 아니다. execute_item이 계정 상태를 Blocked로
    // 바꾸는(apply_waiting_for_successful_posts) 판정과 동일한 함수를 검증한다.
    #[test]
    fn blocked_flag_true_only_when_a_blocking_failure_present() {
        // 차단 1건 → true.
        assert!(!blocked_post_login_ids(&[forum_blocked("acc", "삼성전자", "005930")]).is_empty());
        // 성공·일반 엔진 실패·건너뜀만 → false(차단 아님).
        let non_blocked = vec![
            forum_ok("acc", "삼성전자", "005930"),
            forum_fail("acc", "현대차", "005380", "ENGINE"),
            forum_skipped("acc", "SK하이닉스", "000660"),
        ];
        assert!(blocked_post_login_ids(&non_blocked).is_empty());
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
            comment_url: String::new(),
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
    fn ip_matches_same_diff_and_fail_open() {
        // 동일이면 true, 다르면 false.
        assert!(ip_matches("1.2.3.4", "1.2.3.4"));
        assert!(!ip_matches("1.2.3.4", "5.6.7.8"));
        // 한쪽이 "(확인 실패)"로 시작하면 fail-open(true) — IP를 못 읽었다고 게시를 막지 않는다.
        assert!(ip_matches("(확인 실패)", "5.6.7.8"));
        assert!(ip_matches("1.2.3.4", "(확인 실패)"));
        assert!(ip_matches("(확인 실패)", "(확인 실패)"));
    }

    fn forum_target(account: &str, name: &str, code: &str) -> crate::ipc::queue::ForumTarget {
        crate::ipc::queue::ForumTarget {
            account_id: account.into(),
            name: name.into(),
            code: code.into(),
            comment_url: String::new(),
        }
    }

    #[test]
    fn group_cafe_and_forum_same_account_into_one_naver_group() {
        // 같은 계정의 카페 + 종토방은 같은 네이버 쿠키를 공유하므로 한 그룹이다.
        let mut p = plan(ModeValue::Post, vec![naver_target("u0")]);
        p.forum = vec![forum_target("u0", "삼성전자", "005930")];
        let groups = group_accounts_for_publish(&p);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].account_id, "u0");
        assert_eq!(groups[0].family, AccountFamily::Naver);
        assert!(groups[0].login.is_none());
    }

    #[test]
    fn forum_only_group_skips_relogin_but_cafe_group_keeps_it() {
        // 종토방 전용 계정(u0)은 재로그인 스킵(저장 쿠키 사용), 카페가 섞인 계정(u1)은 유지.
        let mut p = plan(ModeValue::Post, vec![naver_target("u1")]); // u1 = 카페
        p.forum = vec![
            forum_target("u0", "삼성", "005930"), // u0 = 종토방 전용
            forum_target("u1", "현대", "005380"), // u1 = 카페 + 종토방
        ];
        let groups = group_accounts_for_publish(&p);
        for g in &groups {
            match g.account_id.as_str() {
                "u0" => assert!(forum_only_group(&p, g), "종토방 전용은 재로그인 스킵"),
                "u1" => assert!(!forum_only_group(&p, g), "카페 섞이면 재로그인 유지(10004)"),
                other => panic!("예상치 못한 계정 {other}"),
            }
        }
    }

    #[test]
    fn forum_only_group_false_for_band_group() {
        // 밴드 그룹은 항상 false(별도 쿠키·경로) — 재로그인 유지.
        let mut p = plan(ModeValue::Post, vec![]);
        p.band = vec![band_target("u0", "밴드", "https://band.us/band/1")];
        let groups = group_accounts_for_publish(&p);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].family, AccountFamily::Band);
        assert!(!forum_only_group(&p, &groups[0]));
    }

    #[test]
    fn group_naver_and_band_same_account_into_two_groups() {
        // 같은 account_id라도 네이버와 밴드는 쿠키가 달라 별도 그룹(family로 분리)이다.
        let mut p = plan(ModeValue::Post, vec![naver_target("u0")]);
        p.band = vec![band_target("u0", "투자밴드", "https://band.us/band/1")];
        let groups = group_accounts_for_publish(&p);
        assert_eq!(groups.len(), 2);
        assert!(groups
            .iter()
            .any(|g| g.account_id == "u0" && g.family == AccountFamily::Naver));
        assert!(groups
            .iter()
            .any(|g| g.account_id == "u0" && g.family == AccountFamily::Band));
    }

    #[test]
    fn group_without_login_yields_login_none() {
        // login이 없는(즉시/예약) 게시는 login=None 그룹 — 회전·로그인·IP검증 스킵 경로.
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        let groups = group_accounts_for_publish(&p);
        assert_eq!(groups.len(), 1);
        assert!(groups[0].login.is_none());
    }

    #[test]
    fn group_login_attached_and_ordered_by_login() {
        // login 순서가 그룹 순서를 결정한다. login 대상은 그룹에 LoginTarget이 붙는다.
        let mut p = plan(ModeValue::Post, vec![naver_target("a"), naver_target("b")]);
        p.band = vec![band_target("a", "밴드A", "https://band.us/band/1")];
        // login을 b(naver) → a(band) → a(naver) 순서로 둔다.
        p.login = Some(vec![
            login_target("b", PlatformId::Naver),
            login_target("a", PlatformId::Band),
            login_target("a", PlatformId::Naver),
        ]);
        let groups = group_accounts_for_publish(&p);
        // 게시 타깃이 있는 3개 그룹이 login 순서대로.
        assert_eq!(groups.len(), 3);
        assert_eq!(
            (groups[0].account_id.as_str(), groups[0].family),
            ("b", AccountFamily::Naver)
        );
        assert_eq!(
            (groups[1].account_id.as_str(), groups[1].family),
            ("a", AccountFamily::Band)
        );
        assert_eq!(
            (groups[2].account_id.as_str(), groups[2].family),
            ("a", AccountFamily::Naver)
        );
        assert!(groups.iter().all(|g| g.login.is_some()));
    }

    #[test]
    fn group_login_only_account_makes_no_publish_group() {
        // login만 있고 그 계정의 게시 타깃이 없으면 게시 그룹을 만들지 않는다(로그인 전용은 별 경로).
        let p = {
            let mut p = plan(ModeValue::Post, vec![]); // 게시 타깃 없음
            p.login = Some(vec![login_target("u0", PlatformId::Naver)]);
            p
        };
        assert!(group_accounts_for_publish(&p).is_empty());
    }

    #[test]
    fn group_total_matches_estimate_total() {
        // 그룹 합산 total(글+종토방+밴드)이 기존 estimate_total과 일치한다(회귀 가드, post 모드).
        let mut p = plan(
            ModeValue::Post,
            vec![naver_target("u0"), naver_target("u1")],
        );
        p.forum = vec![
            forum_target("u0", "삼성전자", "005930"),
            forum_target("u1", "카카오", "035720"),
        ];
        p.band = vec![band_target("u2", "밴드", "https://band.us/band/1")];
        // post 모드라 댓글은 0. 그룹별 글+종토방+밴드 합 = naver 2 + forum 2 + band 1 = 5.
        let group_sum: usize = group_accounts_for_publish(&p)
            .iter()
            .map(|g| match g.family {
                AccountFamily::Naver => {
                    p.naver
                        .iter()
                        .filter(|t| t.account_id == g.account_id)
                        .count()
                        + p.forum
                            .iter()
                            .filter(|f| f.account_id == g.account_id)
                            .count()
                }
                AccountFamily::Band => p
                    .band
                    .iter()
                    .filter(|b| b.account_id == g.account_id)
                    .count(),
            })
            .sum();
        assert_eq!(group_sum as u32, estimate_total(&p));
    }

    #[test]
    fn set_progress_value_writes_done_and_total() {
        let next = set_progress_value(vec![now_item("a", QueueState::Running, None)], "a", 2, 3);
        assert_eq!(next[0].progress, Some((2, 3)));
    }

    fn batch_item(target: &str, status: BatchItemStatus) -> BatchItem {
        BatchItem {
            platform: PlatformId::Naver,
            target: target.into(),
            code: None,
            board: None,
            login_id: "u0".into(),
            status,
            msg: "m".into(),
            trace: None,
            posted: None,
        }
    }

    #[test]
    fn apply_queue_items_sets_items_on_matching_id_only() {
        let items = vec![
            now_item("a", QueueState::Running, None),
            now_item("b", QueueState::Waiting, None),
        ];
        let next = apply_queue_items(
            items,
            "a",
            vec![batch_item("카페", BatchItemStatus::Running)],
        );
        assert_eq!(next[0].items.len(), 1);
        assert_eq!(next[0].items[0].status, BatchItemStatus::Running);
        assert!(next[1].items.is_empty(), "다른 아이템은 건드리지 않는다");
    }

    #[test]
    fn apply_progress_and_items_sets_progress_and_items_together() {
        let items = vec![now_item("a", QueueState::Running, None)];
        let next = apply_progress_and_items(
            items,
            "a",
            1,
            2,
            vec![batch_item("삼성전자", BatchItemStatus::Success)],
        );
        assert_eq!(next[0].progress, Some((1, 2)));
        assert_eq!(next[0].items.len(), 1);
        assert_eq!(next[0].items[0].target, "삼성전자");
    }

    #[test]
    fn running_post_items_marks_each_naver_target_running() {
        let p = plan(
            ModeValue::Post,
            vec![naver_target("u0"), naver_target("u1")],
        );
        let items = running_post_items(&p);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.status == BatchItemStatus::Running));
        // cafe_name(동결 표시 이름)을 대상명으로 쓰고, board=menu_id.
        assert_eq!(items[0].target, "테스트카페");
        assert_eq!(items[0].board.as_deref(), Some("7"));
        assert_eq!(items[0].login_id, "u0");
    }

    #[test]
    fn build_items_maps_post_report_with_cafe_name_label() {
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        let items = build_items(&p, &[post_report("u0", 123, 456)], &[], &[], &[], &[], &[], &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, BatchItemStatus::Success);
        assert_eq!(items[0].target, "테스트카페");
        assert_eq!(items[0].msg, "글 게시 완료");
    }

    #[test]
    fn login_item_maps_platform_account_and_status() {
        let t = LoginTarget {
            account_id: "user01".into(),
            platform: PlatformId::Band,
            headless: false,
            use_adb: true,
            force: true,
        };
        let it = login_item(
            &t,
            BatchItemStatus::Success,
            "로그인 성공".into(),
            Some("tr".into()),
        );
        assert_eq!(it.platform, PlatformId::Band);
        assert_eq!(it.target, "user01");
        assert_eq!(it.login_id, "user01");
        assert_eq!(it.status, BatchItemStatus::Success);
        assert_eq!(it.trace.as_deref(), Some("tr"));
    }

    #[test]
    fn forum_skeleton_items_lists_every_stock_as_waiting() {
        use crate::ipc::queue::ForumTarget;
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![
            ForumTarget {
                account_id: "u0".into(),
                name: "삼성전자".into(),
                code: "005930".into(),
                comment_url: String::new(),
            },
            ForumTarget {
                account_id: "u0".into(),
                name: "카카오".into(),
                code: "035720".into(),
                comment_url: String::new(),
            },
        ];
        let items = forum_skeleton_items(&plan_to_forum_requests(&p));
        // 완료된 것만이 아니라 모든 종목이 "대기 중"으로 깔린다(#219).
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.status == BatchItemStatus::Waiting));
        assert_eq!(items[0].target, "삼성전자");
        assert_eq!(items[0].code.as_deref(), Some("005930"));
        assert_eq!(items[0].login_id, "u0");
    }

    #[test]
    fn band_skeleton_items_lists_every_band_as_waiting() {
        let mut p = plan(ModeValue::Post, vec![]);
        p.band = vec![
            band_target("u0", "투자밴드", "https://band.us/band/1"),
            band_target("u1", "정보밴드", "https://band.us/band/2"),
        ];
        let targets: Vec<&BandTarget> = p.band.iter().collect();
        let items = band_skeleton_items(&targets);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.status == BatchItemStatus::Waiting));
        assert_eq!(items[0].target, "투자밴드");
        assert_eq!(items[1].login_id, "u1");
    }

    #[tokio::test]
    async fn collect_targets_both_comments_on_just_posted_articles() {
        let p = plan(ModeValue::Both, vec![naver_target("u0")]);
        let reports = vec![post_report("u0", 111, 222), post_report("u0", 111, 333)];
        let targets = collect_comment_targets(&p, &reports, None).await.targets;
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
        let targets = collect_comment_targets(&p, &[], None).await.targets;
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
                comment_url: String::new(),
            },
            ForumTarget {
                account_id: "u0".into(),
                name: "SK하이닉스".into(),
                code: "000660".into(),
                comment_url: String::new(),
            },
            ForumTarget {
                account_id: "u1".into(),
                name: "에코프로".into(),
                code: "086520".into(),
                comment_url: String::new(),
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
    fn forum_url_comment_target_makes_a_single_url_request_skipping_stock_grouping() {
        use crate::ipc::queue::ForumTarget;
        // "특정 게시글" 댓글: comment_url이 채워진 forum 대상은 종목 묶음이 아니라 그 글 URL
        // 하나에만 댓글을 다는 요청으로 풀려야 한다(댓글 전용, run_post=false). code(035720)는
        // URL에서 온 값으로 토큰 치환·라벨에 쓰인다(랜덤 글/종목 선택 없음).
        let url = "https://stock.naver.com/domestic/stock/035720/discussion/421063210?chip=all";
        let mut p = plan(ModeValue::Comment, vec![]);
        p.comments = vec!["좋은 글이네요".into()];
        p.forum = vec![ForumTarget {
            account_id: "u0".into(),
            name: "종목토론방 글 #421063210".into(),
            code: "035720".into(),
            comment_url: url.into(),
        }];
        let reqs = plan_to_forum_requests(&p);
        assert_eq!(reqs.len(), 1);
        let r = &reqs[0];
        assert_eq!(r.account_id, "u0");
        assert_eq!(r.comment_url.as_deref(), Some(url)); // 그 글 URL로 직접 댓글
        assert!(r.run_comment);
        assert!(!r.run_post); // 특정 글 댓글은 댓글 전용으로 강제
        assert_eq!(r.stocks.len(), 1);
        assert_eq!(r.stocks[0].code, "035720"); // URL의 종목코드
        assert_eq!(r.comment, "좋은 글이네요");
    }

    #[test]
    fn forum_plain_targets_keep_empty_comment_url_after_split() {
        use crate::ipc::queue::ForumTarget;
        // comment_url이 빈 일반 forum 대상은 기존처럼 계정별 per-종목 요청으로 묶이고,
        // comment_url은 None이어야 한다(랜덤/per-종목 동작 무변경).
        let mut p = plan(ModeValue::Post, vec![]);
        p.forum = vec![ForumTarget {
            account_id: "u0".into(),
            name: "삼성전자".into(),
            code: "005930".into(),
            comment_url: String::new(),
        }];
        let reqs = plan_to_forum_requests(&p);
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].comment_url.is_none());
        assert_eq!(reqs[0].stocks.len(), 1);
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
        let b = build_log_batch(&p, &reports, &[], &[], &[], &[], &[], &[], 1_700_000_000_000, 0);
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
    fn failure_reason_maps_ip_check_failure_10004_to_relogin_hint() {
        // 10004(IP check failure): 원문이 한국어를 포함해도(인증 실패) 혼란스러우므로 재로그인
        // 안내로 덮어쓴다. 원문 loginStat 문자열은 메인 라인에 노출하지 않는다.
        let cafe = cafe_error(
            Some(500),
            Some("10004"),
            Some("NaverUser 인증 실패 - loginStat : 'Failure-[401:IP check failure]'"),
        );
        let reason = failure_reason("REGISTER_HTTP_ERROR", Some(&cafe));
        assert_eq!(
            reason,
            "로그인한 IP와 게시 IP가 달라 인증에 실패했습니다. 해당 계정을 다시 로그인해 주세요 (10004)"
        );
        assert!(!reason.contains("loginStat"));
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
    fn failure_reason_maps_ip_mismatch_and_login_failed() {
        // #10004 예방: IP_MISMATCH/LOGIN_FAILED 내부 코드를 행동 가능한 한국어로 치환한다.
        assert_eq!(
            failure_reason("IP_MISMATCH", None),
            "로그인 IP와 게시 IP가 달라 게시를 건너뛰었습니다(IP가 계속 변동). 잠시 후 다시 시도해 주세요 (IP_MISMATCH)"
        );
        assert_eq!(
            failure_reason("LOGIN_FAILED", None),
            "게시 전 로그인에 실패해 이 계정의 게시를 건너뛰었습니다 (LOGIN_FAILED)"
        );
    }

    #[test]
    fn synth_failures_mark_group_targets_as_fail_with_reason() {
        // IP 불일치/로그인 실패로 게시조차 못 한 그룹의 카페·종토방·밴드 타깃을 합성 실패로 남긴다.
        let mut p = plan(
            ModeValue::Post,
            vec![naver_target("u0"), naver_target("u1")],
        );
        p.forum = vec![forum_target("u0", "삼성전자", "005930")];
        p.band = vec![band_target("u0", "밴드", "https://band.us/band/1")];
        let skip = GroupSkip {
            code: "IP_MISMATCH".into(),
            message: "재시도 후에도 불일치".into(),
            trace: None,
        };
        // 카페: u0 대상만 합성 실패(u1 제외).
        let posts = synth_post_failures(&p, "u0", &skip);
        assert_eq!(posts.len(), 1);
        assert!(!posts[0].success);
        assert_eq!(posts[0].error.as_ref().unwrap().code, "IP_MISMATCH");
        // 종토방·밴드도 u0 대상만.
        assert_eq!(synth_forum_failures(&p, "u0", &skip).len(), 1);
        assert_eq!(synth_band_failures(&p, "u0", &skip).len(), 1);
        // build_items가 한국어 사유로 렌더링하는지 확인.
        let items = build_items(
            &p,
            &posts,
            &[],
            &[],
            &synth_forum_failures(&p, "u0", &skip),
            &synth_band_failures(&p, "u0", &skip),
            &[],
            &[],
        );
        assert!(items[0].msg.contains("IP가 계속 변동"));
        assert_eq!(items[0].status, BatchItemStatus::Fail);
    }

    #[test]
    fn synth_failures_surface_login_cdp_backtrace_in_trace() {
        // CDP/자동화 오류로 게시 전 로그인이 죽으면 캡처된 backtrace가 카페·종토방·밴드의
        // "자세히 보기" trace에 모두 실린다(비번오류는 trace None이라 안 실림 — 그건 별개).
        let mut p = plan(ModeValue::Post, vec![naver_target("u0")]);
        p.forum = vec![forum_target("u0", "삼성전자", "005930")];
        p.band = vec![band_target("u0", "밴드", "https://band.us/band/1")];
        let skip = GroupSkip {
            code: "LOGIN_FAILED".into(),
            message: "로그인 중 오류가 발생했습니다".into(),
            trace: Some("at login_flow.rs:12:3\n\nframe0: cdp_disconnect".into()),
        };
        let posts = synth_post_failures(&p, "u0", &skip);
        let forum = synth_forum_failures(&p, "u0", &skip);
        let band = synth_band_failures(&p, "u0", &skip);
        let items = build_items(&p, &posts, &[], &[], &forum, &band, &[], &[]);
        assert_eq!(items.len(), 3, "카페·종토방·밴드 3개 항목");
        for it in &items {
            let tr = it.trace.as_deref().unwrap_or_default();
            assert!(
                tr.contains("frame0: cdp_disconnect"),
                "{:?} trace에 backtrace가 없음: {tr}",
                it.platform
            );
            // 메인 사유 라인엔 backtrace가 새지 않는다(자세히 보기에만).
            assert!(
                !it.msg.contains("frame0"),
                "{:?} msg에 backtrace 누출",
                it.platform
            );
        }
    }

    #[test]
    fn synth_failures_without_trace_omit_backtrace() {
        // 비번오류·IP 불일치(trace None)는 trace 본문에 backtrace 없이 사유 메시지만 남는다.
        let p = plan(ModeValue::Post, vec![naver_target("u0")]);
        let skip = GroupSkip {
            code: "LOGIN_FAILED".into(),
            message: "아이디 또는 비밀번호가 올바르지 않습니다".into(),
            trace: None,
        };
        assert_eq!(
            skip.trace_body(),
            "아이디 또는 비밀번호가 올바르지 않습니다"
        );
        let items = build_items(
            &p,
            &synth_post_failures(&p, "u0", &skip),
            &[],
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(!items[0]
            .trace
            .as_deref()
            .unwrap_or_default()
            .contains("\n\n"));
    }

    #[test]
    fn synth_comment_failures_cover_comment_only_targets_not_both() {
        // comment 전용: 로그인/IP 실패 시 댓글 대상도 조용히 누락하지 않고 Fail로 남긴다.
        let mut t = naver_target("u0");
        t.comment_target = Some(CommentTargetSpec {
            mode: CommentTarget::Latest,
            count: Some(3),
            cafe_id: Some(123),
            article_id: None,
        });
        let p = plan(ModeValue::Comment, vec![t]);
        let skip = GroupSkip {
            code: "LOGIN_FAILED".into(),
            message: "쿠키 저장 실패".into(),
            trace: None,
        };
        let fails = synth_comment_failures(&p, "u0", &skip);
        assert_eq!(fails.len(), 1);
        assert_eq!(fails[0].cafe_id, 123);
        // build_items가 Fail 항목으로 렌더링한다.
        let items = build_items(&p, &[], &[], &fails, &[], &[], &[], &[]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, BatchItemStatus::Fail);
        // both 모드는 self-comment가 글에 의존하므로 별도 합성 댓글 실패를 만들지 않는다.
        let both = plan(ModeValue::Both, vec![naver_target("u0")]);
        assert!(synth_comment_failures(&both, "u0", &skip).is_empty());
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
        let b = build_log_batch(&p, &reports, &[], &[], &[], &[], &[], &[], 1, 0);
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
        let b = build_log_batch(&p, &[], &[], &[], &[], &[], &[], &[], 1, 0);
        assert!(b.body.is_none()); // comment 모드 → 본문 스냅샷 없음
        assert_eq!(b.comment.as_deref(), Some("좋은 글이네요")); // 공백 항목은 건너뜀
        assert!(b.items.is_empty());
    }

    #[test]
    fn build_log_batch_includes_forum_items_with_code_and_account() {
        let p = plan(ModeValue::Post, vec![]);
        let forum = vec![forum_ok("u0", "삼성전자", "005930")];
        let b = build_log_batch(&p, &[], &[], &forum, &[], &[], &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Forum);
        assert_eq!(b.items[0].target, "삼성전자");
        assert_eq!(b.items[0].code.as_deref(), Some("005930"));
        assert_eq!(b.items[0].login_id, "u0");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
    }

    #[test]
    fn build_log_batch_forum_fail_maps_friendly_msg_and_keeps_raw_trace() {
        // 실패 시 친절 사유를 메인에, 원문 기술 메시지+백트레이스는 자세히 보기(trace)로
        // 분리한다(#243). forum_fail 헬퍼의 message("엔진 오류")는 개발 용어가 없어 그대로 노출.
        let p = plan(ModeValue::Post, vec![]);
        let forum = vec![forum_fail(
            "u0",
            "삼성전자",
            "005930",
            "Chrome 실행 실패: connect refused",
        )];
        let b = build_log_batch(&p, &[], &[], &forum, &[], &[], &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        assert_eq!(b.items[0].msg, "엔진 오류");
        // 원문 message가 trace 맨 위에 보존되고 그 아래 백트레이스가 붙는다.
        assert_eq!(
            b.items[0].trace.as_deref(),
            Some("엔진 오류\n\nChrome 실행 실패: connect refused")
        );
    }

    #[test]
    fn forum_result_to_item_marks_skipped_as_skip_status() {
        // #267-9: 차단으로 건너뛴 글은 Fail(X)이 아니라 Skip("건너뜀")으로 표시한다.
        let result = ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: false,
            message: "앞선 글이 로그인/권한 오류로 실패해 건너뜀".into(),
            trace: None,
            posted: None,
            skipped: true,
        };
        let item = forum_result_to_item("u0", &result);
        assert_eq!(item.status, BatchItemStatus::Skip);
        assert_eq!(item.msg, "앞선 글이 로그인/권한 오류로 실패해 건너뜀");
        assert_eq!(item.trace, None);
    }

    #[test]
    fn waiting_login_ids_collect_only_successful_posts() {
        // #267-3: 글 게시 성공 계정만 대기 대상(실패·skip 제외).
        let forum = vec![
            forum_ok("acc_a", "삼성전자", "005930"),
            forum_fail("acc_b", "SK하이닉스", "000660", "trace"),
        ];
        let ids = successful_post_login_ids(&forum);
        assert!(ids.contains("acc_a"), "성공 계정은 대기 대상");
        assert!(!ids.contains("acc_b"), "실패 계정은 제외");
    }

    #[test]
    fn blocked_login_ids_collect_only_blocking_failures() {
        // #2: 게시 도중 차단(권한 만료 등)을 만난 계정만 모은다. 단순 엔진 오류·skip·성공은 제외.
        let forum = vec![
            forum_blocked("acc_block", "삼성전자", "005930"),
            forum_fail("acc_err", "SK하이닉스", "000660", "trace"),
            forum_skipped("acc_skip", "카카오", "035720"),
            forum_ok("acc_ok", "네이버", "035420"),
        ];
        let ids = blocked_post_login_ids(&forum);
        assert!(ids.contains("acc_block"), "차단성 실패 계정은 차단 대상");
        assert!(!ids.contains("acc_err"), "일반 엔진 오류는 차단 아님");
        assert!(!ids.contains("acc_skip"), "건너뜀(skip)은 차단 아님");
        assert!(!ids.contains("acc_ok"), "성공은 차단 아님");
    }

    #[test]
    fn mid_post_block_takes_precedence_over_waiting() {
        // #2 핵심: 한 계정이 1글 성공 후 2글에서 차단되면(부분 성공) — 대기가 아니라 차단으로
        // 분류돼야 한다. 같은 계정의 성공 결과가 있어도 차단이 우선한다.
        let forum = vec![
            forum_ok("acc_mixed", "삼성전자", "005930"),
            forum_blocked("acc_mixed", "SK하이닉스", "000660"),
            forum_skipped("acc_mixed", "카카오", "035720"),
        ];
        let blocked = blocked_post_login_ids(&forum);
        assert!(blocked.contains("acc_mixed"), "도중 차단된 계정은 차단");
        // 대기 후보(성공)에 들어가더라도, 호출부에서 차단 집합에 있으면 대기에서 빠진다.
        let waiting: Vec<String> = successful_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id))
            .collect();
        assert!(
            waiting.is_empty(),
            "차단 계정은 부분 성공이 있어도 대기로 두지 않는다"
        );
    }

    #[test]
    fn timed_out_login_ids_collect_only_timeout_and_server_500() {
        // #7: 페이지 대기시간 초과·네이버 서버 오류(HTTP 500)만 대기초과로 모은다. 차단·일반
        // 엔진 오류·skip·성공은 제외.
        let forum = vec![
            forum_timed_out(
                "acc_to1",
                "삼성전자",
                "005930",
                "페이지 로드 대기 시간이 초과되었습니다.",
            ),
            forum_timed_out(
                "acc_to2",
                "SK하이닉스",
                "000660",
                "네이버 서버에 문제가 발생했습니다 (REGISTER_HTTP_ERROR)",
            ),
            forum_blocked("acc_block", "카카오", "035720"),
            forum_skipped("acc_skip", "네이버", "035420"),
            forum_ok("acc_ok", "LG", "066570"),
        ];
        let ids = timed_out_post_login_ids(&forum);
        assert!(ids.contains("acc_to1"), "대기시간 초과는 대기초과 대상");
        assert!(ids.contains("acc_to2"), "HTTP 500 서버 오류는 대기초과 대상");
        assert!(!ids.contains("acc_block"), "차단(403)은 대기초과 아님");
        assert!(!ids.contains("acc_skip"), "건너뜀(skip)은 대기초과 아님");
        assert!(!ids.contains("acc_ok"), "성공은 대기초과 아님");
    }

    #[test]
    fn block_takes_precedence_over_timed_out() {
        // #7: 한 계정이 타임아웃과 차단을 모두 만나면 — 더 강한 종료성 실패인 차단이 우선한다.
        let forum = vec![
            forum_timed_out(
                "acc_mix",
                "삼성전자",
                "005930",
                "페이지 로드 대기 시간이 초과되었습니다.",
            ),
            forum_blocked("acc_mix", "SK하이닉스", "000660"),
        ];
        let blocked = blocked_post_login_ids(&forum);
        let timed_out: std::collections::BTreeSet<String> = timed_out_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id))
            .collect();
        assert!(blocked.contains("acc_mix"), "차단이 잡혀야 한다");
        assert!(
            !timed_out.contains("acc_mix"),
            "차단 계정은 대기초과에서 빠진다(차단 우선)"
        );
    }

    #[test]
    fn timed_out_takes_precedence_over_waiting() {
        // #7: 한 계정이 1글 성공 후 다른 글에서 타임아웃/500이면 — 대기가 아니라 대기초과로
        // 분류돼 게시 목록에서 숨겨진다(같은 계정의 성공이 있어도 대기초과 우선).
        let forum = vec![
            forum_ok("acc_mixed", "삼성전자", "005930"),
            forum_timed_out(
                "acc_mixed",
                "SK하이닉스",
                "000660",
                "네이버 서버에 문제가 발생했습니다",
            ),
        ];
        let blocked = blocked_post_login_ids(&forum);
        let timed_out: std::collections::BTreeSet<String> = timed_out_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id))
            .collect();
        let waiting: Vec<String> = successful_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id) && !timed_out.contains(id))
            .collect();
        assert!(timed_out.contains("acc_mixed"), "타임아웃 계정은 대기초과");
        assert!(
            waiting.is_empty(),
            "대기초과 계정은 부분 성공이 있어도 대기로 두지 않는다"
        );
    }

    #[test]
    fn errored_collects_unclassified_failures_only() {
        // 사용자 지시(후속): 차단도 대기초과도 아닌 게시 실패(약관 동의하기 비활성·버튼 못찾음,
        // 응답 읽기 IO 실패 등 "엔진 오류" 계열)는 전부 'Error'로 칠해 계정이 활성으로 안 남게 한다.
        let forum = vec![
            forum_fail("acc_err", "삼성전자", "005930", "trace-x"), // message "엔진 오류" — 미분류
            forum_blocked("acc_block", "카카오", "035720"),         // 차단(403)
            forum_timed_out("acc_to", "LG", "066570", "페이지 로드 대기 시간이 초과되었습니다."),
            forum_skipped("acc_skip", "네이버", "035420"),
            forum_ok("acc_ok", "SK하이닉스", "000660"),
        ];
        let ids = errored_post_login_ids(&forum);
        assert!(ids.contains("acc_err"), "미분류 실패는 에러 대상");
        assert!(!ids.contains("acc_block"), "차단은 에러 아님(전용 상태가 우선)");
        assert!(!ids.contains("acc_to"), "대기초과는 에러 아님(전용 상태가 우선)");
        assert!(!ids.contains("acc_skip"), "건너뜀(skip)은 에러 아님");
        assert!(!ids.contains("acc_ok"), "성공은 에러 아님");
    }

    #[test]
    fn error_takes_precedence_over_waiting_but_not_blocked_or_timed_out() {
        // 한 계정이 1글 성공 + 다른 글 미분류 실패면 — 대기가 아니라 에러로 표면화한다(#7과 동일
        // 철학). 같은 계정에 차단/대기초과가 있으면 그 전용 상태가 우선이라 에러에서 빠진다.
        let forum = vec![
            forum_ok("acc_mix", "삼성전자", "005930"),
            forum_fail("acc_mix", "SK하이닉스", "000660", "trace-y"),
            forum_blocked("acc_block_err", "카카오", "035720"),
            forum_fail("acc_block_err", "네이버", "035420", "trace-z"),
        ];
        let blocked = blocked_post_login_ids(&forum);
        let timed_out: std::collections::BTreeSet<String> = timed_out_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id))
            .collect();
        let errored: std::collections::BTreeSet<String> = errored_post_login_ids(&forum)
            .into_iter()
            .filter(|id| !blocked.contains(id) && !timed_out.contains(id))
            .collect();
        let waiting: Vec<String> = successful_post_login_ids(&forum)
            .into_iter()
            .filter(|id| {
                !blocked.contains(id) && !timed_out.contains(id) && !errored.contains(id)
            })
            .collect();
        assert!(errored.contains("acc_mix"), "성공+미분류실패 계정은 에러");
        assert!(
            waiting.is_empty(),
            "에러 계정은 부분 성공이 있어도 대기로 두지 않는다"
        );
        assert!(blocked.contains("acc_block_err"), "차단이 잡혀야 한다");
        assert!(
            !errored.contains("acc_block_err"),
            "차단 계정은 에러에서 빠진다(차단 우선)"
        );
    }

    #[test]
    fn forum_failure_reason_maps_http_status_to_korean() {
        // packet_client가 만드는 "… 패킷 HTTP 실패: HTTP status 403 …"를 비개발자용 사유로(#243).
        let msg = "글쓰기 form 패킷 HTTP 실패: HTTP status 403 Forbidden for url (https://m.stock.naver.com/x)";
        assert_eq!(
            forum_failure_reason(msg),
            "권한이 없거나 로그인이 만료되었습니다"
        );
        assert_eq!(parse_http_status(msg), Some(403));
        // "status=500, body=…" 형식도 잡는다.
        assert_eq!(
            parse_http_status("글쓰기 form 패킷 HTTP 실패: status=500, body=x"),
            Some(500)
        );
    }

    #[test]
    fn forum_failure_reason_keeps_friendly_korean_and_hides_jargon() {
        // 개발 용어 없는 안내문은 그대로, 기술 원문은 일반 폴백으로 가린다(#243).
        assert_eq!(
            forum_failure_reason(
                "네이버 로그인이 확인되지 않았습니다. Chrome에서 로그인한 뒤 다시 실행하세요."
            ),
            "네이버 로그인이 확인되지 않았습니다. Chrome에서 로그인한 뒤 다시 실행하세요."
        );
        assert_eq!(
            forum_failure_reason("글쓰기 form 응답에서 txId를 찾지 못했습니다."),
            "게시에 실패했습니다. 계정 로그인·잠금 상태를 확인한 뒤 다시 시도해 주세요"
        );
    }

    #[test]
    fn forum_failure_reason_maps_transport_failure_to_network_not_lock() {
        // 전송 계층(연결/DNS/타임아웃) 실패는 로그인·잠금이 아니라 '네트워크 끊김'이다(#330 후속).
        // getProfile send 실패가 "패킷" 단어 때문에 잠금 폴백으로 새던 버그를 막는다.
        let network = "잠시 인터넷 연결이 끊겨 게시에 실패했습니다. 잠시 후 다시 시도해 주세요";
        // reqwest 전송 실패 원문(우리가 "전송 실패" 접두어를 붙임).
        assert_eq!(
            forum_failure_reason(
                "getProfile 패킷 전송 실패: error sending request for url (https://static.nid.naver.com/getProfile)"
            ),
            network
        );
        // 타임아웃 source가 붙은 형태도 잠금이 아니라 네트워크로.
        assert_eq!(
            forum_failure_reason("getProfile 패킷 전송 실패: operation timed out"),
            network
        );
        // 잠금 폴백과 헷갈리지 않게: HTTP 상태가 있으면 여전히 상태 매핑이 우선.
        assert_eq!(
            forum_failure_reason("글쓰기 form 패킷 HTTP 실패: HTTP status 403 for url (x)"),
            "권한이 없거나 로그인이 만료되었습니다"
        );
    }

    #[test]
    fn is_network_transport_failure_recognizes_all_markers_and_excludes_others() {
        // 우리가 send 실패에 붙이는 한국어 접두어.
        assert!(is_network_transport_failure("getProfile 패킷 전송 실패: x"));
        // reqwest/하부가 남기는 영어 표식(대소문자 무관).
        for marker in [
            "error sending request for url (x)",
            "tcp connect error: refused",
            "dns error: failed to lookup address",
            "operation timed out",
            "request Timeout reached",
            "connection refused (os error 111)",
        ] {
            assert!(
                is_network_transport_failure(marker),
                "전송 계층 실패여야 함: {marker}"
            );
        }
        // 전송과 무관한 메시지는 네트워크로 오분류하면 안 된다(잠금·HTTP상태·게시 파싱).
        assert!(!is_network_transport_failure("아이디 잠금조치"));
        assert!(!is_network_transport_failure("HTTP status 403 Forbidden for url (x)"));
        assert!(!is_network_transport_failure(
            "글쓰기 form 응답에서 txId를 찾지 못했습니다."
        ));
    }

    #[test]
    fn forum_result_to_item_maps_status_and_preserves_original_in_trace() {
        // 메인은 친절 사유, trace 맨 위엔 원문 기술 메시지 보존(#243).
        let result = ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: false,
            message: "글쓰기 form 패킷 HTTP 실패: HTTP status 403 Forbidden for url (https://x)"
                .into(),
            trace: Some("at foo.rs:1\n\nframe0".into()),
            posted: None,
            skipped: false,
        };
        let item = forum_result_to_item("u0", &result);
        assert_eq!(item.status, BatchItemStatus::Fail);
        assert_eq!(item.msg, "권한이 없거나 로그인이 만료되었습니다");
        assert_eq!(
            item.trace.as_deref(),
            Some("글쓰기 form 패킷 HTTP 실패: HTTP status 403 Forbidden for url (https://x)\n\nat foo.rs:1\n\nframe0")
        );
    }

    #[test]
    fn forum_result_to_item_empty_reason_falls_back() {
        // 사유가 비는 예외적 경우에만 일반 폴백 문구를 쓴다(#243).
        let result = ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: false,
            message: "   ".into(),
            trace: None,
            posted: None,
            skipped: false,
        };
        let item = forum_result_to_item("u0", &result);
        assert_eq!(item.msg, "종목토론방 게시에 실패했습니다");
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
        let b = build_log_batch(&p, &[], &[], &[], &bands, &[], &[], &[], 1, 0);
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
        let b = build_log_batch(&p, &[], &[], &[], &bands, &[], &[], &[], 1, 0);
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

    // --- 네이버 블로그 댓글(#271) 완료 로그 매핑 ---

    fn blog_ok(account: &str, name: &str) -> BlogOutcome {
        BlogOutcome {
            account_id: account.into(),
            name: name.into(),
            link: format!("https://blog.naver.com/{name}/100"),
            contents: "좋은 글이네요".into(),
            result: Ok(crate::naver_blog::BlogCommentResult {
                comment_no: "7".into(),
                contents: "좋은 글이네요".into(),
            }),
        }
    }

    fn blog_fail(account: &str, name: &str) -> BlogOutcome {
        BlogOutcome {
            account_id: account.into(),
            name: name.into(),
            link: format!("https://blog.naver.com/{name}/200"),
            contents: String::new(),
            result: Err(crate::naver_blog::BlogError::new("groupId를 찾지 못했습니다")),
        }
    }

    #[test]
    fn build_log_batch_maps_blog_success_with_comment_and_url() {
        // 블로그는 카페·밴드와 별개 platform(Blog)으로, 성공 시 단 댓글 본문과 글 URL을 남긴다.
        let p = plan(ModeValue::Comment, vec![]);
        let blog = vec![blog_ok("u0", "press02")];
        let b = build_log_batch(&p, &[], &[], &[], &[], &blog, &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Blog);
        assert_eq!(b.items[0].target, "press02");
        assert_eq!(b.items[0].login_id, "u0");
        assert_eq!(b.items[0].status, BatchItemStatus::Success);
        assert_eq!(b.items[0].msg, "댓글 게시 완료");
        // 완료 로그에 단 댓글 본문과 글 URL을 보존한다(올라간 글 열기).
        let posted = b.items[0].posted.as_ref().expect("posted가 있어야 함");
        assert_eq!(posted.comment.as_deref(), Some("좋은 글이네요"));
        assert_eq!(
            posted.url.as_deref(),
            Some("https://blog.naver.com/press02/100")
        );
        assert!(b.items[0].trace.is_none());
    }

    #[test]
    fn build_log_batch_maps_blog_failure_with_backtrace() {
        // 실패는 메인=친절 사유 / 자세히=BlogError trace(backtrace 포함, #199)로 나눈다.
        let p = plan(ModeValue::Comment, vec![]);
        let blog = vec![blog_fail("u1", "cho41004")];
        let b = build_log_batch(&p, &[], &[], &[], &[], &blog, &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Blog);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        assert!(b.items[0].msg.contains("댓글 게시 실패"));
        // trace는 자세히 보기에 노출되며 실패 지점 앵커(at …)를 항상 포함한다.
        assert!(b.items[0]
            .trace
            .as_deref()
            .is_some_and(|t| t.contains("at ")));
        assert!(b.items[0].posted.is_none());
    }

    #[test]
    fn blog_shortfall_outcome_renders_as_fail_no_posts() {
        // "최신 N개" 모드에서 글이 모자라면(#279) 부족분은 "글이 없습니다" 실패로 즉시 남긴다.
        let p = plan(ModeValue::Comment, vec![]);
        let shortfall = BlogOutcome {
            account_id: "u0".into(),
            name: "press02".into(),
            link: "https://blog.naver.com/press02".into(),
            contents: String::new(),
            result: Err(crate::naver_blog::BlogError::new("글이 없습니다")),
        };
        let b = build_log_batch(&p, &[], &[], &[], &[], &[shortfall], &[], &[], 1, 0);
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.items[0].platform, PlatformId::Blog);
        assert_eq!(b.items[0].status, BatchItemStatus::Fail);
        assert!(b.items[0].msg.contains("글이 없습니다"));
        assert!(b.items[0].posted.is_none());
    }

    #[test]
    fn synth_blog_failures_cover_blog_targets_on_group_skip() {
        // 로그인/IP 실패로 그룹을 건너뛰면, 블로그 댓글 대상도 조용히 누락하지 않고 Fail로 남긴다.
        let mut p = plan(ModeValue::Comment, vec![]);
        p.blog = vec![crate::ipc::queue::BlogTarget {
            account_id: "u0".into(),
            name: "press02".into(),
            blog_id: "press02".into(),
            log_no: "100".into(),
            link: "https://blog.naver.com/press02/100".into(),
            count: None,
            category_no: None,
        }];
        let skip = GroupSkip {
            code: "LOGIN_FAILED".into(),
            message: "로그인 실패".into(),
            trace: None,
        };
        let outcomes = synth_blog_failures(&p, "u0", &skip);
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].result.is_err());
        // 다른 계정 대상은 합성하지 않는다.
        assert!(synth_blog_failures(&p, "other", &skip).is_empty());
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
        assert!(collect_comment_targets(&p, &[], None)
            .await
            .targets
            .is_empty());
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
        let b = build_log_batch(&p, &[], &[], &[], &[], &[], &[], &failures, 1, 0);
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

    // #219: 게시 큐/완료 로그의 "올라간 글 열기"가 동작하려면 변환 단계에서 posted.url을
    // 채워야 한다. 카페 글/댓글·밴드 글이 각각 올바른 URL을 채우는지 검증한다.
    #[test]
    fn cafe_post_success_fills_article_url() {
        let p = plan(ModeValue::Post, vec![naver_target("u1")]);
        let report = JobReport {
            account_id: "u1".into(),
            cafe: "123".into(),
            menu_id: 7,
            success: true,
            result: Some(ArticleRegisterResult {
                cafe_id: 999,
                article_id: 42,
                menu_id: 7,
            }),
            error: None,
        };
        let item = post_report_to_item(&p, &report);
        let posted = item.posted.expect("성공이면 posted가 있어야 한다");
        assert_eq!(
            posted.url.as_deref(),
            Some("https://cafe.naver.com/ca-fe/cafes/999/articles/42")
        );
        // 종토방·밴드처럼 작성 내용(plan의 제목/본문)도 실어 "게시 내용"에 보이게 한다.
        assert_eq!(posted.title, "T");
        assert_eq!(posted.body, "B");
        // 카페 댓글은 별도 항목이라 글 항목의 comment는 비운다.
        assert_eq!(posted.comment, None);
    }

    #[test]
    fn cafe_post_failure_has_no_url() {
        let p = plan(ModeValue::Post, vec![naver_target("u1")]);
        let report = JobReport {
            account_id: "u1".into(),
            cafe: "123".into(),
            menu_id: 7,
            success: false,
            result: None,
            error: None,
        };
        let item = post_report_to_item(&p, &report);
        assert!(item.posted.is_none());
    }

    #[test]
    fn cafe_comment_success_fills_comment_and_target_article_url() {
        let p = plan(ModeValue::Comment, vec![naver_target("u1")]);
        let report = CommentJobReport {
            account_id: "u1".into(),
            cafe_id: 123,
            article_id: 55,
            success: true,
            result: None,
            error: None,
            content: "정말 좋은 글이네요".into(),
        };
        // 댓글 1건 = 1행. 성공 시 댓글 본문과 대상 글 URL이 채워진다.
        let item = comment_report_to_item(&p, &report);
        assert_eq!(item.status, BatchItemStatus::Success);
        assert_eq!(item.msg, "댓글 게시 완료");
        let posted = item.posted.expect("성공 시 게시 내용이 채워진다");
        assert_eq!(posted.comment.as_deref(), Some("정말 좋은 글이네요"));
        assert_eq!(
            posted.url.as_deref(),
            Some("https://cafe.naver.com/ca-fe/cafes/123/articles/55")
        );
    }

    #[test]
    fn comment_skeleton_shows_pending_then_running_phase() {
        let p = plan(ModeValue::Comment, vec![naver_target("u1")]);
        let job = CommentJob {
            account_id: "u1".into(),
            cafe_id: 123,
            article_id: 55,
            content: "댓글".into(),
        };
        // 시작 전 "게시 전"(대기), 시작 후 "게시 중…"(진행)으로 단계가 바뀐다.
        let pending = comment_skeleton(&p, &job, BatchItemStatus::Waiting);
        assert_eq!(pending.status, BatchItemStatus::Waiting);
        assert_eq!(pending.msg, "댓글 게시 전");
        assert!(pending.posted.is_none());
        let running = comment_skeleton(&p, &job, BatchItemStatus::Running);
        assert_eq!(running.status, BatchItemStatus::Running);
        assert_eq!(running.msg, "댓글 게시 중…");
    }

    #[test]
    fn band_published_fills_web_url() {
        let outcome = BandOutcome {
            account_id: "u1".into(),
            band_name: "테스트밴드".into(),
            result: Ok(BandJobResult::Published(BandPublishOutcome {
                joined: true,
                post_no: 7,
                web_url: "https://band.us/band/100/post/7".into(),
                commented_count: 0,
                comment_total: 0,
                band_name: None,
            })),
        };
        let item = band_outcome_to_item(&outcome, "제목", "본문", Some("댓글"));
        assert_eq!(
            item.posted.clone().and_then(|c| c.url).as_deref(),
            Some("https://band.us/band/100/post/7")
        );
        // 작성 내용(제목/본문)도 함께 보존된다.
        assert_eq!(item.posted.as_ref().map(|c| c.title.as_str()), Some("제목"));
        assert_eq!(item.posted.as_ref().map(|c| c.body.as_str()), Some("본문"));
    }

    #[test]
    fn band_comment_only_fills_comment_without_url() {
        // 댓글 전용은 여러 글 대상이라 단일 글 URL이 없어 url은 비우되, 단 댓글 본문은 채운다(#245).
        let outcome = BandOutcome {
            account_id: "u1".into(),
            band_name: "테스트밴드".into(),
            result: Ok(BandJobResult::Commented(BandCommentOutcome {
                target_count: 3,
                commented_count: 3,
                band_name: None,
            })),
        };
        let item = band_outcome_to_item(&outcome, "제목", "본문", Some("댓글"));
        let posted = item.posted.expect("성공 시 게시 내용이 채워진다");
        assert_eq!(posted.comment.as_deref(), Some("댓글"));
        assert!(posted.url.is_none());
    }
}
