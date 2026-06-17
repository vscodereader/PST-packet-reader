//! Queue (게시 큐) domain — JSON-file-backed, wired over Tauri IPC. Reuses
//! `PlatformId`/`ModeValue` from the accounts/posts modules so the generated TS
//! bindings stay a single source of truth.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::PlatformId;
use super::log_batches::BatchItem;
use super::posts::{CommentTarget, ModeValue};
use crate::ipc::activity::{record, ActivityType};
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum QueueState {
    Running,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueLocation {
    pub p: PlatformId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
}

// --------------------------------------------------------------------------
// Execution payload (게시 실행 페이로드)
//
// 표시용 `QueueLocation`과 달리, 큐 아이템을 워커가 실제로 게시하는 데 필요한
// 본문·계정·플랫폼 파라미터를 담는다. 본문(title/body_text/comments)은 **예약 시점에
// 동결(snapshot)** 해, 이후 원본 글이 수정/삭제돼도 예약 당시 내용으로 게시한다(이슈 #142).
// --------------------------------------------------------------------------

/// 댓글 게시 대상 지정. `mode`가 `latest`/`popular`면 `count`(상위 N개)를, `url`이면
/// `cafe_id`/`article_id`를 사용한다. 표시용 enum `CommentTarget`을 재사용한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommentTargetSpec {
    pub mode: CommentTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub cafe_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub article_id: Option<u64>,
}

/// 네이버 카페 게시 대상 1건 (orchestrator `PostJob`의 재료). `account_id`는 로그인 쿠키
/// 키(= loginId) 규약을 따른다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct NaverTarget {
    pub account_id: String,
    pub cafe: String,
    /// 카페 표시 이름(예약 시점 동결). 완료 로그에 카페 ID 대신 보여준다. 과거에
    /// 저장된 plan에는 없을 수 있어 기본값(빈 문자열)을 허용한다.
    #[serde(default)]
    pub cafe_name: String,
    #[ts(type = "number")]
    pub menu_id: u64,
    pub board_type: String,
    /// comment/both 모드에서 댓글을 달 대상. post 전용이면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_target: Option<CommentTargetSpec>,
}

/// 종목토론방 게시 대상 1건 (`ForumPublishRequest`의 stock 재료).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ForumTarget {
    pub account_id: String,
    pub name: String,
    pub code: String,
}

/// 밴드(band.us) 게시 대상 1건. `account_id`는 band 로그인 쿠키 키(= loginId) 규약을
/// 따른다. post/both 모드는 새 글을 쓰고(both면 그 글에 댓글) `band_publish`로, comment
/// 전용 모드는 기존 글(최신/인기) 상위 N개에 댓글을 다는 `band_comment`로 처리된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BandTarget {
    pub account_id: String,
    /// 밴드 표시 이름(예약 시점 동결). 완료 로그/큐 표시에 ID 대신 보여준다.
    pub name: String,
    /// band.us 가입·게시 링크(또는 band_no). `band_publish`가 여기서 band_no를 추출한다.
    pub link: String,
    /// 댓글 전용 모드(`kind == Comment`)에서 댓글을 달 기존 글 대상. post/both면 None.
    /// 밴드는 url 댓글을 지원하지 않아 `mode`는 latest/popular만 의미가 있다(url이면
    /// 호출부가 latest로 폄). `cafe_id`/`article_id`는 밴드에서 쓰지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_target: Option<CommentTargetSpec>,
}

/// 로그인 배치의 계정 1건. `account_id`는 로그인 쿠키 키(= loginId) 규약을 따른다.
/// `platform`이 `Band`면 band.us 로그인(`process_band_account`), 그 외(naver/forum 등)는
/// 네이버 로그인(`process_account`)으로 처리된다(프론트 `runLogin`의 naver/band 분기 미러).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LoginTarget {
    pub account_id: String,
    pub platform: PlatformId,
    pub headless: bool,
    pub use_adb: bool,
    pub force: bool,
}

/// 큐 아이템을 실제로 게시하는 데 필요한 동결된 실행 페이로드.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PublishPlan {
    /// 출처 글(LibraryPost) id — 추적/표시용. 본문은 아래 필드에 동결되어 있다.
    pub post_id: String,
    pub kind: ModeValue,
    pub title: String,
    pub body_text: String,
    #[serde(default)]
    pub comments: Vec<String>,
    /// `#{링크}` 토큰 치환에 쓸 사용자 지정 링크값(선택). 비우면 종목별 시세 링크를 쓴다.
    /// 기존 plan과 호환되도록 기본값(빈 문자열)을 허용한다.
    #[serde(default)]
    pub link_override: String,
    #[serde(default)]
    pub naver: Vec<NaverTarget>,
    #[serde(default)]
    pub forum: Vec<ForumTarget>,
    #[serde(default)]
    pub band: Vec<BandTarget>,
    /// 로그인 전용 아이템의 계정 목록. 게시 아이템에는 없다(직렬화 생략 → 기존 plan과 호환).
    /// 워커(`execute_item`)는 이 필드가 채워진 아이템을 게시 대신 계정별 로그인으로 처리한다
    /// (배치 1개 = 아이템 1개, 진행률 분모 = 계정 수). 일원화: 로그인도 now 큐로 흐른다(#210).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub login: Option<Vec<LoginTarget>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueNowItem {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub state: QueueState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub progress: Option<(u32, u32)>,
    pub locs: Vec<QueueLocation>,
    /// 워커가 실제 게시에 사용하는 실행 페이로드. 레거시/표시 전용 아이템은 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan: Option<PublishPlan>,
    /// 워커가 phase별로 갱신하는 대상별 실시간 상태(진행 전/중/완료/실패). 알림 로그와
    /// 같은 `BatchItem` 모델을 재사용해 프론트가 SubLog를 그대로 쓴다. 대기/표시 전용
    /// 아이템은 빈 Vec(레거시 JSON엔 키가 없어 기본값 빈 Vec로 역직렬화).
    #[serde(default)]
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct QueueScheduledItem {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub when: String,
    pub rel: String,
    /// 예약 시각(epoch ms). 자동 트리거 스케줄러가 이 값과 현재 시각을 비교한다.
    /// 과거에 저장된(표시 전용) 아이템엔 없을 수 있어 기본값(0)을 허용한다.
    #[serde(default)]
    #[ts(type = "number")]
    pub at: i64,
    /// 앱이 꺼져 있는 동안 예약 시각이 지나 미발행된 상태. 자동 게시하지 않고 사용자가
    /// 재예약/취소하도록 표시한다. 기본 false.
    #[serde(default)]
    pub missed: bool,
    pub locs: Vec<QueueLocation>,
    /// 워커가 실제 게시에 사용하는 실행 페이로드. 레거시/표시 전용 아이템은 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan: Option<PublishPlan>,
}

