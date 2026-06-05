//! 카페 게시글 목록(최신글/인기글) 모델.
//!
//! 실패킷(2026-06-05, cafeId 31732304·10000260)으로 확정한 스키마다. 최신글과
//! 인기글은 **경로·응답 봉투·필드가 다른 별개 API**라 내부 역직렬화 타입을 둘로
//! 나눈다:
//! - 최신글: `cafe-boardlist-api/v1` → `{ "result": { "articleList": [ {"type","item"} ] } }`
//! - 인기글: `cafe2/WeeklyPopularArticleListV3.json` → `{ "message": { "result": { "articleList": [ … ] } } }`
//!
//! 둘 다 사용자에게는 공통 [`Article`]로 합쳐 노출한다. 작성자 식별용 민감
//! 필드(`memberKey`·`maskedMemberId`)는 내부 구조체에 선언하지 않아 자동 배제된다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};

/// 게시글 목록 조회 오류 타입 — 공통 오류 봉투 재사용.
pub type ArticleListError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// 게시글 목록 정렬 기준 — 최신글 / 인기글.
///
/// 둘은 호출하는 API 자체가 다르다(최신글=boardlist, 인기글=주간 인기글 V3).
/// 인기글의 "댓글 TOP / 좋아요 TOP"은 추후 변형으로 확장할 수 있다.
// 주의: 이 파일은 다른 바인딩 소스(src/ipc)보다 한 단계 깊어(article_list/)
// export_to 경로의 `../`가 하나 적다. [[project_ts_rs_export_path]]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    /// 최신글 — 작성 시각 내림차순(boardlist API, `sortBy=TIME`).
    Latest,
    /// 인기글 — 주간 인기글(totalScore 기준, WeeklyPopular API).
    Popular,
}

/// UI로 반환하는 게시글 1건 (slim).
///
/// 최신글·인기글 두 응답을 공통 형태로 합친다. 최신글 응답엔 작성자 닉네임이
/// 없어(`writerInfo.memberKey`만 존재) `writer_nickname`이 빈 문자열이고,
/// 인기글 응답엔 게시판 이름이 없어 `menu_name`이 빈 문자열이다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Article {
    /// 숫자 게시글 ID — 댓글 대상이 되는 값.
    #[ts(type = "number")]
    pub article_id: u64,
    /// 게시글 제목.
    pub subject: String,
    /// 작성자 닉네임 (최신글 응답엔 없어 빈 문자열).
    pub writer_nickname: String,
    /// 글이 속한 게시판(메뉴) ID.
    #[ts(type = "number")]
    pub menu_id: u64,
    /// 게시판 이름 (인기글 응답엔 없어 빈 문자열).
    pub menu_name: String,
    /// 댓글 수.
    #[ts(type = "number")]
    pub comment_count: u64,
    /// 조회 수.
    #[ts(type = "number")]
    pub read_count: u64,
    /// 좋아요 수 (최신글 `likeCount` / 인기글 `upCount`).
    #[ts(type = "number")]
    pub like_count: u64,
    /// 작성 시각(epoch millis).
    #[ts(type = "number")]
    pub write_date_timestamp: u64,
}

/// 게시글 목록 조회 결과 — UI 반환용.
///
/// 실제 응답엔 페이지네이션 정보가 없어(최신글은 단일 페이지, 인기글은 전체
/// 랭킹) 목록만 담는다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ArticleListResponse {
    /// 조회된 게시글 목록.
    pub articles: Vec<Article>,
}

// ---------------------------------------------------------------------------
// 최신글 응답 (boardlist-api) — { "result": { "articleList": [ {"type","item"} ] } }
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BoardListEnvelope {
    pub result: BoardListResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BoardListResult {
    #[serde(default)]
    pub article_list: Vec<BoardListEntry>,
}

/// 목록 원소 — 글은 `{ "type": "ARTICLE", "item": {…} }`로 감싸여 온다.
/// (광고 등 다른 type이 섞일 수 있어 변환 시 ARTICLE만 추린다.)
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct BoardListEntry {
    #[serde(rename = "type", default)]
    pub entry_type: String,
    pub item: BoardListItem,
}

/// 최신글 item — 필요한 키만 선언(민감 필드 memberKey 등은 무시됨).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BoardListItem {
    pub article_id: u64,
    // 표시 전용 필드는 블라인드/탈퇴 글에서 누락될 수 있어 기본값을 허용한다.
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub menu_id: u64,
    #[serde(default)]
    pub menu_name: String,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub read_count: u64,
    #[serde(default)]
    pub like_count: u64,
    #[serde(default)]
    pub write_date_timestamp: u64,
}

