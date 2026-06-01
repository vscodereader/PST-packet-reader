//! 네이버 카페 게시글 작성 서비스 — dry-run 미리보기 및 실행 진입점.
//!
//! 이 모듈은 어떠한 HTTP 클라이언트도 포함하지 않는다.
//! 기본 실행 모드는 [`PostExecutionMode::DryRun`]이며,
//! [`PostExecutionMode::Live`]는 아직 구현되지 않았다.

use serde::{Deserialize, Serialize};

use super::{
    error::{PostError, PostErrorData},
    models::PostRequest,
    request_builder::{
        article_post_headers, article_post_path, build_article_write_body_with_content, API_HOST,
    },
    smart_editor::IdProvider,
};
use crate::naver_cafe::{
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    models::CafeTarget,
};

// ---------------------------------------------------------------------------
// 실행 모드
// ---------------------------------------------------------------------------

/// 게시글 작성 실행 모드.
///
/// 기본값은 [`DryRun`](PostExecutionMode::DryRun) — 어떤 네트워크 요청도 발생하지 않는다.
/// [`Live`](PostExecutionMode::Live)는 세션/HTTP 계층이 구현된 이후에만 사용 가능하다.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PostExecutionMode {
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
/// `build_post_preview`로 생성되며, 어떤 네트워크 요청도 보내지 않는다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PostPreview {
    // ---- 대상 ----
    /// 대상 카페 ID.
    pub cafe_id: String,
    /// 대상 게시판(메뉴) ID.
    pub menu_id: u64,

    // ---- 게시글 내용 ----
    /// 게시글 제목.
    pub subject: String,
    /// 원본 본문 텍스트.
    pub body_text: String,
    /// 본문 줄(단락) 수 — 빈 줄 포함 줄 개수.
    pub body_line_count: usize,
    /// 태그 목록.
    pub tag_list: Vec<String>,

    // ---- 빌더 적용 후 확정된 공개 옵션 ----
    /// 공개 여부 (기본값: false).
    pub open: bool,
    /// 네이버 공개 여부 (기본값: true).
    pub naver_open: bool,
    /// 외부 공개 여부 (기본값: true).
    pub external_open: bool,
    /// 댓글 허용 여부 (기본값: true).
    pub enable_comment: bool,
    /// 스크랩 허용 여부 (기본값: false).
    pub enable_scrap: bool,
    /// 복사 허용 여부 (기본값: false).
    pub enable_copy: bool,

    // ---- 요청 메타데이터 ----
    /// HTTP 메서드 (항상 "POST").
    pub method: String,
    /// API 호스트 (예: "apis.cafe.naver.com").
    pub host: String,
    /// 엔드포인트 경로 (예: "/editor/v2.0/cafes/.../articles").
    pub path: String,
    /// 요청 헤더 목록 (이름, 값) 쌍.
    pub headers: Vec<(String, String)>,

    // ---- 페이로드 크기 정보 ----
    /// 생성된 `contentJson` 문자열의 바이트(문자) 길이.
    /// 실제 페이로드 크기를 인증·세션 정보 없이 검토할 수 있다.
    pub content_json_length: usize,
    /// 직렬화된 전체 요청 바디 문자열 길이(바이트).
    pub request_body_length: usize,
}

// ---------------------------------------------------------------------------
// 실행 결과
// ---------------------------------------------------------------------------

/// 게시글 작성 실행 결과.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PostOutcome {
    /// Dry-run 미리보기 결과.
    Preview(PostPreview),
}

// ---------------------------------------------------------------------------
// 오류 코드 상수
// ---------------------------------------------------------------------------

/// contentJson / 요청 바디 직렬화 실패 오류 코드.
pub const CODE_CONTENT_BUILD_FAILED: &str = "CONTENT_BUILD_FAILED";

/// Live 전송 미구현 오류 코드.
pub const CODE_LIVE_SEND_NOT_IMPLEMENTED: &str = "LIVE_SEND_NOT_IMPLEMENTED";

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// `serde_json::Error`를 `PostError`로 변환한다.
fn serde_err_to_post_error(request: &PostRequest, err: serde_json::Error) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_CONTENT_BUILD_FAILED.to_string(),
        message: format!(
            "contentJson 또는 요청 바디 직렬화에 실패했습니다: {}",
            err
        ),
        error_data: Some(PostErrorData {
            cafe: NaverCafeCommonErrorData {
                target: Some(CafeTarget {
                    cafe_id: request.cafe_id.clone(),
                    cafe_name: None,
                    menu_id: Some(request.menu_id),
                    article_id: None,
                    ref_comment_id: None,
                }),
                http_status: None,
                api_error_code: None,
                api_error_message: None,
                retryable: false,
            },
            menu_id: Some(request.menu_id),
            subject: Some(request.subject.clone()),
            validation_errors: vec![],
        }),
    }
}

// ---------------------------------------------------------------------------
// 공개 API
// ---------------------------------------------------------------------------