// --------------------------------------------------------------------------
// Pure logic
// --------------------------------------------------------------------------

pub fn apply_cancel_now(items: Vec<QueueNowItem>, id: &str) -> Vec<QueueNowItem> {
    items.into_iter().filter(|i| i.id != id).collect()
}

pub fn apply_cancel_scheduled(items: Vec<QueueScheduledItem>, id: &str) -> Vec<QueueScheduledItem> {
    items.into_iter().filter(|i| i.id != id).collect()
}

/// now 큐를 `ordered_ids` 순서로 재배열한다. 단 실행 중(Running) 아이템은 워커가
/// 처리 중이므로 순서를 바꾸지 않고 항상 맨 앞에 고정한다(프론트 UI의 "running은 0번"
/// 가드와 일치). `ordered_ids`에 없는(알 수 없는) 아이템은 원래 상대 순서대로 뒤에
/// 보존해 유실을 막는다.
pub fn apply_reorder_now(items: Vec<QueueNowItem>, ordered_ids: &[String]) -> Vec<QueueNowItem> {
    let mut running = Vec::new();
    let mut rest = Vec::new();
    for item in items {
        if item.state == QueueState::Running {
            running.push(item);
        } else {
            rest.push(item);
        }
    }

    let mut ordered = Vec::with_capacity(rest.len());
    for id in ordered_ids {
        if let Some(pos) = rest.iter().position(|i| &i.id == id) {
            ordered.push(rest.remove(pos));
        }
    }
    // ordered_ids에 빠진 대기 아이템은 원래 순서대로 뒤에 보존한다.
    ordered.extend(rest);

    running.extend(ordered);
    running
}

/// 큐 아이템의 자동 우선순위(낮을수록 먼저 실행). **0 = 로그인 전용, 1 = 종목토론방 포함,
/// 2 = 그 외(카페/밴드/표시 전용)**. 사수 지침: 로그인 1순위·종토 2순위(#229).
///
/// - 로그인 전용 = `plan.login`이 비어있지 않고 게시 타깃(naver/forum/band)이 하나도 없는
///   아이템(#210). 게시에 로그인이 동봉된 아이템은 그 게시 등급으로 본다(로그인 전용 아님).
/// - 종목토론방 = `plan.forum` 비어있지 않음. 카페·밴드와 섞인 혼합 아이템도 forum이 끼면
///   2순위(=1)로 승격한다.
pub fn item_priority(item: &QueueNowItem) -> u8 {
    let Some(plan) = item.plan.as_ref() else {
        return 2;
    };
    let login_only = plan.login.as_ref().is_some_and(|l| !l.is_empty())
        && plan.naver.is_empty()
        && plan.forum.is_empty()
        && plan.band.is_empty();
    if login_only {
        0
    } else if !plan.forum.is_empty() {
        1
    } else {
        2
    }
}

/// now 큐를 자동 우선순위로 재정렬한다(로그인 1 > 종토 2 > 카페/밴드). 실행 중(Running)
/// 아이템은 `apply_reorder_now`와 동일하게 맨 앞에 고정한다(워커가 처리 중이라 건드리지
/// 않는다). 대기(Waiting)는 `item_priority`로 **안정 정렬** — 같은 등급은 들어온 순서(FIFO)와
/// 사용자가 수동으로 잡아둔 순서를 그대로 보존한다(수동 드래그는 등급 안에서만 유효, #229).
pub fn apply_priority_order(items: Vec<QueueNowItem>) -> Vec<QueueNowItem> {
    let mut running = Vec::new();
    let mut waiting = Vec::new();
    for item in items {
        if item.state == QueueState::Running {
            running.push(item);
        } else {
            waiting.push(item);
        }
    }
    // slice::sort_by_key는 안정 정렬 — 동일 우선순위의 기존 상대 순서를 보존한다.
    waiting.sort_by_key(item_priority);
    running.extend(waiting);
    running
}

/// 실행 중(Running) 아이템을 **삭제하지 않고** 대기(Waiting)로 되돌린다 — 더 높은 우선순위
/// 작업(로그인/종토방)에 자리를 내주는 "중지(yield)"용(#232). `remaining_plan`은 **아직 게시하지
/// 않은 계정 그룹만** 담은 축소 plan이라(완료 그룹은 `retain_plan_accounts`로 제거됨) 재개 시
/// 중복 게시가 0이다. 진행 메타(batch_id/progress/items)는 비워 잔여 작업 기준으로 다시 채워지게
/// 하고, `apply_priority_order`로 재정렬해 양보한 아이템이 선점한 고우선 대기자 아래로 내려가게
/// 한다. id가 큐에 없으면(양보 직전 사용자가 취소) 아무것도 되살리지 않는다(no-op).
pub fn apply_yield_now(
    items: Vec<QueueNowItem>,
    id: &str,
    remaining_plan: PublishPlan,
) -> Vec<QueueNowItem> {
    let items = items
        .into_iter()
        .map(|mut item| {
            if item.id == id {
                item.state = QueueState::Waiting;
                item.plan = Some(remaining_plan.clone());
                item.batch_id = None;
                item.progress = None;
                item.items = Vec::new();
            }
            item
        })
        .collect();
    apply_priority_order(items)
}

