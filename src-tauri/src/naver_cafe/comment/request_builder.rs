//! 네이버 카페 댓글/대댓글 작성 요청 빌더.
//!
//! [`CommentRequest`]/[`ReplyRequest`]를 받아 실제 HTTP POST 요청의
//! `application/x-www-form-urlencoded` 바디, 엔드포인트 경로, 필수 헤더를 조립한다.
//!
//! 글 작성(`post`)과의 차이:
//! - 호스트가 `apis.naver.com` (글 작성은 `apis.cafe.naver.com`).
//! - 본문이 JSON이 아니라 폼 인코딩(`application/x-www-form-urlencoded`).
//! - 스마트에디터 `contentJson`이 필요 없고 평문 `content`를 그대로 보낸다.
//!
//! 실제 HTTP 전송은 이 모듈의 범위 밖이다. 세션 레이어가 쿠키/인증 헤더를
//! 주입하며, 이 모듈은 요청 구조만 생성한다.

use serde::{Deserialize, Serialize};

use super::models::{CommentRequest, ReplyRequest};

// ---------------------------------------------------------------------------
// 상수
// ---------------------------------------------------------------------------

/// 댓글/대댓글 등록 API 호스트.
pub const API_HOST: &str = "apis.naver.com";

/// Origin 헤더 값 — 패킷 캡처에서 확인된 값.
const ORIGIN: &str = "https://cafe.naver.com";

/// `x-cafe-product` 헤더 값. 대댓글 캡처에서 확인됨.
pub const CAFE_PRODUCT_PC: &str = "pc";

/// `requestFrom` 폼 필드 고정값 — 패킷 캡처에서 확인된 값(`A`).
pub const REQUEST_FROM: &str = "A";

// ---------------------------------------------------------------------------
// 폼 바디 모델
// ---------------------------------------------------------------------------

/// 일반 댓글 폼 바디 — 필드 순서는 패킷 캡처 본문 순서를 그대로 따른다.
///
/// 캡처 본문: `content=...&stickerId=&cafeId=...&articleId=...&requestFrom=A`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentForm {
    /// 댓글 본문.
    pub content: String,
    /// 스티커 ID — 미사용 시 빈 문자열.
    pub sticker_id: String,
    /// 카페 ID.
    pub cafe_id: String,
    /// 게시글 ID.
    pub article_id: String,
    /// 요청 출처 고정값(`A`).
    pub request_from: String,
}

/// 대댓글 폼 바디 — `refCommentId`가 `cafeId` 앞에 위치한다(캡처 순서).
///
/// 캡처 본문: `content=...&stickerId=&refCommentId=...&cafeId=...&articleId=...&requestFrom=A`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplyForm {
    /// 대댓글 본문.
    pub content: String,
    /// 스티커 ID — 미사용 시 빈 문자열.
    pub sticker_id: String,
    /// 부모 댓글 ID.
    pub ref_comment_id: String,
    /// 카페 ID.
    pub cafe_id: String,
    /// 게시글 ID.
    pub article_id: String,
    /// 요청 출처 고정값(`A`).
    pub request_from: String,
}

// ---------------------------------------------------------------------------
// 빌더 함수
// ---------------------------------------------------------------------------

/// [`CommentRequest`]로 [`CommentForm`]을 조립한다.
///
/// `sticker_id`가 `None`이면 빈 문자열로, `request_from`은 [`REQUEST_FROM`]으로 채운다.
pub fn build_comment_form(request: &CommentRequest) -> CommentForm {
    CommentForm {
        content: request.content.clone(),
        sticker_id: request.sticker_id.clone().unwrap_or_default(),
        cafe_id: request.cafe_id.clone(),
        article_id: request.article_id.clone(),
        request_from: REQUEST_FROM.to_string(),
    }
}

