//! Posts (글 보관함) domain — JSON-file-backed, wired over Tauri IPC.
//!
//! ts-rs generates the TS types into `src/shared/bindings/`. Optional fields use
//! `#[ts(optional)]` so they emit `body?: T` (matching the frontend interface
//! under exactOptionalPropertyTypes); serde skips them when None.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::activity::{record, ActivityType};
use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ModeValue {
    Post,
    Comment,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    Draft,
    Ready,
    Scheduled,
    Published,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CommentTarget {
    Latest,
    Popular,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LibraryPost {
    pub id: String,
    pub title: String,
    pub kind: ModeValue,
    pub updated: String,
    pub words: u32,
    pub status: PostStatus,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comments: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_target: Option<CommentTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_url: Option<String>,
    /// 특정 게시글(url) 댓글의 대상 링크들(여러 개). 프론트에서 여러 링크를 넣으면 각 링크의
    /// 글마다 댓글을 단다. 예전엔 이 필드가 백엔드 구조체에 없어 저장 시 serde가 버렸고, 다시
    /// 열면 `comment_url`(단수) 1개로 줄어들었다(2026-07-01 버그). `comment_url`은 하위호환으로
    /// 유지하며 항상 `comment_urls[0]`과 같은 값을 담는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_urls: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_count: Option<u32>,
}

// --------------------------------------------------------------------------
// Pure logic
// --------------------------------------------------------------------------

/// Replace the post with a matching id, or prepend it if new (newest first).
pub fn apply_upsert(mut posts: Vec<LibraryPost>, post: LibraryPost) -> Vec<LibraryPost> {
    match posts.iter_mut().find(|p| p.id == post.id) {
        Some(slot) => *slot = post,    // update in place — single pass, no clone
        None => posts.insert(0, post), // new → prepend (newest first)
    }
    posts
}

pub fn apply_delete(posts: Vec<LibraryPost>, id: &str) -> Vec<LibraryPost> {
    posts.into_iter().filter(|p| p.id != id).collect()
}

pub fn seed() -> Vec<LibraryPost> {
    vec![
        LibraryPost {
            id: "l1".into(),
            title: "#{종목명} 4분기 실적 기대 — 매수 관점 정리".into(),
            kind: ModeValue::Post,
            updated: "방금 전".into(),
            words: 280,
            status: PostStatus::Ready,
            excerpt: "#{종목명}에 외국인 순매수가 다시 들어오고 있습니다.".into(),
            body: Some("<p>#{종목명}에 외국인 순매수가 다시 들어오고 있습니다.</p>".into()),
            comments: None,
            comment_target: None,
            comment_url: None,
            comment_urls: None,
            comment_count: None,
        },
        LibraryPost {
            id: "l2".into(),
            title: "반도체 흐름 코멘트 모음 (10종)".into(),
            kind: ModeValue::Comment,
            updated: "30분 전".into(),
            words: 120,
            status: PostStatus::Ready,
            excerpt: "‘오늘 흐름 좋네요’ 등 자연스러운 댓글 10종.".into(),
            body: None,
            comments: Some(vec![
                "오늘 흐름 좋네요 👍".into(),
                "저도 오전에 추가 매수했습니다".into(),
                "관심종목 추가요".into(),
            ]),
            comment_target: Some(CommentTarget::Latest),
            comment_url: None,
            comment_urls: None,
            comment_count: None,
        },
        LibraryPost {
            id: "l3".into(),
            title: "에코프로 조정 구간 대응 전략".into(),
            kind: ModeValue::Both,
            updated: "방금 전".into(),
            words: 842,
            status: PostStatus::Draft,
            excerpt: "단기 변동성이 커진 구간입니다.".into(),
            body: Some("<p>분할 매수 관점으로 접근하는 것이 좋겠습니다.</p>".into()),
            comments: Some(vec!["분할 매수 관점 동의합니다".into()]),
            comment_target: None,
            comment_url: None,
            comment_urls: None,
            comment_count: None,
        },
        LibraryPost {
            id: "l5".into(),
            title: "2차전지 섹터 코멘트 세트".into(),
            kind: ModeValue::Comment,
            updated: "어제".into(),
            words: 90,
            status: PostStatus::Published,
            excerpt: "소재주 중심으로 분위기를 띄우는 댓글 세트입니다.".into(),
            body: None,
            comments: Some(vec![
                "소재주 흐름 좋네요".into(),
                "장기적으로 봅니다".into(),
            ]),
            comment_target: Some(CommentTarget::Popular),
            comment_url: None,
            comment_urls: None,
            comment_count: None,
        },
    ]
}

// --------------------------------------------------------------------------
// Activity message builders (pure, unit-tested).
// --------------------------------------------------------------------------

pub fn saved_msg(title: &str) -> String {
    format!("게시글 '{title}' 저장됨")
}
pub fn deleted_msg(title: &str) -> String {
    format!("게시글 '{title}' 삭제됨")
}

// --------------------------------------------------------------------------
// Commands
// --------------------------------------------------------------------------

#[tauri::command]
pub fn list_posts(store: tauri::State<'_, JsonStore<LibraryPost>>) -> Vec<LibraryPost> {
    store.snapshot()
}

#[tauri::command]
pub fn upsert_post(
    store: tauri::State<'_, JsonStore<LibraryPost>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    post: LibraryPost,
) -> Vec<LibraryPost> {
    if post.title.trim().is_empty() {
        record(
            activity.inner(),
            ActivityType::Error,
            "게시글 저장 실패 — 제목이 비어 있습니다",
        );
        return store.snapshot();
    }
    let title = post.title.clone();
    let next = store.mutate(|posts| apply_upsert(posts, post));
    record(activity.inner(), ActivityType::Success, saved_msg(&title));
    next
}

#[tauri::command]
pub fn delete_post(
    store: tauri::State<'_, JsonStore<LibraryPost>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<LibraryPost> {
    let mut recorded_title = String::new();
    let next = store.mutate(|posts| {
        if let Some(p) = posts.iter().find(|p| p.id == id) {
            recorded_title = p.title.clone();
        }
        apply_delete(posts, &id)
    });
    record(
        activity.inner(),
        ActivityType::Info,
        deleted_msg(&recorded_title),
    );
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_event_messages() {
        assert_eq!(saved_msg("실적 정리"), "게시글 '실적 정리' 저장됨");
        assert_eq!(deleted_msg("실적 정리"), "게시글 '실적 정리' 삭제됨");
    }

    #[test]
    fn comment_urls_survive_serde_round_trip() {
        // 프론트가 보내는 여러 링크(commentUrls)가 저장(역직렬화→직렬화)에서 유지돼야 한다.
        // 예전엔 백엔드 구조체에 comment_urls 필드가 없어 3개 넣어도 1개(commentUrl)로 줄었다.
        let incoming = serde_json::json!({
            "id": "p1",
            "title": "특정 게시글 댓글",
            "kind": "comment",
            "updated": "방금 전",
            "words": 3,
            "status": "ready",
            "excerpt": "x",
            "commentTarget": "url",
            "commentUrl": "https://stock.naver.com/a/discussion/1",
            "commentUrls": [
                "https://stock.naver.com/a/discussion/1",
                "https://stock.naver.com/b/discussion/2",
                "https://stock.naver.com/c/discussion/3",
            ],
        });
        let post: LibraryPost = serde_json::from_value(incoming).expect("역직렬화 성공");
        assert_eq!(post.comment_urls.as_ref().map(Vec::len), Some(3));
        // 저장 후 다시 읽어도 3개 그대로여야 한다(라운드트립).
        let json = serde_json::to_string(&post).expect("직렬화 성공");
        let restored: LibraryPost = serde_json::from_str(&json).expect("재역직렬화 성공");
        assert_eq!(restored.comment_urls, post.comment_urls);
        assert_eq!(restored.comment_urls.map(|v| v.len()), Some(3));
    }

    fn post(id: &str, title: &str) -> LibraryPost {
        LibraryPost {
            id: id.into(),
            title: title.into(),
            kind: ModeValue::Post,
            updated: "방금 전".into(),
            words: 10,
            status: PostStatus::Draft,
            excerpt: "x".into(),
            body: None,
            comments: None,
            comment_target: None,
            comment_url: None,
            comment_urls: None,
            comment_count: None,
        }
    }

    #[test]
    fn upsert_prepends_a_new_post() {
        let next = apply_upsert(vec![post("l1", "one")], post("l2", "two"));
        assert_eq!(next.len(), 2);
        assert_eq!(next[0].id, "l2");
    }

    #[test]
    fn upsert_replaces_existing_in_place() {
        let start = vec![post("l1", "one"), post("l2", "two")];
        let mut edited = post("l2", "renamed");
        edited.status = PostStatus::Published;
        let next = apply_upsert(start, edited);
        assert_eq!(next.len(), 2);
        assert_eq!(next[1].title, "renamed");
        assert_eq!(next[1].status, PostStatus::Published);
    }

    #[test]
    fn delete_removes_by_id() {
        let next = apply_delete(vec![post("l1", "one"), post("l2", "two")], "l1");
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].id, "l2");
    }

    #[test]
    fn seed_roundtrips_through_json() {
        let seeded = seed();
        assert!(!seeded.is_empty());
        let back: Vec<LibraryPost> =
            serde_json::from_str(&serde_json::to_string(&seeded).unwrap()).unwrap();
        assert_eq!(seeded, back);
    }

    #[test]
    fn optional_fields_are_omitted_when_none() {
        let json = serde_json::to_string(&post("l1", "t")).unwrap();
        assert!(!json.contains("body"));
        assert!(!json.contains("comments"));
        assert!(json.contains("\"kind\":\"post\""));
    }
}