/// dry-run 미리보기를 생성한다. 어떤 네트워크 요청도 보내지 않는다.
///
/// 내부에서 [`build_article_write_body_with_content`]를 호출하여 요청 바디를 완전히 조립하고,
/// 결과에서 확정된 옵션 값을 추출해 [`PostPreview`]를 반환한다.
/// 이 방식으로 기본값 로직을 빌더 한 곳에만 유지한다.
///
/// # Errors
/// contentJson 또는 요청 바디 직렬화에 실패하면 [`PostError`]를 반환한다.
pub fn build_post_preview(
    request: &PostRequest,
    ids: &mut impl IdProvider,
) -> Result<PostPreview, PostError> {
    // Step 3 + Step 4: contentJson 생성 → ArticleWriteBody 조립
    let body =
        build_article_write_body_with_content(request, ids)
            .map_err(|e| serde_err_to_post_error(request, e))?;

    let article = &body.article;

    // 페이로드 길이 — 직렬화 자체를 검증하기도 함
    let content_json_length = article.content_json.len();
    let request_body_json =
        serde_json::to_string(&body).map_err(|e| serde_err_to_post_error(request, e))?;
    let request_body_length = request_body_json.len();

    // 요청 메타데이터
    let path = article_post_path(&request.cafe_id, request.menu_id);
    let headers = article_post_headers(&request.cafe_id);

    // 본문 줄 수 — lines() 는 빈 마지막 개행을 제외하므로 split('\n')을 사용
    let body_line_count = if request.body_text.is_empty() {
        0
    } else {
        request.body_text.split('\n').count()
    };

    Ok(PostPreview {
        cafe_id: request.cafe_id.clone(),
        menu_id: request.menu_id,
        subject: request.subject.clone(),
        body_text: request.body_text.clone(),
        body_line_count,
        tag_list: request.tag_list.clone(),
        // 확정된 옵션 값 — 빌더에서 파생, 이 파일에서 기본값을 중복 정의하지 않음
        open: article.open,
        naver_open: article.naver_open,
        external_open: article.external_open,
        enable_comment: article.enable_comment,
        enable_scrap: article.enable_scrap,
        enable_copy: article.enable_copy,
        method: "POST".to_string(),
        host: API_HOST.to_string(),
        path,
        headers,
        content_json_length,
        request_body_length,
    })
}