/// 즉시 처리 대기열(now 큐)에 새로 적재되는 아이템을 정규화한다 — 워커가 실행 상태를
/// 채우므로 항상 대기 상태로 시작하고 실행 메타(batch_id/progress)는 비운다. 프론트가
/// 보낸 값에 대한 방어(add_queue_scheduled가 missed를 강제 해제하는 것과 같은 취지).
pub fn as_fresh_now_item(mut item: QueueNowItem) -> QueueNowItem {
    item.state = QueueState::Waiting;
    item.batch_id = None;
    item.progress = None;
    item.items = Vec::new();
    item
}

/// Convert a scheduled item into a waiting immediate-queue item (for "즉시 처리").
/// 실행 페이로드(plan)도 그대로 옮겨, 승격된 아이템을 워커가 게시할 수 있게 한다.
pub fn to_now_item(s: QueueScheduledItem) -> QueueNowItem {
    QueueNowItem {
        id: s.id,
        title: s.title,
        kind: s.kind,
        state: QueueState::Waiting,
        batch_id: None,
        progress: None,
        locs: s.locs,
        plan: s.plan,
        items: Vec::new(),
    }
}

/// 게시 큐는 빈 상태로 시작한다 — 실제 게시 작업만 큐에 들어가야 하므로 데모 시드를
/// 두지 않는다(이슈 #142). 시그니처는 `JsonStore::load_or_seed` 호출부 호환을 위해 유지.
pub fn seed_now() -> Vec<QueueNowItem> {
    vec![]
}

pub fn seed_scheduled() -> Vec<QueueScheduledItem> {
    vec![]
}

// --------------------------------------------------------------------------
// Commands
// --------------------------------------------------------------------------

#[tauri::command]
pub fn list_queue_now(store: tauri::State<'_, JsonStore<QueueNowItem>>) -> Vec<QueueNowItem> {
    store.snapshot()
}

#[tauri::command]
pub fn list_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
) -> Vec<QueueScheduledItem> {
    store.snapshot()
}

#[tauri::command]
pub fn cancel_queue_now(
    store: tauri::State<'_, JsonStore<QueueNowItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<QueueNowItem> {
    let next = store.mutate(|items| apply_cancel_now(items, &id));
    record(activity.inner(), ActivityType::Info, "진행 작업 취소됨");
    next
}

/// 즉시 게시("지금 바로")를 게시 큐(now 큐)에 적재한다(이슈 #198). 예약
/// (`add_queue_scheduled`)이 scheduled 큐에 넣는 것과 달리, 곧바로 워커가 집어가도록 now
/// 큐에 대기 상태로 push하고 워커를 기동한다(`start_if_idle`). 워커(`execute_item`)가
/// 카페 글·댓글·종목토론방·밴드 게시와 완료 로그를 모두 처리하므로, 즉시 게시와 예약
/// 게시가 동일 실행 경로로 수렴한다.
#[tauri::command]
pub fn add_queue_now<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    now: tauri::State<'_, JsonStore<QueueNowItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    runner: tauri::State<'_, super::queue_runner::NowQueueRunner>,
    item: QueueNowItem,
) -> Vec<QueueNowItem> {
    let title = item.title.clone();
    let after = now.mutate(|mut items| {
        items.push(as_fresh_now_item(item));
        // 적재 직후 자동 우선순위로 재정렬해, 새 종토/로그인이 대기열(및 화면)에서 위로
        // 올라가게 한다(#229). 실행 중 아이템은 맨 앞 고정이라 안 건드린다.
        apply_priority_order(items)
    });
    super::queue_runner::start_if_idle(runner.inner(), app);
    record(
        activity.inner(),
        ActivityType::Info,
        format!("즉시 처리 대기열에 추가됨 — {title}"),
    );
    after
}

/// 대기열 순서를 `ordered_ids`대로 영속화한다(드래그/우선순위 변경). 빈번한 조작이라
/// activity 피드에는 기록하지 않는다.
#[tauri::command]
pub fn reorder_queue_now(
    store: tauri::State<'_, JsonStore<QueueNowItem>>,
    ordered_ids: Vec<String>,
) -> Vec<QueueNowItem> {
    // 사용자가 잡은 수동 순서를 적용한 뒤 자동 우선순위로 한 번 더 정렬한다 — 수동 드래그는
    // 같은 등급 안에서만 유효하고, 등급 간 순서(로그인>종토>카페/밴드)는 항상 우선한다(#229).
    store.mutate(|items| apply_priority_order(apply_reorder_now(items, &ordered_ids)))
}

#[tauri::command]
pub fn cancel_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<QueueScheduledItem> {
    let next = store.mutate(|items| apply_cancel_scheduled(items, &id));
    record(activity.inner(), ActivityType::Info, "예약 취소됨");
    next
}

/// True if `at` is no earlier than the start of the current minute. Minute
/// precision keeps "now" (the picker default) schedulable despite seconds drift.
///
/// 의도된 부작용: `at`이 "현재 분"의 과거 초(최대 ~59초 전)여도 통과한다. 그런 예약은
/// 곧바로 `pick_due`(at <= now)에 걸려 다음 tick에서 즉시 자동 게시된다 — 분 단위 picker
/// 의 "지금" 선택을 허용하기 위한 의도적 동작이며, 초 단위 미래 예약은 지원하지 않는다.
pub fn is_future_enough(at: i64, now: i64) -> bool {
    at >= now - now.rem_euclid(60_000)
}

/// 시작 시 "놓침" 판정의 유예(1분). 직전 1분 내에 예약 시각이 된 아이템은 놓침으로
/// 보지 않고 티커가 곧 게시한다(앱을 막 켠 직후의 오판 방지).
const MISSED_GRACE_MS: i64 = 60_000;

/// 지금 게시해야 할 예약 아이템의 id 목록(`at <= now` 이고 놓침이 아닌 것). 티커가
/// 앱 실행 중 시각이 도래한 아이템을 promote하는 데 쓴다(순서 보존).
pub fn pick_due(items: &[QueueScheduledItem], now: i64) -> Vec<String> {
    items
        .iter()
        .filter(|s| !s.missed && s.at <= now)
        .map(|s| s.id.clone())
        .collect()
}

