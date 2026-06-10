//! Queue (게시 큐) domain — JSON-file-backed, wired over Tauri IPC. Reuses
//! `PlatformId`/`ModeValue` from the accounts/posts modules so the generated TS
//! bindings stay a single source of truth.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::accounts::PlatformId;
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
    #[serde(default)]
    pub naver: Vec<NaverTarget>,
    #[serde(default)]
    pub forum: Vec<ForumTarget>,
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

/// 대기열 순서를 `ordered_ids`대로 영속화한다(드래그/우선순위 변경). 빈번한 조작이라
/// activity 피드에는 기록하지 않는다.
#[tauri::command]
pub fn reorder_queue_now(
    store: tauri::State<'_, JsonStore<QueueNowItem>>,
    ordered_ids: Vec<String>,
) -> Vec<QueueNowItem> {
    store.mutate(|items| apply_reorder_now(items, &ordered_ids))
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
        items
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
