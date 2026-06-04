//! 네이버 카페 게시글 작성 서비스 — dry-run 미리보기 및 실행 진입점.
//!
//! 기본 실행 모드는 [`PostExecutionMode::DryRun`]이며,
//! Live 전송은 비동기 진입점인 [`execute_post_live`]를 사용한다.
//!
//! # 동기 vs 비동기
//! - [`execute_post`]: 동기 함수. `DryRun` 미리보기만 반환한다.
//!   `Live` 모드를 전달하면 `USE_ASYNC_LIVE` 오류를 반환한다.
//! - [`execute_post_live`]: 비동기 함수. reqwest로 실제 HTTP POST를 전송한다.

use serde::{Deserialize, Serialize};

use super::{
    client::{cookie_header_from_storage_state, CafeHttpClient, CODE_SESSION_INVALID},
    error::{PostError, PostErrorData},
    models::PostRequest,
    parser::ArticleRegisterResult,
    request_builder::{
        article_post_headers, article_post_path, build_article_write_body_with_content, API_HOST,
    },
    smart_editor::IdProvider,
};
use crate::{
    auth,
    naver_cafe::{
        error::{ErrorEnvelope, NaverCafeCommonErrorData},
        models::CafeTarget,
    },
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

/// `execute_post(Live)` 호출 시 반환되는 오류 코드.
/// Live 전송은 비동기 함수 [`execute_post_live`]를 사용해야 한다.
pub const CODE_LIVE_SEND_NOT_IMPLEMENTED: &str = "LIVE_SEND_NOT_IMPLEMENTED";

/// `execute_post`에 `Live` 모드를 전달하면 반환되는 리다이렉트 오류 코드.
/// 호출자는 대신 [`execute_post_live`]를 사용해야 한다.
pub const CODE_USE_ASYNC_LIVE: &str = "USE_ASYNC_LIVE";

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// `serde_json::Error`를 `PostError`로 변환한다.
fn serde_err_to_post_error(request: &PostRequest, err: serde_json::Error) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_CONTENT_BUILD_FAILED.to_string(),
        message: format!("contentJson 또는 요청 바디 직렬화에 실패했습니다: {}", err),
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
    let body = build_article_write_body_with_content(request, ids)
        .map_err(|e| serde_err_to_post_error(request, e))?;

    let article = &body.article;

    let content_json_length = article.content_json.len();
    let request_body_json =
        serde_json::to_string(&body).map_err(|e| serde_err_to_post_error(request, e))?;
    let request_body_length = request_body_json.len();

    let path = article_post_path(&request.cafe_id, request.menu_id);
    let headers = article_post_headers(&request.cafe_id, &request.board_type);

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

/// 동기 실행 진입점. 모드 기본값은 [`PostExecutionMode::DryRun`].
///
/// - [`PostExecutionMode::DryRun`]: 미리보기만 생성 (전송 없음).
/// - [`PostExecutionMode::Live`]: `USE_ASYNC_LIVE` 오류를 즉시 반환한다.
///   실제 HTTP 전송이 필요한 경우 비동기 함수 [`execute_post_live`]를 사용하라.
///
/// # Errors
/// - `DryRun`에서 직렬화 실패 시 [`PostError`]를 반환한다.
/// - `Live`는 항상 `Err(PostError { code: "USE_ASYNC_LIVE" })`를 반환한다.
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
            // Live 전송은 execute_post_live(비동기)를 사용해야 한다.
            // 동기 컨텍스트에서 실수로 Live를 호출하는 경우를 안내한다.
            Err(ErrorEnvelope {
                trace_id: String::new(),
                code: CODE_USE_ASYNC_LIVE.to_string(),
                message: "Live 전송은 execute_post_live 비동기 함수를 사용하세요.".to_string(),
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

/// 비동기 Live 전송 진입점 — 실제 HTTP POST 요청을 네이버 카페 API에 전송한다.
///
/// 1. `request`에서 contentJson + ArticleWriteBody를 조립한다(Step 3/4).
/// 2. `account_id`로 Playwright storage-state 쿠키를 읽는다.
///    유효한 쿠키가 없으면 `SESSION_INVALID` 오류를 반환한다.
/// 3. [`cookie_header_from_storage_state`]로 Cookie 헤더를 생성한다.
///    쿠키 값은 절대 에러/로그에 노출되지 않는다.
/// 4. [`CafeHttpClient::new`]로 실제 네이버 API에 요청을 전송한다.
///
/// # Errors
/// - 직렬화 실패: `CONTENT_BUILD_FAILED`
/// - 쿠키 없음/만료: `SESSION_INVALID`
/// - HTTP 오류: `REGISTER_HTTP_ERROR` (원본 응답 바디 보존)
/// - 2xx 파싱 실패: `REGISTER_PARSE_ERROR` (원본 응답 바디 보존)
pub async fn execute_post_live(
    request: &PostRequest,
    account_id: &str,
    ids: &mut impl IdProvider,
) -> Result<ArticleRegisterResult, PostError> {
    let body = build_article_write_body_with_content(request, ids)
        .map_err(|e| serde_err_to_post_error(request, e))?;

    let cookies_value = auth::read_account_cookies(account_id)
        .map_err(|e| ErrorEnvelope {
            trace_id: String::new(),
            code: CODE_SESSION_INVALID.to_string(),
            message: format!("계정 쿠키를 읽는 데 실패했습니다: {}", e),
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
        })?
        .ok_or_else(|| ErrorEnvelope {
            trace_id: String::new(),
            code: CODE_SESSION_INVALID.to_string(),
            message: "세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.".to_string(),
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
        })?;

    // 보안: Cookie 헤더 값을 로그/에러에 노출하지 않는다.
    let cookie_header = cookie_header_from_storage_state(&cookies_value);

    let client = CafeHttpClient::new();
    client
        .post_article(
            &request.cafe_id,
            request.menu_id,
            &request.board_type,
            &body,
            cookie_header.as_deref(),
        )
        .await
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naver_cafe::post::{client::CafeHttpClient, smart_editor::SequentialIdProvider};

    fn base_request() -> PostRequest {
        PostRequest {
            cafe_id: "31732304".to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
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
        assert!(
            !preview.enable_comment,
            "enableComment=Some(false)이 반영되어야 함"
        );
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
        assert_eq!(preview.path, "/editor/v2.0/cafes/31732304/menus/1/articles");
    }

    #[test]
    fn preview_referer_header_uses_write_url_with_board_type_l() {
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
            referer.contains("articles/write?boardType=L"),
            "Referer는 articles/write?boardType=L을 포함해야 함: {}",
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
    // execute_post — Live 모드는 execute_post_live 사용 안내
    // ------------------------------------------------------------------

    #[test]
    fn execute_post_live_returns_use_async_live_error() {
        let req = base_request();
        let mut ids = SequentialIdProvider::new();
        let result = execute_post(&req, PostExecutionMode::Live, &mut ids);
        assert!(result.is_err(), "Live는 Err여야 함(동기 미지원)");
        let err = result.unwrap_err();
        assert_eq!(
            err.code, CODE_USE_ASYNC_LIVE,
            "오류 코드가 USE_ASYNC_LIVE여야 함"
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

    // ------------------------------------------------------------------
    // 실제 네이버 서버 진단 테스트 — 기본 실행에서 제외됨
    // ------------------------------------------------------------------

    /// 만료된(또는 유효하지 않은) 쿠키를 **만료 검증 없이** 실제 네이버 카페 API로 전송해
    /// 서버 실패 응답의 실제 형태(http_status, api_error_message)를 출력한다.
    ///
    /// 이 테스트는 `cargo test` 기본 실행에서 제외된다.
    /// 수동 실행 명령:
    /// ```
    /// PSTMACRO_LIVE_ACCOUNT_ID=<계정ID> \
    /// PSTMACRO_LIVE_CAFE_ID=<카페ID> \
    /// PSTMACRO_LIVE_MENU_ID=<메뉴ID> \
    /// cargo test -p pstmacro live_expired_cookie_sends_real_request -- --ignored --nocapture
    /// ```
    ///
    /// # 보안 주의
    /// 쿠키 헤더 값 및 쿠키 내용은 절대 출력하지 않는다.
    /// 요청 대상(카페 ID, 메뉴 ID)과 응답 결과만 출력한다.
    #[tokio::test]
    #[ignore = "실제 네이버로 요청을 보냄. `cargo test ... -- --ignored --nocapture` 로만 수동 실행"]
    async fn live_expired_cookie_sends_real_request() {
        // --- 환경 변수에서 설정 읽기 ---
        let account_id = std::env::var("PSTMACRO_LIVE_ACCOUNT_ID").unwrap_or_else(|_| {
            panic!(
                "필수 환경 변수가 없습니다. 다음과 같이 설정 후 재실행하세요:\n\
                 PSTMACRO_LIVE_ACCOUNT_ID=<계정ID> \\\n\
                 PSTMACRO_LIVE_CAFE_ID=<카페ID> \\\n\
                 PSTMACRO_LIVE_MENU_ID=<메뉴ID> \\\n\
                 cargo test -p pstmacro live_expired_cookie_sends_real_request -- --ignored --nocapture"
            )
        });
        let cafe_id = std::env::var("PSTMACRO_LIVE_CAFE_ID").unwrap_or_else(|_| {
            panic!(
                "필수 환경 변수 PSTMACRO_LIVE_CAFE_ID가 없습니다. \
                 PSTMACRO_LIVE_ACCOUNT_ID / PSTMACRO_LIVE_CAFE_ID / PSTMACRO_LIVE_MENU_ID 를 모두 설정하세요."
            )
        });
        let menu_id_str = std::env::var("PSTMACRO_LIVE_MENU_ID").unwrap_or_else(|_| {
            panic!(
                "필수 환경 변수 PSTMACRO_LIVE_MENU_ID가 없습니다. \
                 PSTMACRO_LIVE_ACCOUNT_ID / PSTMACRO_LIVE_CAFE_ID / PSTMACRO_LIVE_MENU_ID 를 모두 설정하세요."
            )
        });
        let menu_id: u64 = menu_id_str
            .parse()
            .expect("PSTMACRO_LIVE_MENU_ID는 숫자(u64)여야 합니다");
        let subject = std::env::var("PSTMACRO_LIVE_SUBJECT").unwrap_or_else(|_| "test".to_string());
        let body = std::env::var("PSTMACRO_LIVE_BODY").unwrap_or_else(|_| "test body".to_string());
        let board_type =
            std::env::var("PSTMACRO_LIVE_BOARD_TYPE").unwrap_or_else(|_| "L".to_string());

        // --- 쿠키 상태 확인 (유효성 검증 포함 경로) ---
        // 보안: 쿠키 값 자체는 출력하지 않고 유효/만료 여부만 출력한다.
        let cookie_status_label = match auth::read_account_cookies(&account_id) {
            Ok(Some(_)) => "유효(valid)",
            Ok(None) => "없음/만료(invalid)",
            Err(e) => {
                println!("[진단] 쿠키 상태 확인 중 오류: {}", e);
                "확인 불가"
            }
        };
        println!(
            "[진단] 계정='{}' 쿠키 상태(유효성 검증 포함): {}",
            account_id, cookie_status_label
        );

        // --- 만료 검증 없이 쿠키 읽기 ---
        let cookies_value = auth::read_account_cookies_unchecked(&account_id)
            .unwrap_or_else(|e| panic!("쿠키 파일 읽기 오류: {}", e));
        let cookies_value = cookies_value.unwrap_or_else(|| {
            panic!(
                "쿠키 파일이 없습니다. 계정 '{}' 의 쿠키 파일이 존재해야 합니다.",
                account_id
            )
        });

        // --- Cookie 헤더 생성 ---
        // 보안: cookie_header 값은 절대 출력하지 않는다.
        let cookie_header =
            crate::naver_cafe::post::client::cookie_header_from_storage_state(&cookies_value);

        // --- 요청 구성 ---
        let request = PostRequest {
            cafe_id: cafe_id.clone(),
            menu_id,
            board_type: board_type.clone(),
            subject,
            body_text: body,
            tag_list: vec![],
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };

        let mut ids = SequentialIdProvider::default();
        let body =
            build_article_write_body_with_content(&request, &mut ids).expect("요청 바디 생성 실패");

        println!(
            "[진단] 요청 대상: cafe_id='{}', menu_id={}",
            cafe_id, menu_id
        );

        // --- 실제 전송 ---
        let client = CafeHttpClient::new();
        let result = client
            .post_article(
                &cafe_id,
                menu_id,
                &board_type,
                &body,
                cookie_header.as_deref(),
            )
            .await;

        // --- 결과 출력 (성공/실패 모두 허용, assert 없음) ---
        // 보안: 쿠키 헤더나 쿠키 값은 여기서도 출력하지 않는다.
        match result {
            Ok(register_result) => {
                println!("[결과] 성공: {:?}", register_result);
            }
            Err(post_error) => {
                let pretty = serde_json::to_string_pretty(&post_error)
                    .unwrap_or_else(|_| format!("{:?}", post_error));
                println!("[결과] 실패:\n{}", pretty);
            }
        }
    }
}
