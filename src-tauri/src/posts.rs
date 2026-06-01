//! Posts (글 보관함) domain — JSON-file-backed, wired over Tauri IPC.
//!
//! ts-rs generates the TS types into `src/shared/bindings/`. Optional fields use
//! `#[ts(optional)]` so they emit `body?: T` (matching the frontend interface
//! under exactOptionalPropertyTypes); serde skips them when None.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::store::JsonStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ModeValue {
    Post,
    Comment,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    Draft,
    Ready,
    Scheduled,
    Published,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum CommentTarget {
    Latest,
    Popular,
    Url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment_count: Option<u32>,
}

// --------------------------------------------------------------------------
// Pure logic
// --------------------------------------------------------------------------

/// Replace the post with a matching id, or prepend it if new (newest first).
pub fn apply_upsert(posts: Vec<LibraryPost>, post: LibraryPost) -> Vec<LibraryPost> {
    if posts.iter().any(|p| p.id == post.id) {
        posts
            .into_iter()
            .map(|p| if p.id == post.id { post.clone() } else { p })
            .collect()
    } else {
        let mut next = Vec::with_capacity(posts.len() + 1);
        next.push(post);
        next.extend(posts);
        next
    }
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
            comments: Some(vec!["소재주 흐름 좋네요".into(), "장기적으로 봅니다".into()]),
            comment_target: Some(CommentTarget::Popular),
            comment_url: None,
            comment_count: None,
        },
    ]
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
    post: LibraryPost,
) -> Vec<LibraryPost> {
    store.mutate(|posts| apply_upsert(posts, post))
}

#[tauri::command]
pub fn delete_post(
    store: tauri::State<'_, JsonStore<LibraryPost>>,
    id: String,
) -> Vec<LibraryPost> {
    store.mutate(|posts| apply_delete(posts, &id))
}

#[cfg(test)]
mod tests {
    use super::*;

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