/// 앱 시작 시 호출: 유예를 넘겨(`at <= now - MISSED_GRACE_MS`) 미발행 상태로 지나간
/// 예약을 `missed`로 표시한다. 게시는 하지 않는다. 멱등(이미 missed면 그대로).
pub fn mark_missed(mut items: Vec<QueueScheduledItem>, now: i64) -> Vec<QueueScheduledItem> {
    for item in &mut items {
        if !item.missed && item.at <= now - MISSED_GRACE_MS {
            item.missed = true;
        }
    }
    items
}

/// 예약 시각을 변경한다(재예약): 매칭 id의 `at`/표시 문자열(`when`/`rel`)을 갱신하고
/// `missed`를 해제한다. 매칭 없으면 원본 유지.
pub fn apply_reschedule(
    mut items: Vec<QueueScheduledItem>,
    id: &str,
    at: i64,
    when: &str,
    rel: &str,
) -> Vec<QueueScheduledItem> {
    for item in &mut items {
        if item.id == id {
            item.at = at;
            item.when = when.to_owned();
            item.rel = rel.to_owned();
            item.missed = false;
        }
    }
    items
}

/// Append a new scheduled item (used when a post is scheduled from the publish
/// modal). Rejects a past time — defense-in-depth behind the picker's own guard.
#[tauri::command]
pub fn add_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    item: QueueScheduledItem,
    at: i64,
) -> Result<Vec<QueueScheduledItem>, String> {
    if !is_future_enough(at, crate::util::now_ms()) {
        return Err("예약 시각이 현재보다 과거입니다".into());
    }
    let title = item.title.clone();
    let next = store.mutate(|mut items| {
        // 예약 시각을 아이템에 박제한다 — 자동 트리거 스케줄러가 이 값을 본다.
        let mut item = item;
        item.at = at;
        item.missed = false;
        items.push(item);
        items
    });
    record(
        activity.inner(),
        ActivityType::Info,
        format!("예약 추가됨 — {title}"),
    );
    Ok(next)
}

/// 예약 아이템 1건을 now 큐로 승격한다(수동 "즉시 처리"·자동 스케줄러 공용). scheduled
/// 에서 제거 → now에 추가(plan 보존) → 워커 기동. 활동 로그는 호출부가 맥락에 맞게 남긴다
/// (수동/자동 메시지가 다름). 아이템이 없으면(취소·중복 race) `None`.
///
/// 성공 시 **워커 기동 직전에 잡은** now 큐 스냅샷을 `Some`으로 돌려준다. 워커는 비동기로
/// 큐를 비우므로(특히 plan이 없는 아이템은 즉시 완료·제거), 기동 후 `now.snapshot()`을
/// 다시 읽으면 막 승격한 항목이 사라져 있을 수 있다(반환값 비결정성). push 직후의 스냅샷을
/// 반환해 호출부가 '방금 승격된 상태'를 결정적으로 받게 한다(이슈 #181).
fn promote_one<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    now: &JsonStore<QueueNowItem>,
    scheduled: &JsonStore<QueueScheduledItem>,
    runner: &super::queue_runner::NowQueueRunner,
    id: &str,
) -> Option<Vec<QueueNowItem>> {
    // scheduled 스토어 락 안에서 원자적으로 꺼낸다: 매칭 id가 있으면 제거하며 그 항목을
    // `taken`에 옮기고, 없으면 그대로 둔다. 수동 "즉시 처리"와 자동 스케줄러(run_due_now)는
    // 별개 스레드에서 같은 id를 동시에 승격하려 할 수 있는데, 락 안의 take-and-remove로
    // 실제로 제거한 쪽만 Some을 받게 해 now 큐 중복 push(=중복 게시)를 막는다.
    let mut taken: Option<QueueScheduledItem> = None;
    scheduled.mutate(|items| {
        let mut kept = Vec::with_capacity(items.len());
        for s in items {
            if taken.is_none() && s.id == id {
                taken = Some(s);
            } else {
                kept.push(s);
            }
        }
        kept
    });
    let s = taken?;
    // 워커 기동(start_if_idle) 전에 스냅샷을 잡는다 — 기동 후 워커가 큐를 비우면 반환값이
    // 비결정적이 되므로(이슈 #181), push 결과(mutate 반환)를 그대로 돌려준다.
    let after = now.mutate(|mut items| {
        items.push(to_now_item(s));
        // 승격된 종토/로그인도 자동 우선순위로 위로 올라가게 재정렬한다(#229).
        apply_priority_order(items)
    });
    super::queue_runner::start_if_idle(runner, app.clone());
    Some(after)
}

/// Move a scheduled item into the immediate queue ("즉시 처리"): drop it from the
/// scheduled store, append it to the now store, and return the updated now list.
/// now 큐에 작업이 생기면 실행 워커를 기동한다(이미 돌고 있으면 무시).
#[tauri::command]
pub fn promote_queue_scheduled<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    now: tauri::State<'_, JsonStore<QueueNowItem>>,
    scheduled: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    runner: tauri::State<'_, super::queue_runner::NowQueueRunner>,
    id: String,
) -> Vec<QueueNowItem> {
    match promote_one(&app, now.inner(), scheduled.inner(), runner.inner(), &id) {
        Some(after) => {
            record(
                activity.inner(),
                ActivityType::Info,
                "예약을 즉시 게시로 전환",
            );
            after
        }
        // 매칭 id가 없으면(이미 처리/취소됨) 현재 now 큐를 그대로 돌려준다.
        None => now.snapshot(),
    }
}

/// 예약 시각을 변경한다(재예약). 놓친(missed) 예약을 새 시각으로 되살리거나, 대기 중인
/// 예약의 시각을 바꾸는 데 쓴다. 과거 시각은 거부(add와 동일 가드). missed는 해제된다.
#[tauri::command]
pub fn reschedule_queue_scheduled(
    store: tauri::State<'_, JsonStore<QueueScheduledItem>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
    at: i64,
    when: String,
    rel: String,
) -> Result<Vec<QueueScheduledItem>, String> {
    if !is_future_enough(at, crate::util::now_ms()) {
        return Err("예약 시각이 현재보다 과거입니다".into());
    }
    // 매칭되는 예약이 있었는지 락 안에서 확인한다. apply_reschedule는 비매칭 id면 리스트를
    // 그대로 돌려주므로, 그것만으로는 성공/실패를 구분할 수 없다 — 동시 tick/취소로 막
    // 사라진 id나 잘못된 id에 대해 "변경됨"이라는 거짓 성공을 남기지 않도록 분기한다.
    let mut matched = false;
    let next = store.mutate(|items| {
        let next = apply_reschedule(items, &id, at, &when, &rel);
        matched = next.iter().any(|s| s.id == id);
        next
    });
    if !matched {
        return Err("재예약할 예약을 찾지 못했어요".into());
    }
    record(activity.inner(), ActivityType::Info, "예약 시각 변경됨");
    Ok(next)
}