/// [`ReplyRequest`]로 [`ReplyForm`]을 조립한다.
pub fn build_reply_form(request: &ReplyRequest) -> ReplyForm {
    ReplyForm {
        content: request.content.clone(),
        sticker_id: request.sticker_id.clone().unwrap_or_default(),
        ref_comment_id: request.ref_comment_id.clone(),
        cafe_id: request.cafe_id.clone(),
        article_id: request.article_id.clone(),
        request_from: REQUEST_FROM.to_string(),
    }
}

/// 폼 구조체를 `application/x-www-form-urlencoded` 문자열로 인코딩한다.
///
/// 한글 등 비ASCII 문자는 UTF-8 percent-encoding, 공백은 `+`로 인코딩된다.
///
/// # Errors
/// 인코딩에 실패하면 `serde_urlencoded::ser::Error`를 반환한다.
/// (모든 필드가 문자열인 평탄한 구조라 실제로는 실패하지 않는다.)
pub fn encode_form<T: Serialize>(form: &T) -> Result<String, serde_urlencoded::ser::Error> {
    serde_urlencoded::to_string(form)
}

// ---------------------------------------------------------------------------
// 엔드포인트 / 헤더 헬퍼
// ---------------------------------------------------------------------------

/// 일반 댓글 등록 엔드포인트 경로.
pub fn comment_post_path() -> &'static str {
    "/cafe-web/cafe-mobile/CommentPost.json"
}

/// 대댓글 등록 엔드포인트 경로.
pub fn comment_reply_path() -> &'static str {
    "/cafe-web/cafe-mobile/CommentReply.json"
}

