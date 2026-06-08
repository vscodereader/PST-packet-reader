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
pub fn is_future_enough(at: i64, now: i64) -> bool {
    at >= now - now.rem_euclid(60_000)
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
    let found = scheduled.snapshot().into_iter().find(|s| s.id == id);
    match found {
        Some(s) => {
            scheduled.mutate(|items| apply_cancel_scheduled(items, &id));
            let next = now.mutate(|mut items| {
                items.push(to_now_item(s));
                items
            });
            record(
                activity.inner(),
                ActivityType::Info,
                "예약을 즉시 게시로 전환",
            );
            super::queue_runner::start_if_idle(&runner, app);
            next
        }
        None => now.snapshot(),
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
}
