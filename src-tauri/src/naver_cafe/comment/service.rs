//! 네이버 카페 댓글/대댓글 작성 서비스 — dry-run 미리보기 진입점.
//!
//! 기본 실행 모드는 [`CommentExecutionMode::DryRun`]이며, 어떤 네트워크 요청도
//! 발생하지 않는다. 동기 `execute_*`에 `Live`를 전달하면 [`CODE_USE_ASYNC_LIVE`]
//! 오류를 반환한다 — 실제 전송은 비동기
//! [`CafeCommentClient`](super::client::CafeCommentClient)를 사용한다.

use serde::{Deserialize, Serialize};

use super::{
    error::{CommentError, CommentErrorData},
    models::{CommentRequest, ReplyRequest},
    request_builder::{
        build_comment_form, build_reply_form, comment_headers, comment_post_path,
        comment_reply_path, encode_form, API_HOST,
    },
};
use crate::naver_cafe::{
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    models::CafeTarget,
};

// ---------------------------------------------------------------------------
// 실행 모드
// ---------------------------------------------------------------------------

/// 댓글/대댓글 작성 실행 모드.
///
/// 기본값은 [`DryRun`](CommentExecutionMode::DryRun) — 어떤 네트워크 요청도 발생하지 않는다.
/// [`Live`](CommentExecutionMode::Live)는 아직 미구현이다.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum CommentExecutionMode {
    /// 미리보기만 생성하며 전송하지 않는다.
    #[default]
    DryRun,
    /// 실제 HTTP POST 요청을 전송한다. **아직 미구현.**
    Live,
}

// ---------------------------------------------------------------------------
// 미리보기 모델
// ---------------------------------------------------------------------------

/// 실제 전송 전 사람이 검토할 수 있는 미리보기 — 요청 전체를 요약한다.
///
/// 일반 댓글과 대댓글을 공통 구조로 표현한다. 대댓글이면 [`ref_comment_id`]가
/// `Some`이고 [`is_reply`]가 `true`다.
///
/// [`ref_comment_id`]: CommentPreview::ref_comment_id
/// [`is_reply`]: CommentPreview::is_reply
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentPreview {
    // ---- 대상 ----
    /// 대상 카페 ID.
    pub cafe_id: String,
    /// 대상 게시글 ID.
    pub article_id: String,

    // ---- 내용 ----
    /// 댓글 본문.
    pub content: String,
    /// 본문 글자 수(문자 단위).
    pub content_char_count: usize,
    /// 스티커 ID (선택).
    pub sticker_id: Option<String>,

    // ---- 대댓글 구분 ----
    /// 대댓글이면 부모 댓글 ID, 일반 댓글이면 `None`.
    pub ref_comment_id: Option<String>,
    /// 대댓글 여부.
    pub is_reply: bool,

    // ---- 요청 메타데이터 ----
    /// HTTP 메서드 (항상 "POST").
    pub method: String,
    /// API 호스트 (예: "apis.naver.com").
    pub host: String,
    /// 엔드포인트 경로.
    pub path: String,
    /// 요청 헤더 목록 (이름, 값) 쌍.
    pub headers: Vec<(String, String)>,

    // ---- 페이로드 크기 ----
    /// 폼 인코딩된 전체 요청 바디 문자열 길이(바이트).
    pub request_body_length: usize,
}

// ---------------------------------------------------------------------------
// 실행 결과
// ---------------------------------------------------------------------------

/// 댓글/대댓글 작성 실행 결과.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum CommentOutcome {
    /// Dry-run 미리보기 결과.
    Preview(CommentPreview),
}

// ---------------------------------------------------------------------------
// 오류 코드 상수
// ---------------------------------------------------------------------------

/// 폼 인코딩 실패 오류 코드.
pub const CODE_FORM_BUILD_FAILED: &str = "FORM_BUILD_FAILED";

/// Live 모드 호출 시 반환되는 오류 코드.
/// 실제 전송은 [`CafeCommentClient`](super::client::CafeCommentClient)를 사용해야 한다.
pub const CODE_USE_ASYNC_LIVE: &str = "USE_ASYNC_LIVE";

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// 오류용 [`CafeTarget`]을 만든다.
fn target(cafe_id: &str, article_id: &str, ref_comment_id: Option<&str>) -> CafeTarget {
    CafeTarget {
        cafe_id: cafe_id.to_string(),
        cafe_name: None,
        menu_id: None,
        article_id: Some(article_id.to_string()),
        ref_comment_id: ref_comment_id.map(|s| s.to_string()),
    }
}