/// 지금 게시해야 할 예약(`pick_due`)을 모두 now 큐로 승격하고 자동 게시 활동을 남긴다.
/// 스케줄러 티커가 매 tick 호출한다. 동기 함수라 State 가드를 await 너머로 들지 않는다.
pub fn run_due_now<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;

    let scheduled = app.state::<JsonStore<QueueScheduledItem>>();
    let now = app.state::<JsonStore<QueueNowItem>>();
    let activity = app.state::<JsonStore<crate::ipc::activity::ActivityItem>>();
    let runner = app.state::<super::queue_runner::NowQueueRunner>();

    let snap = scheduled.snapshot();
    for id in pick_due(&snap, crate::util::now_ms()) {
        if promote_one(app, now.inner(), scheduled.inner(), runner.inner(), &id).is_some() {
            let title = snap
                .iter()
                .find(|s| s.id == id)
                .map_or("", |s| s.title.as_str());
            record(
                activity.inner(),
                ActivityType::Info,
                format!("예약 시각 도래 — 자동 게시: {title}"),
            );
        }
    }
}

/// 앱 시작 reconciliation: 앱 종료 중 시각이 지난 미발행 예약을 missed로 표시하고(자동
/// 게시하지 않음) 새로 놓친 건이 있으면 사용자에게 알림으로 남긴다. 티커 spawn 전에
/// 동기로 호출해, 첫 tick이 이미 missed로 표시된 항목을 자동 게시하지 않게 한다.
///
/// 의도된 경계: 직전 1분(`MISSED_GRACE_MS`) 내에 도래한 예약은 여기서 missed로 보지 않고
/// (mark_missed 유예), 첫 tick의 `pick_due`가 자동 게시한다. 즉 "앱이 꺼진 동안 지난 예약"
/// 중 마지막 1분 안짝 건은 놓침이 아니라 즉시 게시되는 게 정상이다.
pub fn reconcile_missed_on_startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;

    let scheduled = app.state::<JsonStore<QueueScheduledItem>>();
    let activity = app.state::<JsonStore<crate::ipc::activity::ActivityItem>>();

    let before = scheduled.snapshot().iter().filter(|s| s.missed).count();
    let after = scheduled.mutate(|items| mark_missed(items, crate::util::now_ms()));
    let newly = after
        .iter()
        .filter(|s| s.missed)
        .count()
        .saturating_sub(before);
    if newly > 0 {
        record(
            activity.inner(),
            ActivityType::Error,
            format!("예약 {newly}건이 앱 종료 중 시각이 지나 미발행됐습니다. 큐에서 재예약하거나 취소하세요."),
        );
        // 창을 닫아둔(트레이) 사용자가 시작 시 놓침을 인지하도록 OS 토스트도 띄운다(#163).
        let (toast_title, toast_body) = super::notify::missed_message(newly);
        super::notify::notify_desktop(app, &toast_title, &toast_body);
    }
}

/// 스케줄러 검사 주기(초). 예약 시각보다 최대 이만큼 늦게 게시될 수 있다(게시 용도엔 충분).
const SCHEDULER_TICK_SECS: u64 = 30;

