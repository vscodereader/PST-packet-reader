//! 네이버 카페 글 작성 요청 빌더.
//!
//! [`PostRequest`]와 `contentJson` 문자열을 받아 실제 HTTP POST 요청의
//! JSON 바디, 엔드포인트 경로, 필수 헤더를 조립한다.
//!
//! 실제 HTTP 전송은 이 모듈의 범위 밖이다. 세션 레이어가 쿠키/인증 헤더를
//! 주입하며, 이 모듈은 요청 구조만 생성한다.

use serde::{Deserialize, Serialize};

use super::{
    models::PostRequest,
    smart_editor::{build_content_json_string, IdProvider},
};

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 게시글 등록 API 호스트.
pub const API_HOST: &str = "apis.cafe.naver.com";

/// Origin 헤더 값 — 패킷 캡처에서 확인된 값.
const ORIGIN: &str = "https://cafe.naver.com";

/// `x-cafe-product` 헤더 값.
///
/// `apis.cafe.naver.com/editor/*` 에디터 서비스가 이 헤더를 요구하며,
/// 없으면 errorCode 10404(Page Not Found) 또는 11001을 반환한다.
pub const CAFE_PRODUCT_PC: &str = "pc";

// ---------------------------------------------------------------------------
// 요청 바디 모델
// ---------------------------------------------------------------------------

/// 게시글 등록 요청 최상위 바디 — `article` 봉투.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArticleWriteBody {
    /// 실제 게시글 데이터.
    pub article: ArticleWrite,
}

/// 게시글 등록 요청 내부 데이터.
///
/// 패킷 캡처로 확인된 실제 요청 바디 형태를 그대로 반영한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArticleWrite {
    /// 카페 ID — 캡처에서 확인: **문자열** 타입 (예: `"31732304"`).
    pub cafe_id: String,
    /// 스마트에디터 문서 JSON 문자열 (Step 3에서 생성된 직렬화 결과).
    pub content_json: String,
    /// 플랫폼 고정값. 항상 `"pc"`.
    pub from: String,
    /// 게시판(메뉴) ID — 캡처에서 확인: **숫자** 타입.
    pub menu_id: u64,
    /// 게시글 제목.
    pub subject: String,
    /// 태그 목록.
    pub tag_list: Vec<String>,
    /// 에디터 버전 고정값. 항상 `4`.
    pub editor_version: u32,
    /// 부모 게시글 ID 고정값. 항상 `0` (일반 글쓰기).
    pub parent_id: u32,
    /// 공개 여부. 캡처 기본값: `false`.
    pub open: bool,
    /// 네이버 공개 여부. 캡처 기본값: `true`.
    pub naver_open: bool,
    /// 외부 공개 여부. 캡처 기본값: `true`.
    pub external_open: bool,
    /// 댓글 허용 여부. 캡처 기본값: `true`.
    pub enable_comment: bool,
    /// 스크랩 허용 여부. 캡처 기본값: `false`.
    pub enable_scrap: bool,
    /// 복사 허용 여부. 캡처 기본값: `false`.
    pub enable_copy: bool,
    /// 자동 출처 사용 여부 고정값. 항상 `false`.
    pub use_auto_source: bool,
    /// CCL 유형 목록 고정값. 항상 빈 배열.
    pub ccl_types: Vec<String>,
    /// CCL 사용 여부 고정값. 항상 `false`.
    pub use_ccl: bool,
}

// ---------------------------------------------------------------------------
// 빌더 함수
// ---------------------------------------------------------------------------

/// [`PostRequest`]와 `content_json` 문자열로 [`ArticleWriteBody`]를 조립한다.
///
/// `content_json`은 외부에서 주입받아 테스트 시 고정 값을 사용할 수 있다.
/// `Option<bool>` 필드가 `None`이면 패킷 캡처에서 확인된 기본값을 적용한다:
/// - `open` → `false`
/// - `naver_open` → `true`
/// - `external_open` → `true`
/// - `enable_comment` → `true`
/// - `enable_scrap` → `false`
/// - `enable_copy` → `false`
pub fn build_article_write_body(request: &PostRequest, content_json: String) -> ArticleWriteBody {
    ArticleWriteBody {
        article: ArticleWrite {
            cafe_id: request.cafe_id.clone(),
            content_json,
            from: "pc".to_string(),
            menu_id: request.menu_id,
            subject: request.subject.clone(),
            tag_list: request.tag_list.clone(),
            editor_version: 4,
            parent_id: 0,
            open: request.open.unwrap_or(false),
            naver_open: request.naver_open.unwrap_or(true),
            external_open: request.external_open.unwrap_or(true),
            enable_comment: request.enable_comment.unwrap_or(true),
            enable_scrap: request.enable_scrap.unwrap_or(false),
            enable_copy: request.enable_copy.unwrap_or(false),
            use_auto_source: false,
            ccl_types: vec![],
            use_ccl: false,
        },
    }
}