/// 폼 인코딩 오류를 [`CommentError`]로 변환한다.
fn form_err_to_comment_error(
    cafe_id: &str,
    article_id: &str,
    ref_comment_id: Option<&str>,
    err: serde_urlencoded::ser::Error,
) -> CommentError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_FORM_BUILD_FAILED.to_string(),
        message: format!("댓글 폼 인코딩에 실패했습니다: {err}"),
        error_data: Some(CommentErrorData {
            cafe: NaverCafeCommonErrorData {
                target: Some(target(cafe_id, article_id, ref_comment_id)),
                http_status: None,
                api_error_code: None,
                api_error_message: None,
                retryable: false,
            },
            article_id: Some(article_id.to_string()),
            ref_comment_id: ref_comment_id.map(|s| s.to_string()),
            validation_errors: vec![],
        }),
    }
}

/// Live 모드 미구현 안내 오류를 만든다.
fn use_async_live_error(
    cafe_id: &str,
    article_id: &str,
    ref_comment_id: Option<&str>,
) -> CommentError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_USE_ASYNC_LIVE.to_string(),
        message: "동기 execute_*는 dry-run 전용입니다. 실제 전송은 CafeCommentClient::post_comment/post_reply(비동기)를 사용하세요."
            .to_string(),
        error_data: Some(CommentErrorData {
            cafe: NaverCafeCommonErrorData {
                target: Some(target(cafe_id, article_id, ref_comment_id)),
                http_status: None,
                api_error_code: None,
                api_error_message: None,
                retryable: false,
            },
            article_id: Some(article_id.to_string()),
            ref_comment_id: ref_comment_id.map(|s| s.to_string()),
            validation_errors: vec![],
        }),
    }
}

// ---------------------------------------------------------------------------
// 공개 API
// ---------------------------------------------------------------------------

/// 일반 댓글 dry-run 미리보기를 생성한다. 어떤 네트워크 요청도 보내지 않는다.
///
/// # Errors
/// 폼 인코딩에 실패하면 [`CommentError`]를 반환한다.
pub fn build_comment_preview(request: &CommentRequest) -> Result<CommentPreview, CommentError> {
    let form = build_comment_form(request);
    let body = encode_form(&form)
        .map_err(|e| form_err_to_comment_error(&request.cafe_id, &request.article_id, None, e))?;

    Ok(CommentPreview {
        cafe_id: request.cafe_id.clone(),
        article_id: request.article_id.clone(),
        content: request.content.clone(),
        content_char_count: request.content.chars().count(),
        sticker_id: request.sticker_id.clone(),
        ref_comment_id: None,
        is_reply: false,
        method: "POST".to_string(),
        host: API_HOST.to_string(),
        path: comment_post_path().to_string(),
        headers: comment_headers(&request.cafe_id, &request.article_id),
        request_body_length: body.len(),
    })
}

/// 대댓글 dry-run 미리보기를 생성한다. 어떤 네트워크 요청도 보내지 않는다.
///
/// # Errors
/// 폼 인코딩에 실패하면 [`CommentError`]를 반환한다.
pub fn build_reply_preview(request: &ReplyRequest) -> Result<CommentPreview, CommentError> {
    let form = build_reply_form(request);
    let body = encode_form(&form).map_err(|e| {
        form_err_to_comment_error(
            &request.cafe_id,
            &request.article_id,
            Some(&request.ref_comment_id),
            e,
        )
    })?;

    Ok(CommentPreview {
        cafe_id: request.cafe_id.clone(),
        article_id: request.article_id.clone(),
        content: request.content.clone(),
        content_char_count: request.content.chars().count(),
        sticker_id: request.sticker_id.clone(),
        ref_comment_id: Some(request.ref_comment_id.clone()),
        is_reply: true,
        method: "POST".to_string(),
        host: API_HOST.to_string(),
        path: comment_reply_path().to_string(),
        headers: comment_headers(&request.cafe_id, &request.article_id),
        request_body_length: body.len(),
    })
}

/// 일반 댓글 동기 실행 진입점. 모드 기본값은 [`CommentExecutionMode::DryRun`].
///
/// - `DryRun`: 미리보기만 생성 (전송 없음).
/// - `Live`: `USE_ASYNC_LIVE` 오류를 반환한다(미구현).
///
/// # Errors
/// - `DryRun`에서 폼 인코딩 실패 시 [`CommentError`].
/// - `Live`는 항상 `Err(CommentError { code: "USE_ASYNC_LIVE" })`.
pub fn execute_comment(
    request: &CommentRequest,
    mode: CommentExecutionMode,
) -> Result<CommentOutcome, CommentError> {
    match mode {
        CommentExecutionMode::DryRun => {
            Ok(CommentOutcome::Preview(build_comment_preview(request)?))
        }
        CommentExecutionMode::Live => Err(use_async_live_error(
            &request.cafe_id,
            &request.article_id,
            None,
        )),
    }
}

