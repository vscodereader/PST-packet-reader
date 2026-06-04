//! 카페 게시글 목록(최신글/인기글) 모델.
//!
//! ⚠️ 미확인 스키마: 게시글 목록 엔드포인트의 실제 패킷 캡처가 없어
//! ([[#96]] 구현 시점) 요청 URL/파라미터와 응답 봉투를 형제 모듈
//! (menu·joined_cafes)과 네이버 카페 공개 article-list API 관례로 추정했다.
//! 실제 응답으로 검증 후 확정할 것. 사용자에게 노출할 [`Article`]·
//! [`ArticleListResponse`]만 ts-rs로 내보낸다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};

/// 게시글 목록 조회 오류 타입 — 공통 오류 봉투 재사용.
pub type ArticleListError = ErrorEnvelope<NaverCafeCommonErrorData>;

/// 게시글 목록 정렬 기준 — 최신글 / 인기글.
///
/// API 쿼리 파라미터 `sortBy`로 매핑된다(최신글=`TIME`, 인기글=`LIKE`).
// 주의: 이 파일은 다른 바인딩 소스(src/ipc)보다 한 단계 깊어(article_list/)
// export_to 경로의 `../`가 하나 적다. [[project_ts_rs_export_path]]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    /// 최신글 — 작성 시각 내림차순.
    Latest,
    /// 인기글 — 좋아요/조회 기준.
    Popular,
}

impl SortBy {
    /// 네이버 API `sortBy` 쿼리 값으로 변환한다.
    ///
    /// ⚠️ 미확인: 실제 파라미터 값은 캡처로 검증 후 확정할 것.
    pub fn as_query_value(self) -> &'static str {
        match self {
            SortBy::Latest => "TIME",
            SortBy::Popular => "LIKE",
        }
    }
}

/// UI로 반환하는 게시글 1건 (slim).
///
/// 원본 응답의 다수 필드 중 목록 표시에 필요한 것만 추린다. 작성자 식별용
/// 민감 필드(`memberKey` 등)는 내부 구조체에 선언하지 않아 자동 배제된다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Article {
    /// 숫자 게시글 ID. (JS `number` — [`crate::ipc::cafes::Board`] 참고)
    #[ts(type = "number")]
    pub article_id: u64,
    /// 게시글 제목.
    pub subject: String,
    /// 작성자 닉네임.
    pub writer_nickname: String,
    /// 글이 속한 게시판(메뉴) ID.
    #[ts(type = "number")]
    pub menu_id: u64,
    /// 게시판 이름 (예: "자유게시판").
    pub menu_name: String,
    /// 댓글 수.
    #[ts(type = "number")]
    pub comment_count: u64,
    /// 조회 수.
    #[ts(type = "number")]
    pub read_count: u64,
    /// 좋아요 수.
    #[ts(type = "number")]
    pub like_count: u64,
    /// 작성 시각(epoch millis).
    #[ts(type = "number")]
    pub write_date_timestamp: u64,
}

/// 게시글 목록 조회 결과 — UI 반환용.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ArticleListResponse {
    /// 조회된 게시글 목록.
    pub articles: Vec<Article>,
    /// 마지막 페이지 여부.
    pub last_page: bool,
}

// ---------------------------------------------------------------------------
// 내부 응답 봉투 (deserialize 전용) — message.result.articleList[]
// ---------------------------------------------------------------------------

/// 최상위 응답: `{ "message": { "result": { ... } } }`.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ArticleListEnvelope {
    pub message: ArticleListMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ArticleListMessage {
    pub result: ArticleListResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArticleListResult {
    #[serde(default)]
    pub article_list: Vec<ArticleItem>,
    pub page_info: PageInfo,
}

/// 페이지 정보.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    pub last_page: bool,
}