/// 백그라운드 예약 스케줄러. 앱 시작 시 한 번 spawn되어 앱 수명 동안 `SCHEDULER_TICK_SECS`
/// 마다 예약 시각이 도래한 아이템을 자동 게시한다(`run_due_now`). interval의 첫 tick은
/// 즉시 발화하지만, 시작 reconciliation이 먼저 끝나므로 미발행 예약을 잘못 게시하지 않는다.
pub async fn scheduler_loop<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(SCHEDULER_TICK_SECS));
    loop {
        tick.tick().await;
        run_due_now(&app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(p: PlatformId, name: &str, code: Option<&str>) -> QueueLocation {
        QueueLocation {
            p,
            name: name.into(),
            code: code.map(|c| c.into()),
        }
    }

    fn sample_now_item(id: &str, state: QueueState) -> QueueNowItem {
        QueueNowItem {
            id: id.into(),
            title: format!("글 {id}"),
            kind: ModeValue::Post,
            state,
            batch_id: None,
            progress: None,
            locs: vec![loc(PlatformId::Naver, "테스트 카페", None)],
            plan: None,
            items: Vec::new(),
        }
    }

    fn sample_scheduled_item(id: &str) -> QueueScheduledItem {
        QueueScheduledItem {
            id: id.into(),
            title: format!("예약 {id}"),
            kind: ModeValue::Both,
            when: "오늘 18:30".into(),
            rel: "5시간 후".into(),
            at: 1_700_000_000_000,
            missed: false,
            locs: vec![loc(PlatformId::Forum, "에코프로", Some("086520"))],
            plan: None,
        }
    }

    fn sample_plan() -> PublishPlan {
        PublishPlan {
            post_id: "p1".into(),
            kind: ModeValue::Both,
            title: "제목".into(),
            body_text: "본문".into(),
            comments: vec!["댓글1".into()],
            link_override: String::new(),
            naver: vec![NaverTarget {
                account_id: "user01".into(),
                cafe: "12345".into(),
                cafe_name: "주식투자연구소".into(),
                menu_id: 7,
                board_type: "L".into(),
                comment_target: Some(CommentTargetSpec {
                    mode: CommentTarget::Latest,
                    count: Some(3),
                    cafe_id: None,
                    article_id: None,
                }),
            }],
            forum: vec![ForumTarget {
                account_id: "user01".into(),
                name: "삼성전자".into(),
                code: "005930".into(),
            }],
            band: vec![BandTarget {
                account_id: "user01".into(),
                name: "투자밴드".into(),
                link: "https://band.us/band/12345678".into(),
                comment_target: None,
            }],
            login: None,
        }
    }

    fn sample_login_target(account: &str, platform: PlatformId) -> LoginTarget {
        LoginTarget {
            account_id: account.into(),
            platform,
            headless: false,
            use_adb: false,
            force: true,
        }
    }

    #[test]
    fn cancel_now_removes_by_id() {
        let items = vec![
            sample_now_item("q1", QueueState::Running),
            sample_now_item("q2", QueueState::Waiting),
        ];
        let next = apply_cancel_now(items, "q1");
        assert!(next.iter().all(|i| i.id != "q1"));
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn promote_maps_scheduled_to_a_waiting_now_item() {
        let mut s = sample_scheduled_item("qs1");
        s.plan = Some(sample_plan());
        let id = s.id.clone();
        let (title, locs_len) = (s.title.clone(), s.locs.len());
        let now = to_now_item(s);
        assert_eq!(now.id, id);
        assert_eq!(now.title, title);
        assert_eq!(now.state, QueueState::Waiting);
        assert!(now.batch_id.is_none() && now.progress.is_none());
        assert_eq!(now.locs.len(), locs_len);
        // 실행 페이로드(plan)는 승격 시 보존돼야 워커가 게시할 수 있다(이슈 #142).
        assert_eq!(now.plan, Some(sample_plan()));
    }

    #[test]
    fn as_fresh_now_item_forces_waiting_and_clears_exec_meta() {
        // 즉시 처리 대기열 적재(add_queue_now)는 프론트가 보낸 상태와 무관하게 대기 상태로
        // 시작하고, 워커가 채울 실행 메타(batch_id/progress)는 비운다.
        let mut item = sample_now_item("q1", QueueState::Running);
        item.batch_id = Some("b1".into());
        item.progress = Some((2, 5));
        let title = item.title.clone();
        let fresh = as_fresh_now_item(item);
        assert_eq!(fresh.state, QueueState::Waiting);
        assert_eq!(fresh.batch_id, None);
        assert_eq!(fresh.progress, None);
        // 표시·실행에 필요한 나머지 필드(title 등)는 그대로 보존한다.
        assert_eq!(fresh.title, title);
    }

    #[test]
    fn reorder_now_follows_ordered_ids_but_pins_running_first() {
        let items = vec![
            sample_now_item("r1", QueueState::Running),
            sample_now_item("w1", QueueState::Waiting),
            sample_now_item("w2", QueueState::Waiting),
            sample_now_item("w3", QueueState::Waiting),
        ];
        // 대기 아이템을 w3, w1, w2 순으로 재배열 요청. Running(r1)은 맨 앞 고정.
        let next = apply_reorder_now(
            items,
            &["w3".to_string(), "w1".to_string(), "w2".to_string()],
        );
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["r1", "w3", "w1", "w2"]);
    }

    #[test]
    fn reorder_now_ignores_unknown_ids_and_preserves_missing() {
        let items = vec![
            sample_now_item("w1", QueueState::Waiting),
            sample_now_item("w2", QueueState::Waiting),
            sample_now_item("w3", QueueState::Waiting),
        ];
        // 알 수 없는 id("zzz")는 무시, ordered_ids에 빠진 w3는 원래 순서대로 뒤에 보존.
        let next = apply_reorder_now(
            items,
            &["w2".to_string(), "zzz".to_string(), "w1".to_string()],
        );
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["w2", "w1", "w3"]);
    }

    #[test]
    fn reorder_now_keeps_multiple_running_at_front_in_original_order() {
        let items = vec![
            sample_now_item("w1", QueueState::Waiting),
            sample_now_item("r1", QueueState::Running),
            sample_now_item("r2", QueueState::Running),
        ];
        // running이 여러 개여도 원래 순서대로 맨 앞에 모인다.
        let next = apply_reorder_now(items, &["w1".to_string()]);
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["r1", "r2", "w1"]);
    }

    // --- 자동 우선순위(#229): 로그인 1 > 종토 2 > 카페/밴드 FIFO ---

    fn empty_plan() -> PublishPlan {
        PublishPlan {
            post_id: "p".into(),
            kind: ModeValue::Post,
            title: "t".into(),
            body_text: "b".into(),
            comments: vec![],
            link_override: String::new(),
            naver: vec![],
            forum: vec![],
            band: vec![],
            login: None,
        }
    }

    fn now_item_with(id: &str, state: QueueState, plan: PublishPlan) -> QueueNowItem {
        let mut it = sample_now_item(id, state);
        it.plan = Some(plan);
        it
    }

    fn login_only_plan() -> PublishPlan {
        let mut p = empty_plan();
        p.login = Some(vec![sample_login_target("u", PlatformId::Naver)]);
        p
    }

    fn forum_plan() -> PublishPlan {
        let mut p = empty_plan();
        p.forum = vec![ForumTarget {
            account_id: "u".into(),
            name: "삼성전자".into(),
            code: "005930".into(),
        }];
        p
    }

    fn cafe_plan() -> PublishPlan {
        let mut p = empty_plan();
        p.naver = sample_plan().naver;
        p
    }

    fn band_plan() -> PublishPlan {
        let mut p = empty_plan();
        p.band = sample_plan().band;
        p
    }

    #[test]
    fn item_priority_login_only_is_highest() {
        // 로그인 전용(plan.login 있고 게시 타깃 없음) = 0(최우선).
        let it = now_item_with("l", QueueState::Waiting, login_only_plan());
        assert_eq!(item_priority(&it), 0);
    }

    #[test]
    fn item_priority_forum_is_second() {
        let it = now_item_with("f", QueueState::Waiting, forum_plan());
        assert_eq!(item_priority(&it), 1);
    }

    #[test]
    fn item_priority_cafe_and_band_are_lowest() {
        assert_eq!(
            item_priority(&now_item_with("c", QueueState::Waiting, cafe_plan())),
            2
        );
        assert_eq!(
            item_priority(&now_item_with("b", QueueState::Waiting, band_plan())),
            2
        );
    }

    #[test]
    fn item_priority_mixed_with_forum_is_promoted() {
        // 카페+종토 혼합이면 forum이 끼었으므로 2순위(=1)로 승격.
        let mut p = cafe_plan();
        p.forum = forum_plan().forum;
        assert_eq!(
            item_priority(&now_item_with("m", QueueState::Waiting, p)),
            1
        );
    }

    #[test]
    fn item_priority_login_attached_to_publish_is_not_login_only() {
        // 게시(카페)에 로그인이 동봉된 아이템은 로그인 전용이 아니라 그 게시 등급(카페=2).
        let mut p = cafe_plan();
        p.login = Some(vec![sample_login_target("u", PlatformId::Naver)]);
        assert_eq!(
            item_priority(&now_item_with("p", QueueState::Waiting, p)),
            2
        );
    }

    #[test]
    fn item_priority_no_plan_is_lowest() {
        assert_eq!(item_priority(&sample_now_item("x", QueueState::Waiting)), 2);
    }

    #[test]
    fn priority_order_login_then_forum_then_cafe_band_fifo() {
        // 들어온 순서: 카페c1, 밴드b1, 종토f1, 로그인l1, 카페c2, 종토f2.
        // 기대: 로그인 → 종토(FIFO f1,f2) → 카페/밴드(FIFO c1,b1,c2).
        let items = vec![
            now_item_with("c1", QueueState::Waiting, cafe_plan()),
            now_item_with("b1", QueueState::Waiting, band_plan()),
            now_item_with("f1", QueueState::Waiting, forum_plan()),
            now_item_with("l1", QueueState::Waiting, login_only_plan()),
            now_item_with("c2", QueueState::Waiting, cafe_plan()),
            now_item_with("f2", QueueState::Waiting, forum_plan()),
        ];
        let next = apply_priority_order(items);
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["l1", "f1", "f2", "c1", "b1", "c2"]);
    }

    #[test]
    fn priority_order_pins_running_first_even_if_lower_priority() {
        // 실행 중(Running) 카페는 더 높은 우선순위 로그인보다도 맨 앞 고정(실행 중은 안 건드림).
        let items = vec![
            now_item_with("r1", QueueState::Running, cafe_plan()),
            now_item_with("l1", QueueState::Waiting, login_only_plan()),
            now_item_with("c1", QueueState::Waiting, cafe_plan()),
        ];
        let next = apply_priority_order(items);
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["r1", "l1", "c1"]);
    }

    #[test]
    fn priority_order_preserves_fifo_within_same_class() {
        // 같은 등급(카페)끼리는 들어온 순서·수동 순서를 그대로 보존(안정 정렬).
        let items = vec![
            now_item_with("c1", QueueState::Waiting, cafe_plan()),
            now_item_with("c2", QueueState::Waiting, cafe_plan()),
            now_item_with("c3", QueueState::Waiting, cafe_plan()),
        ];
        let next = apply_priority_order(items);
        let ids: Vec<&str> = next.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["c1", "c2", "c3"]);
    }

    // --- 실행 중 작업 중지/재개(#232): 삭제 아님 = Waiting 복귀 + 잔여 plan ---

    #[test]
    fn apply_yield_now_returns_to_waiting_with_remaining_plan_and_reorders() {
        // 실행 중 카페 c1 + 대기 종토 f1. c1을 (잔여=카페) plan으로 양보.
        let remaining = cafe_plan();
        let items = vec![
            now_item_with("c1", QueueState::Running, cafe_plan()),
            now_item_with("f1", QueueState::Waiting, forum_plan()),
        ];
        let after = apply_yield_now(items, "c1", remaining.clone());
        // 양보한 c1은 Waiting으로 내려가고, 종토 f1(2순위=1)이 위로 올라간다(둘 다 Waiting).
        let ids: Vec<&str> = after.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["f1", "c1"]);
        let c1 = after.iter().find(|i| i.id == "c1").unwrap();
        assert_eq!(c1.state, QueueState::Waiting);
        assert_eq!(c1.plan.as_ref(), Some(&remaining));
        assert_eq!(c1.progress, None);
        assert_eq!(c1.batch_id, None);
        assert!(c1.items.is_empty());
    }

    #[test]
    fn apply_yield_now_unknown_id_is_noop_no_resurrection() {
        // 양보 직전 사용자가 취소(아이템 제거)했으면, 없는 id로의 yield는 아무것도 되살리지 않는다.
        let items = vec![now_item_with("f1", QueueState::Waiting, forum_plan())];
        let after = apply_yield_now(items, "gone", cafe_plan());
        let ids: Vec<&str> = after.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["f1"]);
    }

    #[test]
    fn future_guard_uses_minute_precision() {
        let now: i64 = 1_700_000_045_000; // ...:45s within a minute
        let minute_start = now - now.rem_euclid(60_000);
        assert!(is_future_enough(minute_start, now)); // current minute allowed
        assert!(is_future_enough(now + 60_000, now)); // future allowed
        assert!(!is_future_enough(minute_start - 1, now)); // earlier rejected
    }

    #[test]
    fn cancel_scheduled_removes_by_id() {
        let next = apply_cancel_scheduled(vec![sample_scheduled_item("qs1")], "qs1");
        assert!(next.is_empty());
    }

    #[test]
    fn seeds_start_empty() {
        // 게시 큐는 빈 상태로 시작한다(데모 시드 제거, 이슈 #142).
        assert!(seed_now().is_empty());
        assert!(seed_scheduled().is_empty());
    }

    #[test]
    fn item_roundtrips_through_json() {
        let mut item = sample_now_item("q1", QueueState::Running);
        item.batch_id = Some("b0".into());
        item.progress = Some((2, 3));
        item.plan = Some(sample_plan());
        let back: QueueNowItem =
            serde_json::from_str(&serde_json::to_string(&item).unwrap()).unwrap();
        assert_eq!(item, back);
    }

    #[test]
    fn running_item_serializes_camelcase_progress_tuple() {
        let mut item = sample_now_item("q1", QueueState::Running);
        item.batch_id = Some("b0".into());
        item.progress = Some((2, 3));
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"progress\":[2,3]"));
        assert!(json.contains("\"batchId\":\"b0\""));
        assert!(json.contains("\"state\":\"running\""));
    }

    #[test]
    fn login_omitted_when_none_but_present_camelcase_when_set() {
        // 게시 plan은 login이 None이라 직렬화에서 키가 생략돼 기존 plan과 호환된다.
        let no_login = sample_plan();
        assert!(!serde_json::to_string(&no_login)
            .unwrap()
            .contains("\"login\""));

        // 로그인 전용 plan은 login 배열이 camelCase 필드(accountId/useAdb)로 직렬화된다.
        let mut login_plan = sample_plan();
        login_plan.login = Some(vec![
            sample_login_target("user01", PlatformId::Naver),
            sample_login_target("band01", PlatformId::Band),
        ]);
        let json = serde_json::to_string(&login_plan).unwrap();
        assert!(json.contains("\"login\""));
        assert!(json.contains("\"accountId\":\"user01\""));
        assert!(json.contains("\"useAdb\":false"));
        assert!(json.contains("\"platform\":\"band\""));
    }

    #[test]
    fn legacy_plan_without_login_deserializes_to_none() {
        // login 필드가 없는 구버전/게시 plan JSON은 login=None으로 역직렬화된다(하위호환).
        let json = r#"{"postId":"p1","kind":"post","title":"T","bodyText":"B"}"#;
        let plan: PublishPlan = serde_json::from_str(json).unwrap();
        assert_eq!(plan.login, None);
        assert!(plan.naver.is_empty());

        // 로그인 plan 라운드트립도 보존된다.
        let mut full = sample_plan();
        full.login = Some(vec![sample_login_target("u", PlatformId::Naver)]);
        let back: PublishPlan =
            serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert_eq!(full, back);
    }

    #[test]
    fn plan_omitted_when_none_but_present_when_set() {
        // plan이 없으면 직렬화에서 키가 생략돼 기존 아이템과 호환된다.
        let none_item = sample_now_item("q1", QueueState::Waiting);
        assert!(!serde_json::to_string(&none_item)
            .unwrap()
            .contains("\"plan\""));

        // plan이 있으면 camelCase 필드로 직렬화된다.
        let mut some_item = sample_now_item("q2", QueueState::Waiting);
        some_item.plan = Some(sample_plan());
        let json = serde_json::to_string(&some_item).unwrap();
        assert!(json.contains("\"plan\""));
        assert!(json.contains("\"postId\":\"p1\""));
        assert!(json.contains("\"bodyText\":\"본문\""));
        assert!(json.contains("\"commentTarget\""));
    }

    fn sched_at(id: &str, at: i64, missed: bool) -> QueueScheduledItem {
        let mut s = sample_scheduled_item(id);
        s.at = at;
        s.missed = missed;
        s
    }

    #[test]
    fn pick_due_returns_past_non_missed_in_order() {
        let now = 1_000_000;
        let items = vec![
            sched_at("a", now, false),       // 경계: at == now → 포함
            sched_at("b", now + 1, false),   // 미래 → 제외
            sched_at("c", now - 50, true),   // 과거지만 missed → 제외
            sched_at("d", now - 100, false), // 과거 → 포함
        ];
        assert_eq!(pick_due(&items, now), vec!["a", "d"]);
        assert!(pick_due(&[], now).is_empty());
    }

    #[test]
    fn mark_missed_flags_only_past_grace_unmissed() {
        let now = 10_000_000;
        let items = vec![
            sched_at("grace", now - MISSED_GRACE_MS + 1, false), // 유예 내 → 유지
            sched_at("old", now - MISSED_GRACE_MS - 1, false),   // 유예 초과 → missed
            sched_at("already", now - 1_000_000, true),          // 이미 missed → 유지
            sched_at("future", now + 10_000, false),             // 미래 → 유지
            sched_at("legacy", 0, false),                        // 레거시 at=0 → missed
        ];
        let next = mark_missed(items, now);
        let by = |id: &str| next.iter().find(|s| s.id == id).unwrap().missed;
        assert!(!by("grace"));
        assert!(by("old"));
        assert!(by("already"));
        assert!(!by("future"));
        assert!(by("legacy"));
    }

    #[test]
    fn apply_reschedule_updates_time_and_clears_missed() {
        let items = vec![sched_at("a", 100, true), sched_at("b", 200, false)];
        let next = apply_reschedule(items, "a", 999, "내일 09:00", "내일");
        let a = next.iter().find(|s| s.id == "a").unwrap();
        assert_eq!(a.at, 999);
        assert_eq!(a.when, "내일 09:00");
        assert_eq!(a.rel, "내일");
        assert!(!a.missed); // 재예약 시 놓침 해제
                            // 비매칭 아이템은 그대로
        let b = next.iter().find(|s| s.id == "b").unwrap();
        assert_eq!(b.at, 200);
    }

    #[test]
    fn apply_reschedule_no_match_leaves_list_unchanged() {
        // 비매칭 id면 리스트가 그대로다 — reschedule_queue_scheduled가 이를 보고 거짓 성공
        // 대신 에러를 내도록 분기하는 근거(있는 id면 변경되어 달라진다).
        let items = vec![sched_at("a", 100, true), sched_at("b", 200, false)];
        let next = apply_reschedule(items.clone(), "zzz", 999, "내일 09:00", "내일");
        assert_eq!(next, items);
    }

    #[test]
    fn scheduled_item_defaults_at_and_missed_when_absent() {
        // 구버전 JSON(at/missed 없음) → at=0, missed=false 로 역직렬화(serde default).
        let json =
            r#"{"id":"q1","title":"t","kind":"post","when":"오늘 18:30","rel":"오늘","locs":[]}"#;
        let item: QueueScheduledItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.at, 0);
        assert!(!item.missed);
        // 전체 라운드트립도 보존.
        let full = sched_at("q2", 1_700_000_000_000, true);
        let back: QueueScheduledItem =
            serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert_eq!(full, back);
    }
}