/// 실행 진입점. 모드 기본값은 [`PostExecutionMode::DryRun`].
///
/// - [`PostExecutionMode::DryRun`]: 미리보기만 생성(전송 없음).
/// - [`PostExecutionMode::Live`]: 아직 미구현 — 세션/HTTP 계층 필요.
///   호출 즉시 [`PostError`](`CODE_LIVE_SEND_NOT_IMPLEMENTED`)를 반환한다.
///
/// # Errors
/// - `DryRun`에서 직렬화 실패 시 [`PostError`]를 반환한다.
/// - `Live`는 항상 `Err(PostError { code: "LIVE_SEND_NOT_IMPLEMENTED" })`를 반환한다.
pub fn execute_post(
    request: &PostRequest,
    mode: PostExecutionMode,
    ids: &mut impl IdProvider,
) -> Result<PostOutcome, PostError> {
    match mode {
        PostExecutionMode::DryRun => {
            let preview = build_post_preview(request, ids)?;
            Ok(PostOutcome::Preview(preview))
        }
        PostExecutionMode::Live => {
            // ⛔ 실제 글 작성 전송은 아직 구현되지 않았습니다(세션/HTTP 계층 필요).
            // 이 분기는 의도적으로 네트워크 요청을 발생시키지 않습니다.
            Err(ErrorEnvelope {
                trace_id: String::new(),
                code: CODE_LIVE_SEND_NOT_IMPLEMENTED.to_string(),
                message:
                    "실제 글 작성 전송은 아직 구현되지 않았습니다(세션/HTTP 계층 필요)."
                        .to_string(),
                error_data: Some(PostErrorData {
                    cafe: NaverCafeCommonErrorData {
                        target: Some(CafeTarget {
                            cafe_id: request.cafe_id.clone(),
                            cafe_name: None,
                            menu_id: Some(request.menu_id),
                            article_id: None,
                            ref_comment_id: None,
                        }),
                        http_status: None,
                        api_error_code: None,
                        api_error_message: None,
                        retryable: false,
                    },
                    menu_id: Some(request.menu_id),
                    subject: Some(request.subject.clone()),
                    validation_errors: vec![],
                }),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::post::smart_editor::SequentialIdProvider;

    fn base_request() -> PostRequest {
        PostRequest {
            cafe_id: "31732304".to_string(),
            menu_id: 1,
            subject: "rust".to_string(),
            body_text: "rust 본문".to_string(),
            tag_list: vec!["RUST".to_string(), "C".to_string()],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        }
    }

    // ------------------------------------------------------------------
    // build_post_preview — 기본 속성 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_subject_target_tags_match_request() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");

        assert_eq!(preview.cafe_id, "31732304");
        assert_eq!(preview.menu_id, 1);
        assert_eq!(preview.subject, "rust");
        assert_eq!(preview.tag_list, vec!["RUST", "C"]);
    }

    // ------------------------------------------------------------------
    // build_post_preview — 기본값(None → 캡처 기본값) 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_none_options_produce_captured_defaults() {
        let req = base_request(); // 모든 옵션 None
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");

        assert!(!preview.open, "open 기본값은 false");
        assert!(preview.naver_open, "naverOpen 기본값은 true");
        assert!(preview.external_open, "externalOpen 기본값은 true");
        assert!(preview.enable_comment, "enableComment 기본값은 true");
        assert!(!preview.enable_scrap, "enableScrap 기본값은 false");
        assert!(!preview.enable_copy, "enableCopy 기본값은 false");
    }

    // ------------------------------------------------------------------
    // build_post_preview — Some 오버라이드 반영 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_some_options_override_defaults() {
        let mut req = base_request();
        req.open = Some(true);
        req.enable_comment = Some(false);
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");

        assert!(preview.open, "open=Some(true)이 반영되어야 함");
        assert!(!preview.enable_comment, "enableComment=Some(false)이 반영되어야 함");
    }

    // ------------------------------------------------------------------
    // build_post_preview — 요청 메타데이터 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_method_is_post() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(preview.method, "POST");
    }

    #[test]
    fn preview_host_is_api_host() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(preview.host, API_HOST);
    }

    #[test]
    fn preview_path_is_correct() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(
            preview.path,
            "/editor/v2.0/cafes/31732304/menus/1/articles"
        );
    }

    #[test]
    fn preview_referer_header_contains_menus_0() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");

        let referer = preview
            .headers
            .iter()
            .find(|(name, _)| name == "Referer")
            .map(|(_, val)| val.as_str())
            .expect("Referer 헤더가 없음");

        assert!(
            referer.contains("menus/0"),
            "Referer는 리터럴 menus/0을 포함해야 함: {}",
            referer
        );
    }

    // ------------------------------------------------------------------
    // build_post_preview — contentJson 길이 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_content_json_length_is_nonzero() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert!(
            preview.content_json_length > 0,
            "contentJsonLength는 0보다 커야 함"
        );
    }

    // ------------------------------------------------------------------
    // build_post_preview — 본문 줄 수 검증
    // ------------------------------------------------------------------

    #[test]
    fn preview_body_line_count_single_line() {
        let req = base_request(); // body_text = "rust 본문" (줄바꿈 없음)
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(preview.body_line_count, 1);
    }

    #[test]
    fn preview_body_line_count_multi_line() {
        let mut req = base_request();
        req.body_text = "첫 번째 줄\n두 번째 줄\n세 번째 줄".to_string();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(preview.body_line_count, 3, "줄바꿈 2개 → 3줄");
    }

    #[test]
    fn preview_body_line_count_empty() {
        let mut req = base_request();
        req.body_text = String::new();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        assert_eq!(preview.body_line_count, 0, "빈 본문 → 0줄");
    }

    // ------------------------------------------------------------------
    // execute_post — 기본 모드 DryRun 검증
    // ------------------------------------------------------------------

    #[test]
    fn execute_post_default_mode_is_dry_run() {
        assert_eq!(PostExecutionMode::default(), PostExecutionMode::DryRun);
    }

    #[test]
    fn execute_post_dry_run_returns_preview_outcome() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let result = execute_post(&req, PostExecutionMode::default(), &mut ids);
        assert!(result.is_ok(), "DryRun은 Ok여야 함");
        match result.unwrap() {
            PostOutcome::Preview(_) => {}
        }
    }

    // ------------------------------------------------------------------
    // execute_post — Live 모드 미구현 게이팅 검증
    // ------------------------------------------------------------------

    #[test]
    fn execute_post_live_returns_not_implemented_error() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let result = execute_post(&req, PostExecutionMode::Live, &mut ids);
        assert!(result.is_err(), "Live는 Err여야 함(미구현)");
        let err = result.unwrap_err();
        assert_eq!(
            err.code, CODE_LIVE_SEND_NOT_IMPLEMENTED,
            "오류 코드가 LIVE_SEND_NOT_IMPLEMENTED여야 함"
        );
    }

    // ------------------------------------------------------------------
    // PostPreview 직렬화 / 역직렬화 (sanity round-trip)
    // ------------------------------------------------------------------

    #[test]
    fn preview_serializes_without_panic() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        let _json = serde_json::to_string(&preview).expect("직렬화 실패");
    }

    #[test]
    fn preview_round_trips_through_json() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let preview = build_post_preview(&req, &mut ids).expect("미리보기 생성 실패");
        let json = serde_json::to_string(&preview).expect("직렬화 실패");
        let restored: PostPreview = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(preview, restored);
    }
}