/// 대댓글 동기 실행 진입점. 모드 기본값은 [`CommentExecutionMode::DryRun`].
///
/// # Errors
/// - `DryRun`에서 폼 인코딩 실패 시 [`CommentError`].
/// - `Live`는 항상 `Err(CommentError { code: "USE_ASYNC_LIVE" })`.
pub fn execute_reply(
    request: &ReplyRequest,
    mode: CommentExecutionMode,
) -> Result<CommentOutcome, CommentError> {
    match mode {
        CommentExecutionMode::DryRun => Ok(CommentOutcome::Preview(build_reply_preview(request)?)),
        CommentExecutionMode::Live => Err(use_async_live_error(
            &request.cafe_id,
            &request.article_id,
            Some(&request.ref_comment_id),
        )),
    }
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn base_comment() -> CommentRequest {
        CommentRequest {
            cafe_id: "31732304".to_string(),
            article_id: "2".to_string(),
            content: "안녕하세요".to_string(),
            sticker_id: None,
        }
    }

    fn base_reply() -> ReplyRequest {
        ReplyRequest {
            cafe_id: "31732304".to_string(),
            article_id: "4".to_string(),
            content: "답글입니다".to_string(),
            sticker_id: None,
            ref_comment_id: "62598693".to_string(),
        }
    }

    // ------------------------------------------------------------------
    // build_comment_preview
    // ------------------------------------------------------------------

    #[test]
    fn comment_preview_basic_fields() {
        let preview = build_comment_preview(&base_comment()).expect("미리보기 생성 실패");
        assert_eq!(preview.cafe_id, "31732304");
        assert_eq!(preview.article_id, "2");
        assert_eq!(preview.content, "안녕하세요");
        assert_eq!(preview.content_char_count, 5);
        assert_eq!(preview.method, "POST");
        assert_eq!(preview.host, API_HOST);
        assert_eq!(preview.path, "/cafe-web/cafe-mobile/CommentPost.json");
    }

    #[test]
    fn comment_preview_is_not_reply() {
        let preview = build_comment_preview(&base_comment()).expect("미리보기 생성 실패");
        assert!(!preview.is_reply);
        assert!(preview.ref_comment_id.is_none());
    }

    #[test]
    fn comment_preview_body_length_is_nonzero() {
        let preview = build_comment_preview(&base_comment()).expect("미리보기 생성 실패");
        assert!(preview.request_body_length > 0);
    }

    // ------------------------------------------------------------------
    // build_reply_preview
    // ------------------------------------------------------------------

    #[test]
    fn reply_preview_is_reply_with_ref_id() {
        let preview = build_reply_preview(&base_reply()).expect("미리보기 생성 실패");
        assert!(preview.is_reply);
        assert_eq!(preview.ref_comment_id.as_deref(), Some("62598693"));
        assert_eq!(preview.path, "/cafe-web/cafe-mobile/CommentReply.json");
    }

    // ------------------------------------------------------------------
    // execute_comment / execute_reply — 모드 분기
    // ------------------------------------------------------------------

    #[test]
    fn default_mode_is_dry_run() {
        assert_eq!(
            CommentExecutionMode::default(),
            CommentExecutionMode::DryRun
        );
    }

    #[test]
    fn execute_comment_dry_run_returns_preview() {
        let result = execute_comment(&base_comment(), CommentExecutionMode::default());
        assert!(result.is_ok(), "DryRun은 Ok여야 함");
        match result.unwrap() {
            CommentOutcome::Preview(p) => assert!(!p.is_reply),
        }
    }

    #[test]
    fn execute_reply_dry_run_returns_reply_preview() {
        let result = execute_reply(&base_reply(), CommentExecutionMode::DryRun);
        match result.expect("DryRun은 Ok여야 함") {
            CommentOutcome::Preview(p) => assert!(p.is_reply),
        }
    }

    #[test]
    fn execute_comment_live_returns_use_async_live_error() {
        let err = execute_comment(&base_comment(), CommentExecutionMode::Live)
            .expect_err("Live는 Err여야 함");
        assert_eq!(err.code, CODE_USE_ASYNC_LIVE);
    }

    #[test]
    fn execute_reply_live_returns_use_async_live_error() {
        let err = execute_reply(&base_reply(), CommentExecutionMode::Live)
            .expect_err("Live는 Err여야 함");
        assert_eq!(err.code, CODE_USE_ASYNC_LIVE);
        // 대댓글 오류에는 부모 댓글 ID가 담긴다.
        let data = err.error_data.expect("errorData가 없음");
        assert_eq!(data.ref_comment_id.as_deref(), Some("62598693"));
    }

    // ------------------------------------------------------------------
    // 직렬화 round-trip
    // ------------------------------------------------------------------

    #[test]
    fn comment_preview_round_trips() {
        let preview = build_comment_preview(&base_comment()).expect("미리보기 생성 실패");
        let json = serde_json::to_string(&preview).expect("직렬화 실패");
        let restored: CommentPreview = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(preview, restored);
    }
}