/// 응답의 게시글 항목 — 필요한 키만 선언(민감 필드는 무시됨).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ArticleItem {
    pub article_id: u64,
    pub subject: String,
    pub writer_nickname: String,
    pub menu_id: u64,
    pub menu_name: String,
    #[serde(default)]
    pub comment_count: u64,
    #[serde(default)]
    pub read_count: u64,
    #[serde(default, rename = "likeItCount")]
    pub like_count: u64,
    #[serde(default)]
    pub write_date_timestamp: u64,
}

impl From<ArticleItem> for Article {
    fn from(it: ArticleItem) -> Self {
        Article {
            article_id: it.article_id,
            subject: it.subject,
            writer_nickname: it.writer_nickname,
            menu_id: it.menu_id,
            menu_name: it.menu_name,
            comment_count: it.comment_count,
            read_count: it.read_count,
            like_count: it.like_count,
            write_date_timestamp: it.write_date_timestamp,
        }
    }
}

impl ArticleListEnvelope {
    /// 응답을 UI 반환용 [`ArticleListResponse`]로 변환한다.
    pub(crate) fn into_response(self) -> ArticleListResponse {
        let result = self.message.result;
        let last_page = result.page_info.last_page;
        ArticleListResponse {
            articles: result.article_list.into_iter().map(Article::from).collect(),
            last_page,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSUMED_FIXTURE: &str = include_str!("fixtures/article_list_success.assumed.json");

    #[test]
    fn sort_by_maps_to_query_value() {
        assert_eq!(SortBy::Latest.as_query_value(), "TIME");
        assert_eq!(SortBy::Popular.as_query_value(), "LIKE");
    }

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
        assert!(
            json.get("writerNickname").is_some(),
            "writerNickname 키가 없음"
        );
        assert!(json.get("commentCount").is_some(), "commentCount 키가 없음");
        assert!(
            json.get("article_id").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
    }

    #[test]
    fn article_round_trips() {
        let original = Article {
            article_id: 1023,
            subject: "글".to_string(),
            writer_nickname: "회원A".to_string(),
            menu_id: 1,
            menu_name: "자유게시판".to_string(),
            comment_count: 3,
            read_count: 88,
            like_count: 1,
            write_date_timestamp: 1_717_390_000_000,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: Article = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    #[test]
    fn parses_assumed_fixture_into_response() {
        let envelope: ArticleListEnvelope =
            serde_json::from_str(ASSUMED_FIXTURE).expect("추정 fixture 역직렬화 실패");
        let response = envelope.into_response();

        assert!(response.last_page, "fixture는 lastPage=true여야 함");
        assert_eq!(response.articles.len(), 2, "게시글 2건이어야 함");

        let first = &response.articles[0];
        assert_eq!(first.article_id, 1024);
        assert_eq!(first.subject, "오늘의 공지사항입니다");
        assert_eq!(first.writer_nickname, "관리자");
        assert_eq!(first.menu_id, 1);
        assert_eq!(first.comment_count, 12);
        assert_eq!(first.read_count, 345);
        assert_eq!(first.like_count, 7);
        assert_eq!(first.write_date_timestamp, 1_717_400_000_000);
    }

    #[test]
    fn article_item_ignores_unknown_fields() {
        // 알 수 없는 필드(memberKey 등)가 있어도 파싱에 실패하지 않고 모델에 들어오지 않는다.
        let raw = r#"{
            "articleId": 5,
            "subject": "x",
            "writerNickname": "n",
            "menuId": 1,
            "menuName": "자유게시판",
            "memberKey": "SENSITIVE_SHOULD_BE_DROPPED",
            "unknownXyz": 123
        }"#;
        let item: ArticleItem =
            serde_json::from_str(raw).expect("알 수 없는 필드가 있어도 파싱 성공해야 함");
        let article = Article::from(item);
        let serialized = serde_json::to_string(&article).expect("직렬화 실패");
        assert!(
            !serialized.contains("SENSITIVE_SHOULD_BE_DROPPED"),
            "민감 필드가 노출됨"
        );
        // 누락 수치 필드는 기본값 0.
        assert_eq!(article.comment_count, 0);
        assert_eq!(article.read_count, 0);
        assert_eq!(article.like_count, 0);
    }
}