/// [`PostRequest`]와 ID 공급자를 받아 Step 3(스마트에디터)와 Step 4(요청 빌더)를
/// 연결하는 편의 함수.
///
/// 내부에서 [`build_content_json_string`]을 호출해 `contentJson`을 생성하고,
/// [`build_article_write_body`]로 최종 [`ArticleWriteBody`]를 반환한다.
///
/// # Errors
/// `serde_json` 직렬화 실패 시 `serde_json::Error`를 반환한다.
pub fn build_article_write_body_with_content(
    request: &PostRequest,
    ids: &mut impl IdProvider,
) -> Result<ArticleWriteBody, serde_json::Error> {
    let content_json = build_content_json_string(&request.body_text, ids)?;
    Ok(build_article_write_body(request, content_json))
}

// ---------------------------------------------------------------------------
// 엔드포인트 / 헤더 헬퍼
// ---------------------------------------------------------------------------

/// 게시글 등록 엔드포인트 경로를 반환한다.
///
/// 형식: `/editor/v2.0/cafes/{cafeId}/menus/{menuId}/articles`
pub fn article_post_path(cafe_id: &str, menu_id: u64) -> String {
    format!("/editor/v2.0/cafes/{cafe_id}/menus/{menu_id}/articles")
}

/// 게시글 등록 요청에 필요한 HTTP 헤더 목록을 반환한다.
///
/// 반환값: `(헤더명, 헤더값)` 쌍의 벡터.
///
/// 포함 헤더:
/// - `Content-Type: application/json`
/// - `Accept: application/json, text/plain, */*`
/// - `Origin: https://cafe.naver.com`
/// - `Referer: https://cafe.naver.com/ca-fe/cafes/{cafeId}/articles/write?boardType={boardType}`
///   - `board_type`은 선택된 게시판의 레이아웃 유형([`Menu::board_type`](crate::naver_cafe::menu::Menu))이다.
///     일반 게시판은 보통 `"L"`(리스트형)이지만 게시판마다 다를 수 있다.
/// - `x-cafe-product: pc`  ← 에디터 서비스 필수 헤더 ([`CAFE_PRODUCT_PC`] 참조)
/// - `sec-fetch-site: same-site`
/// - `sec-fetch-mode: cors`
/// - `sec-fetch-dest: empty`
/// - `accept-language: ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7`
///
/// ℹ️ `accept-encoding`, `content-length`, 가상 헤더(`:method` 등), `User-Agent`는
/// 포함하지 않는다 — reqwest 또는 상위 레이어가 처리한다.
pub fn article_post_headers(cafe_id: &str, board_type: &str) -> Vec<(String, String)> {
    vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        ("Origin".to_string(), ORIGIN.to_string()),
        (
            "Referer".to_string(),
            format!(
                "https://cafe.naver.com/ca-fe/cafes/{cafe_id}/articles/write?boardType={board_type}"
            ),
        ),
        ("x-cafe-product".to_string(), CAFE_PRODUCT_PC.to_string()),
        ("sec-fetch-site".to_string(), "same-site".to_string()),
        ("sec-fetch-mode".to_string(), "cors".to_string()),
        ("sec-fetch-dest".to_string(), "empty".to_string()),
        (
            "accept-language".to_string(),
            "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7".to_string(),
        ),
    ]
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::post::smart_editor::SequentialIdProvider;

    /// 테스트용 픽스처 `contentJson` 플레이스홀더.
    /// `article_register_request.json` 픽스처와 동일한 값을 사용한다.
    const FIXTURE_CONTENT_JSON: &str = "<contentJson>";

    /// 패킷 캡처로 확인된 요청 바디 픽스처.
    const ARTICLE_REGISTER_REQUEST: &str = include_str!("fixtures/article_register_request.json");

    fn capture_request() -> PostRequest {
        PostRequest {
            cafe_id: "31732304".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "rust".to_string(),
            body_text: "rust 본문".to_string(),
            tag_list: vec![
                "RUST".to_string(),
                "C".to_string(),
                "JavaScript".to_string(),
            ],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        }
    }

    // ------------------------------------------------------------------
    // 픽스처 동등성 테스트 (serde_json::Value 비교로 키 순서 무관)
    // ------------------------------------------------------------------

    #[test]
    fn body_equals_fixture_as_json_value() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        let actual: serde_json::Value = serde_json::to_value(&body).expect("직렬화 실패");
        let expected: serde_json::Value =
            serde_json::from_str(ARTICLE_REGISTER_REQUEST).expect("픽스처 파싱 실패");
        assert_eq!(actual, expected, "생성된 바디가 픽스처와 다름");
    }

    // ------------------------------------------------------------------
    // cafeId 문자열, menuId 숫자 타입 검증
    // ------------------------------------------------------------------

    #[test]
    fn cafe_id_serializes_as_string() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        let json = serde_json::to_value(&body).expect("직렬화 실패");
        let cafe_id = json["article"]["cafeId"].clone();
        assert!(
            cafe_id.is_string(),
            "cafeId는 JSON 문자열이어야 함, 실제: {:?}",
            cafe_id
        );
        assert_eq!(cafe_id.as_str().unwrap(), "31732304");
    }

    #[test]
    fn menu_id_serializes_as_number() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        let json = serde_json::to_value(&body).expect("직렬화 실패");
        let menu_id = json["article"]["menuId"].clone();
        assert!(
            menu_id.is_number(),
            "menuId는 JSON 숫자여야 함, 실제: {:?}",
            menu_id
        );
        assert_eq!(menu_id.as_u64().unwrap(), 1);
    }

    // ------------------------------------------------------------------
    // Option None → 캡처 기본값 적용 검증
    // ------------------------------------------------------------------

    #[test]
    fn none_options_produce_captured_defaults() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        let a = &body.article;
        assert!(!a.open, "open 기본값은 false");
        assert!(a.naver_open, "naverOpen 기본값은 true");
        assert!(a.external_open, "externalOpen 기본값은 true");
        assert!(a.enable_comment, "enableComment 기본값은 true");
        assert!(!a.enable_scrap, "enableScrap 기본값은 false");
        assert!(!a.enable_copy, "enableCopy 기본값은 false");
    }

    #[test]
    fn some_false_overrides_naver_open_default() {
        let mut req = capture_request();
        req.naver_open = Some(false);
        let body = build_article_write_body(&req, FIXTURE_CONTENT_JSON.to_string());
        assert!(
            !body.article.naver_open,
            "Some(false)는 기본값 true를 덮어써야 함"
        );
    }

    #[test]
    fn some_true_overrides_open_default() {
        let mut req = capture_request();
        req.open = Some(true);
        let body = build_article_write_body(&req, FIXTURE_CONTENT_JSON.to_string());
        assert!(body.article.open, "Some(true)는 기본값 false를 덮어써야 함");
    }

    #[test]
    fn some_true_overrides_enable_scrap_default() {
        let mut req = capture_request();
        req.enable_scrap = Some(true);
        let body = build_article_write_body(&req, FIXTURE_CONTENT_JSON.to_string());
        assert!(
            body.article.enable_scrap,
            "Some(true)는 기본값 false를 덮어써야 함"
        );
    }

    // ------------------------------------------------------------------
    // 고정 상수 필드 검증
    // ------------------------------------------------------------------

    #[test]
    fn constants_from_is_pc() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        assert_eq!(body.article.from, "pc");
    }

    #[test]
    fn constants_editor_version_is_4() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        assert_eq!(body.article.editor_version, 4);
    }

    #[test]
    fn constants_parent_id_is_0() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        assert_eq!(body.article.parent_id, 0);
    }

    #[test]
    fn constants_use_ccl_is_false() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        assert!(!body.article.use_ccl);
    }

    #[test]
    fn constants_ccl_types_is_empty() {
        let body = build_article_write_body(&capture_request(), FIXTURE_CONTENT_JSON.to_string());
        assert!(body.article.ccl_types.is_empty());
    }

    // ------------------------------------------------------------------
    // 엔드포인트 경로 검증
    // ------------------------------------------------------------------

    #[test]
    fn article_post_path_returns_correct_path() {
        let path = article_post_path("31732304", 1);
        assert_eq!(path, "/editor/v2.0/cafes/31732304/menus/1/articles");
    }

    // ------------------------------------------------------------------
    // 헤더 검증
    // ------------------------------------------------------------------

    #[test]
    fn headers_referer_uses_write_url_with_board_type_l() {
        let headers = article_post_headers("31732304", "L");
        let referer = headers
            .iter()
            .find(|(name, _)| name == "Referer")
            .map(|(_, val)| val.as_str())
            .expect("Referer 헤더가 없음");
        assert_eq!(
            referer, "https://cafe.naver.com/ca-fe/cafes/31732304/articles/write?boardType=L",
            "Referer는 브라우저 캡처 값(boardType=L)을 사용해야 함"
        );
    }

    #[test]
    fn headers_referer_uses_provided_board_type() {
        // L이 아닌 게시판 유형도 Referer에 그대로 반영되어야 함
        let headers = article_post_headers("31732304", "M");
        let referer = headers
            .iter()
            .find(|(name, _)| name == "Referer")
            .map(|(_, val)| val.as_str())
            .expect("Referer 헤더가 없음");
        assert_eq!(
            referer, "https://cafe.naver.com/ca-fe/cafes/31732304/articles/write?boardType=M",
            "Referer는 전달된 board_type을 사용해야 함"
        );
    }

    #[test]
    fn headers_origin_is_correct() {
        let headers = article_post_headers("31732304", "L");
        let origin = headers
            .iter()
            .find(|(name, _)| name == "Origin")
            .map(|(_, val)| val.as_str())
            .expect("Origin 헤더가 없음");
        assert_eq!(origin, "https://cafe.naver.com");
    }

    #[test]
    fn headers_content_type_is_json() {
        let headers = article_post_headers("31732304", "L");
        let ct = headers
            .iter()
            .find(|(name, _)| name == "Content-Type")
            .map(|(_, val)| val.as_str())
            .expect("Content-Type 헤더가 없음");
        assert_eq!(ct, "application/json");
    }

    #[test]
    fn headers_x_cafe_product_is_pc() {
        let headers = article_post_headers("31732304", "L");
        let val = headers
            .iter()
            .find(|(name, _)| name == "x-cafe-product")
            .map(|(_, val)| val.as_str())
            .expect("x-cafe-product 헤더가 없음");
        assert_eq!(val, "pc", "x-cafe-product 값은 pc여야 함");
    }

    #[test]
    fn headers_accept_is_json() {
        let headers = article_post_headers("31732304", "L");
        let val = headers
            .iter()
            .find(|(name, _)| name == "Accept")
            .map(|(_, val)| val.as_str())
            .expect("Accept 헤더가 없음");
        assert_eq!(val, "application/json, text/plain, */*");
    }

    #[test]
    fn headers_sec_fetch_headers_present() {
        let headers = article_post_headers("31732304", "L");
        let find = |key: &str| {
            headers
                .iter()
                .find(|(name, _)| name.as_str() == key)
                .map(|(_, val)| val.clone())
        };
        assert_eq!(
            find("sec-fetch-site").as_deref(),
            Some("same-site"),
            "sec-fetch-site 헤더 누락 또는 잘못된 값"
        );
        assert_eq!(
            find("sec-fetch-mode").as_deref(),
            Some("cors"),
            "sec-fetch-mode 헤더 누락 또는 잘못된 값"
        );
        assert_eq!(
            find("sec-fetch-dest").as_deref(),
            Some("empty"),
            "sec-fetch-dest 헤더 누락 또는 잘못된 값"
        );
    }

    #[test]
    fn headers_accept_language_is_set() {
        let headers = article_post_headers("31732304", "L");
        let val = headers
            .iter()
            .find(|(name, _)| name == "accept-language")
            .map(|(_, val)| val.as_str())
            .expect("accept-language 헤더가 없음");
        assert_eq!(val, "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7");
    }

    // ------------------------------------------------------------------
    // 편의 함수 (Step 3 + Step 4 연결) 검증
    // ------------------------------------------------------------------

    #[test]
    fn convenience_fn_content_json_is_non_empty() {
        let req = PostRequest {
            cafe_id: "31732304".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "rust".to_string(),
            body_text: "rust 본문".to_string(),
            tag_list: vec!["RUST".to_string()],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };
        let mut ids = SequentialIdProvider::new();
        let body = build_article_write_body_with_content(&req, &mut ids).expect("빌드 실패");
        assert!(
            !body.article.content_json.is_empty(),
            "contentJson이 빈 문자열이어서는 안 됨"
        );
    }

    #[test]
    fn convenience_fn_content_json_contains_body_text() {
        let req = PostRequest {
            cafe_id: "31732304".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "rust".to_string(),
            body_text: "rust 본문".to_string(),
            tag_list: vec!["RUST".to_string()],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };
        let mut ids = SequentialIdProvider::new();
        let body = build_article_write_body_with_content(&req, &mut ids).expect("빌드 실패");
        assert!(
            body.article.content_json.contains("rust 본문"),
            "contentJson에 본문 텍스트가 없음: {}",
            body.article.content_json
        );
    }
}