impl From<BoardListItem> for Article {
    fn from(it: BoardListItem) -> Self {
        Article {
            article_id: it.article_id,
            subject: it.subject,
            // 최신글 응답엔 닉네임이 없다(writerInfo.memberKey만 — 민감, 미노출).
            writer_nickname: String::new(),
            menu_id: it.menu_id,
            menu_name: it.menu_name,
            comment_count: it.comment_count,
            read_count: it.read_count,
            like_count: it.like_count,
            write_date_timestamp: it.write_date_timestamp,
        }
    }
}

impl BoardListEnvelope {
    /// 응답을 UI 반환용 [`ArticleListResponse`]로 변환한다(ARTICLE만 추림).
    pub(crate) fn into_response(self) -> ArticleListResponse {
        let articles = self
            .result
            .article_list
            .into_iter()
            .filter(|e| e.entry_type == "ARTICLE")
            .map(|e| Article::from(e.item))
            .collect();
        ArticleListResponse { articles }
    }
}

// ---------------------------------------------------------------------------
// 인기글 응답 (WeeklyPopular) — { "message": { "result": { "articleList": [ … ] } } }
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PopularEnvelope {
    pub message: PopularMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PopularMessage {
    pub result: PopularResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopularResult {
    #[serde(default)]
    pub article_list: Vec<PopularItem>,
}

/// 인기글 item — 최신글과 키가 다르다(`nickname`·`upCount`). 점수 필드
/// (`totalScore`)·민감 필드(`memberKey`/`maskedMemberId`)는 노출하지 않는다.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PopularItem {
    pub article_id: u64,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub menu_id: u64,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub read_count: u64,
    /// 좋아요/추천 수.
    #[serde(default)]
    pub up_count: u64,
    #[serde(default)]
    pub write_date_timestamp: u64,
}

impl From<PopularItem> for Article {
    fn from(it: PopularItem) -> Self {
        Article {
            article_id: it.article_id,
            subject: it.subject,
            writer_nickname: it.nickname,
            menu_id: it.menu_id,
            // 인기글 응답엔 게시판 이름이 없다.
            menu_name: String::new(),
            comment_count: it.comment_count,
            read_count: it.read_count,
            like_count: it.up_count,
            write_date_timestamp: it.write_date_timestamp,
        }
    }
}

impl PopularEnvelope {
    /// 응답을 UI 반환용 [`ArticleListResponse`]로 변환한다.
    pub(crate) fn into_response(self) -> ArticleListResponse {
        let articles = self
            .message
            .result
            .article_list
            .into_iter()
            .map(Article::from)
            .collect();
        ArticleListResponse { articles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATEST_FIXTURE: &str = include_str!("fixtures/article_list_latest_success.json");
    const POPULAR_FIXTURE: &str = include_str!("fixtures/article_list_popular_success.json");

    #[test]
    fn sort_by_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&SortBy::Latest).expect("직렬화 실패"),
            "\"latest\""
        );
        assert_eq!(
            serde_json::to_string(&SortBy::Popular).expect("직렬화 실패"),
            "\"popular\""
        );
    }

    #[test]
    fn article_serializes_camel_case() {
        let article = Article {
            article_id: 1024,
            subject: "공지".to_string(),
            writer_nickname: "관리자".to_string(),
            menu_id: 1,
            menu_name: "자유게시판".to_string(),
            comment_count: 12,
            read_count: 345,
            like_count: 7,
            write_date_timestamp: 1_717_400_000_000,
        };
        let json = serde_json::to_value(&article).expect("직렬화 실패");
        assert!(json.get("articleId").is_some(), "articleId 키가 없음");
        assert!(json.get("likeCount").is_some(), "likeCount 키가 없음");
        assert!(
            json.get("article_id").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
    }

    // ------------------------------------------------------------------
    // 최신글(boardlist) 실측 fixture
    // ------------------------------------------------------------------

    #[test]
    fn parses_latest_fixture_into_response() {
        let envelope: BoardListEnvelope =
            serde_json::from_str(LATEST_FIXTURE).expect("최신글 fixture 역직렬화 실패");
        let response = envelope.into_response();

        assert_eq!(response.articles.len(), 2, "게시글 2건이어야 함");
        let first = &response.articles[0];
        assert_eq!(first.article_id, 12);
        assert_eq!(first.subject, "Hello Java");
        assert_eq!(first.menu_id, 1);
        assert_eq!(first.menu_name, "자유게시판");
        assert_eq!(first.comment_count, 3);
        // 최신글 응답엔 닉네임이 없다.
        assert_eq!(first.writer_nickname, "");
    }

    #[test]
    fn latest_does_not_leak_member_key() {
        // writerInfo.memberKey 등 민감 필드는 직렬화 결과에 나오면 안 된다.
        let envelope: BoardListEnvelope =
            serde_json::from_str(LATEST_FIXTURE).expect("역직렬화 실패");
        let json = serde_json::to_string(&envelope.into_response()).expect("직렬화 실패");
        assert!(!json.contains("memberKey"), "memberKey가 노출됨");
        assert!(!json.contains("Nokk1968"), "memberKey 값이 노출됨");
    }

    #[test]
    fn latest_skips_non_article_entries() {
        // ARTICLE이 아닌 type(광고 등)은 제외한다.
        let raw = r#"{ "result": { "articleList": [
            { "type": "AD", "item": { "articleId": 99 } },
            { "type": "ARTICLE", "item": { "articleId": 1, "subject": "글" } }
        ] } }"#;
        let envelope: BoardListEnvelope = serde_json::from_str(raw).expect("역직렬화 실패");
        let response = envelope.into_response();
        assert_eq!(response.articles.len(), 1, "ARTICLE만 남아야 함");
        assert_eq!(response.articles[0].article_id, 1);
    }

    #[test]
    fn latest_tolerates_empty_board() {
        // 빈 게시판은 articleList가 빈 배열인 200을 준다.
        let raw = r#"{ "result": { "articleList": [] } }"#;
        let envelope: BoardListEnvelope = serde_json::from_str(raw).expect("역직렬화 실패");
        assert!(envelope.into_response().articles.is_empty());
    }

    // ------------------------------------------------------------------
    // 인기글(WeeklyPopular) 실측 fixture
    // ------------------------------------------------------------------

    #[test]
    fn parses_popular_fixture_into_response() {
        let envelope: PopularEnvelope =
            serde_json::from_str(POPULAR_FIXTURE).expect("인기글 fixture 역직렬화 실패");
        let response = envelope.into_response();

        assert_eq!(response.articles.len(), 2, "인기글 2건이어야 함");
        let first = &response.articles[0];
        assert_eq!(first.article_id, 3075152);
        assert_eq!(first.subject, "아이들에게 써벨로 S5란?");
        // 인기글은 nickname → writer_nickname, upCount → like_count.
        assert_eq!(first.writer_nickname, "미여기");
        assert_eq!(first.like_count, 20);
        assert_eq!(first.comment_count, 37);
        assert_eq!(first.read_count, 613);
        // 인기글 응답엔 게시판 이름이 없다.
        assert_eq!(first.menu_name, "");
    }

    #[test]
    fn popular_does_not_leak_member_identifiers() {
        let envelope: PopularEnvelope =
            serde_json::from_str(POPULAR_FIXTURE).expect("역직렬화 실패");
        let json = serde_json::to_string(&envelope.into_response()).expect("직렬화 실패");
        assert!(!json.contains("memberKey"), "memberKey가 노출됨");
        assert!(!json.contains("maskedMemberId"), "maskedMemberId가 노출됨");
        assert!(!json.contains("totalScore"), "totalScore가 노출됨");
    }
}