/// 댓글/대댓글 등록 요청에 필요한 HTTP 헤더 목록을 반환한다.
///
/// 일반 댓글과 대댓글이 동일한 헤더 집합을 사용한다. Referer는 글쓰기 화면이
/// 아니라 **게시글 읽기 페이지**(`.../cafes/{cafeId}/articles/{articleId}`)를 가리킨다.
///
/// 포함 헤더:
/// - `Content-Type: application/x-www-form-urlencoded`
/// - `Accept: application/json, text/plain, */*`
/// - `Origin: https://cafe.naver.com`
/// - `Referer: https://cafe.naver.com/ca-fe/cafes/{cafeId}/articles/{articleId}`
/// - `x-cafe-product: pc`
/// - `sec-fetch-site: same-site` / `sec-fetch-mode: cors` / `sec-fetch-dest: empty`
/// - `accept-language: ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7`
///
/// ℹ️ `content-length`, `User-Agent`, 가상 헤더(`:method` 등)는 포함하지 않는다 —
/// reqwest 또는 상위 레이어가 처리한다.
pub fn comment_headers(cafe_id: &str, article_id: &str) -> Vec<(String, String)> {
    vec![
        (
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        (
            "Accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        ("Origin".to_string(), ORIGIN.to_string()),
        (
            "Referer".to_string(),
            format!("https://cafe.naver.com/ca-fe/cafes/{cafe_id}/articles/{article_id}"),
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

    fn capture_comment() -> CommentRequest {
        // cafe-comment-packet-analysis.md 의 캡처 값.
        CommentRequest {
            cafe_id: "31732304".to_string(),
            article_id: "2".to_string(),
            content: "23589182".to_string(),
            sticker_id: None,
        }
    }

    fn capture_reply() -> ReplyRequest {
        // naver-cafe-comment-reply-api.md 의 캡처 값.
        ReplyRequest {
            cafe_id: "31732304".to_string(),
            article_id: "4".to_string(),
            content: "안녕하세요".to_string(),
            sticker_id: None,
            ref_comment_id: "62598693".to_string(),
        }
    }

    // ------------------------------------------------------------------
    // 폼 인코딩 — 캡처 본문과 정확히 일치
    // ------------------------------------------------------------------

    #[test]
    fn comment_body_matches_capture() {
        let body = encode_form(&build_comment_form(&capture_comment())).expect("인코딩 실패");
        assert_eq!(
            body,
            "content=23589182&stickerId=&cafeId=31732304&articleId=2&requestFrom=A"
        );
    }

    #[test]
    fn reply_body_matches_capture() {
        let body = encode_form(&build_reply_form(&capture_reply())).expect("인코딩 실패");
        assert_eq!(
            body,
            "content=%EC%95%88%EB%85%95%ED%95%98%EC%84%B8%EC%9A%94&stickerId=&refCommentId=62598693&cafeId=31732304&articleId=4&requestFrom=A"
        );
    }

    // ------------------------------------------------------------------
    // 폼 필드 구성 검증
    // ------------------------------------------------------------------

    #[test]
    fn comment_form_has_no_ref_comment_id_key() {
        let body = encode_form(&build_comment_form(&capture_comment())).expect("인코딩 실패");
        assert!(
            !body.contains("refCommentId"),
            "일반 댓글에는 refCommentId가 없어야 함: {body}"
        );
    }

    #[test]
    fn reply_form_contains_ref_comment_id() {
        let form = build_reply_form(&capture_reply());
        assert_eq!(form.ref_comment_id, "62598693");
    }

    #[test]
    fn request_from_is_a() {
        assert_eq!(build_comment_form(&capture_comment()).request_from, "A");
        assert_eq!(build_reply_form(&capture_reply()).request_from, "A");
    }

    #[test]
    fn none_sticker_id_becomes_empty_string() {
        let form = build_comment_form(&capture_comment());
        assert_eq!(form.sticker_id, "");
    }

    #[test]
    fn some_sticker_id_is_preserved() {
        let mut req = capture_comment();
        req.sticker_id = Some("st-99".to_string());
        let body = encode_form(&build_comment_form(&req)).expect("인코딩 실패");
        assert!(body.contains("stickerId=st-99"), "stickerId 값이 반영되어야 함: {body}");
    }

    #[test]
    fn korean_content_is_percent_encoded() {
        let mut req = capture_comment();
        req.content = "테스트".to_string();
        let body = encode_form(&build_comment_form(&req)).expect("인코딩 실패");
        assert!(
            body.starts_with("content=%"),
            "한글 본문은 percent-encoding되어야 함: {body}"
        );
        assert!(!body.contains("테스트"), "원문 한글이 그대로 남으면 안 됨: {body}");
    }

    // ------------------------------------------------------------------
    // 엔드포인트 경로 검증
    // ------------------------------------------------------------------

    #[test]
    fn comment_post_path_is_correct() {
        assert_eq!(comment_post_path(), "/cafe-web/cafe-mobile/CommentPost.json");
    }

    #[test]
    fn comment_reply_path_is_correct() {
        assert_eq!(
            comment_reply_path(),
            "/cafe-web/cafe-mobile/CommentReply.json"
        );
    }

    // ------------------------------------------------------------------
    // 헤더 검증
    // ------------------------------------------------------------------

    fn header_value<'a>(headers: &'a [(String, String)], key: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, val)| val.as_str())
    }

    #[test]
    fn headers_content_type_is_form_urlencoded() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(
            header_value(&headers, "Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn headers_referer_points_to_article_page() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(
            header_value(&headers, "Referer"),
            Some("https://cafe.naver.com/ca-fe/cafes/31732304/articles/2")
        );
    }

    #[test]
    fn headers_origin_is_correct() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(header_value(&headers, "Origin"), Some("https://cafe.naver.com"));
    }

    #[test]
    fn headers_x_cafe_product_is_pc() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(header_value(&headers, "x-cafe-product"), Some("pc"));
    }

    #[test]
    fn headers_sec_fetch_present() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(header_value(&headers, "sec-fetch-site"), Some("same-site"));
        assert_eq!(header_value(&headers, "sec-fetch-mode"), Some("cors"));
        assert_eq!(header_value(&headers, "sec-fetch-dest"), Some("empty"));
    }

    #[test]
    fn headers_accept_language_is_set() {
        let headers = comment_headers("31732304", "2");
        assert_eq!(
            header_value(&headers, "accept-language"),
            Some("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7")
        );
    }
}
