//! 카페 글 작성 오케스트레이터 — 내부 서비스 계층 (B 방향).
//!
//! `URL/슬러그 → cafeId → 게시판 목록 → 게시판 1개 선택 → 글 작성` 흐름을
//! 일반 async 함수로 묶는다. UI 작업이 진행 중이므로 지금은 Tauri command(A)로
//! 노출하지 않고 내부 서비스 함수로만 둔다. 추후 [`CafeOrchestrator`] 메서드와
//! [`run_post_jobs`]를 그대로 `#[tauri::command]`로 감싸면 A 방향으로 전환된다.
//!
//! # 두 계층
//! - **디스커버리**(UI 드롭다운 채우기용): [`CafeOrchestrator::resolve_cafe_id`],
//!   [`CafeOrchestrator::fetch_cafe_info`], [`CafeOrchestrator::list_boards`].
//! - **실행**: [`CafeOrchestrator::post_one`] (단건),
//!   [`run_post_jobs`] (계정 쿠키를 읽어 N건을 순차 실행).
//!
//! # 계정 ↔ 카페 매핑
//! [`PostJob`]은 `(계정, 카페, 게시판, 내용)` 단위의 평면 리스트다. 같은 카페에
//! 여러 계정이 쓰거나, 계정마다 다른 카페에 쓰는 두 경우를 모두 표현한다.
//!
//! # 오류 처리
//! [`run_post_jobs`]는 한 건이 실패해도 중단하지 않고 **건너뛴 뒤 계속**한다.
//! 각 건의 성공/실패는 [`JobReport`]로 보고된다.
//!
//! # 쿠키 보안
//! 쿠키 값은 로그·에러·`Debug` 출력에 절대 포함되지 않는다.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use ts_rs::TS;

use crate::auth;
use crate::naver_cafe::{
    cafe_ref::{
        parse_cafe_ref, CafeGateClient, CafeHomeClient, CafeInfoView, CafeRef, CafeRefError,
    },
    comment::{CafeCommentClient, CommentError, CommentErrorData, CommentRequest, CommentResult},
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    joined_cafes::{JoinedCafe, JoinedCafesClient, JoinedCafesError},
    menu::{CafeMenuClient, Menu, MenuError},
    post::{
        build_article_write_body_with_content, cookie_header_from_storage_state,
        ArticleRegisterResult, CafeHttpClient, PostError, PostErrorData, PostRequest,
        SequentialIdProvider,
    },
};

// ---------------------------------------------------------------------------
// 오류 코드
// ---------------------------------------------------------------------------

/// 카페 입력 문자열에서 식별자를 전혀 파싱하지 못한 경우의 오류 코드.
pub const CODE_INVALID_CAFE_INPUT: &str = "INVALID_CAFE_INPUT";

/// 계정의 유효한 세션 쿠키가 없을 때의 오류 코드.
pub const CODE_NO_COOKIES: &str = "NO_COOKIES";

// ---------------------------------------------------------------------------
// 입력/출력 모델 (serde 직렬화 — 추후 Tauri command 인자/반환에 그대로 사용)
// ---------------------------------------------------------------------------

/// 글 작성 작업 1건 — `(계정, 카페, 게시판, 내용)` 조합.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PostJob {
    /// 사용할 계정 ID (쿠키 파일 조회 키).
    pub account_id: String,
    /// 대상 카페 — URL, vanity 슬러그, 또는 숫자 cafeId 문자열 모두 허용.
    pub cafe: String,
    /// 선택된 게시판(메뉴) ID. (JS `number` — [`crate::ipc::cafes::Board`] 참고)
    #[ts(type = "number")]
    pub menu_id: u64,
    /// 선택된 게시판의 `boardType` (예: `"L"`) — 글쓰기 Referer 헤더에 사용.
    pub board_type: String,
    /// 게시글 제목.
    pub subject: String,
    /// 게시글 본문 텍스트.
    pub body_text: String,
    /// 태그 목록.
    #[serde(default)]
    pub tag_list: Vec<String>,
}

/// 작업 1건의 실행 결과 보고.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobReport {
    /// 작업에 사용된 계정 ID.
    pub account_id: String,
    /// 작업 대상 카페 입력값(원본 문자열).
    pub cafe: String,
    /// 게시판(메뉴) ID.
    pub menu_id: u64,
    /// 성공 여부.
    pub success: bool,
    /// 성공 시 등록 결과 (실패 시 `None`).
    pub result: Option<ArticleRegisterResult>,
    /// 실패 시 오류 (성공 시 `None`).
    pub error: Option<PostError>,
}

impl JobReport {
    fn success(job: &PostJob, result: ArticleRegisterResult) -> Self {
        Self {
            account_id: job.account_id.clone(),
            cafe: job.cafe.clone(),
            menu_id: job.menu_id,
            success: true,
            result: Some(result),
            error: None,
        }
    }

    fn failure(job: &PostJob, error: PostError) -> Self {
        Self {
            account_id: job.account_id.clone(),
            cafe: job.cafe.clone(),
            menu_id: job.menu_id,
            success: false,
            result: None,
            error: Some(error),
        }
    }
}

/// 댓글 작성 작업 1건 — `(계정, 카페, 게시글, 내용)` 조합.
///
/// [`PostJob`]과 달리 카페/게시글을 **숫자 ID로 이미 확정한 상태**로 받는다.
/// (글 작성 직후의 `articleId` 재사용, 또는 URL 파싱 결과를 UI가 채운다.)
/// 이 단계(척추)는 카페 해석·글목록 조회를 일절 하지 않는다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct CommentJob {
    /// 사용할 계정 ID (쿠키 파일 조회 키).
    pub account_id: String,
    /// 대상 카페의 숫자 ID. (JS `number` — [`PostJob::menu_id`] 참고)
    #[ts(type = "number")]
    pub cafe_id: u64,
    /// 댓글을 달 대상 게시글의 숫자 ID.
    #[ts(type = "number")]
    pub article_id: u64,
    /// 댓글 본문.
    pub content: String,
}

/// 댓글 작업 1건의 실행 결과 보고(내부용).
///
/// IPC 경계는 슬림한 `CommentPublishOutcome`([`crate::ipc::cafes`])가 담당하며,
/// 풍부한 [`CommentResult`]/[`CommentError`]는 백엔드에만 머문다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentJobReport {
    /// 작업에 사용된 계정 ID.
    pub account_id: String,
    /// 대상 카페 ID.
    pub cafe_id: u64,
    /// 대상 게시글 ID.
    pub article_id: u64,
    /// 성공 여부.
    pub success: bool,
    /// 성공 시 등록 결과 (실패 시 `None`).
    pub result: Option<CommentResult>,
    /// 실패 시 오류 (성공 시 `None`).
    pub error: Option<CommentError>,
    /// 단 댓글 본문(게시 결과 표시용).
    pub content: String,
}

impl CommentJobReport {
    fn success(job: &CommentJob, result: CommentResult) -> Self {
        Self {
            account_id: job.account_id.clone(),
            cafe_id: job.cafe_id,
            article_id: job.article_id,
            success: true,
            result: Some(result),
            error: None,
            content: job.content.clone(),
        }
    }

    fn failure(job: &CommentJob, error: CommentError) -> Self {
        Self {
            account_id: job.account_id.clone(),
            cafe_id: job.cafe_id,
            article_id: job.article_id,
            success: false,
            result: None,
            error: Some(error),
            content: job.content.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// 오류 변환 헬퍼
// ---------------------------------------------------------------------------

/// [`CafeRefError`]를 [`PostError`]로 변환한다(코드/메시지/공통 데이터 보존).
fn cafe_ref_err_to_post_error(err: CafeRefError) -> PostError {
    ErrorEnvelope {
        trace_id: err.trace_id,
        code: err.code,
        message: err.message,
        error_data: Some(PostErrorData {
            cafe: err.error_data.unwrap_or_else(empty_common_error),
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// [`MenuError`]를 [`PostError`]로 변환한다(게시 직전 boardType 해석 단계의 실패).
/// 코드/메시지/공통 데이터를 보존해, 글 게시 실패와 동일한 사유 매핑·자세히 보기를 탄다.
fn menu_err_to_post_error(err: MenuError) -> PostError {
    ErrorEnvelope {
        trace_id: err.trace_id,
        code: err.code,
        message: err.message,
        error_data: Some(PostErrorData {
            cafe: err.error_data.unwrap_or_else(empty_common_error),
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// 선택한 메뉴를 게시판 목록에서 찾지 못했을 때의 [`PostError`]. boardType을 해석할 수
/// 없으므로(기본값 강제 금지) 명시적 실패로 끝낸다.
fn menu_not_found_post_error(menu_id: u64) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_INVALID_CAFE_INPUT.to_string(),
        message: format!("선택한 게시판(menuId={menu_id})을 카페 게시판 목록에서 찾지 못했습니다"),
        error_data: Some(PostErrorData {
            cafe: empty_common_error(),
            menu_id: Some(menu_id),
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// `serde_json::Error`를 [`PostError`]로 변환한다.
fn serde_err_to_post_error(err: serde_json::Error) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: "CONTENT_BUILD_FAILED".to_string(),
        message: format!("요청 바디 직렬화에 실패했습니다: {}", err),
        error_data: Some(PostErrorData {
            cafe: empty_common_error(),
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

fn no_cookies_error(account_id: &str, detail: Option<String>) -> PostError {
    let message = match detail {
        Some(d) => format!("계정 '{}'의 쿠키를 읽지 못했습니다: {}", account_id, d),
        None => format!(
            "계정 '{}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.",
            account_id
        ),
    };
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_NO_COOKIES.to_string(),
        message,
        error_data: Some(PostErrorData {
            cafe: empty_common_error(),
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// 댓글 작업용 NO_COOKIES 오류를 만든다([`no_cookies_error`]의 [`CommentError`] 버전).
/// 쿠키 값은 절대 포함하지 않는다.
fn no_cookies_comment_error(account_id: &str, detail: Option<String>) -> CommentError {
    let message = match detail {
        Some(d) => format!("계정 '{}'의 쿠키를 읽지 못했습니다: {}", account_id, d),
        None => format!(
            "계정 '{}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.",
            account_id
        ),
    };
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_NO_COOKIES.to_string(),
        message,
        error_data: Some(CommentErrorData {
            cafe: empty_common_error(),
            article_id: None,
            ref_comment_id: None,
            validation_errors: vec![],
        }),
    }
}

fn empty_common_error() -> NaverCafeCommonErrorData {
    NaverCafeCommonErrorData {
        target: None,
        http_status: None,
        api_error_code: None,
        api_error_message: None,
        retryable: false,
    }
}

// ---------------------------------------------------------------------------
// 오케스트레이터
// ---------------------------------------------------------------------------

/// 카페 디스커버리 + 글 작성 클라이언트 묶음.
///
/// 테스트에서는 [`CafeOrchestrator::with_base_url`]로 모든 하위 클라이언트를
/// 같은 wiremock 서버로 향하게 할 수 있다(경로가 서로 달라 구분된다).
pub struct CafeOrchestrator {
    home: CafeHomeClient,
    gate: CafeGateClient,
    menu: CafeMenuClient,
    post: CafeHttpClient,
    joined: JoinedCafesClient,
}

impl CafeOrchestrator {
    /// 실제 네이버 호스트를 사용하는 오케스트레이터를 생성한다.
    pub fn new() -> Self {
        Self {
            home: CafeHomeClient::new(),
            gate: CafeGateClient::new(),
            menu: CafeMenuClient::new(),
            post: CafeHttpClient::new(),
            joined: JoinedCafesClient::new(),
        }
    }

    /// 모든 하위 클라이언트를 주입된 `base_url`로 향하게 한다(테스트용).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        Self {
            home: CafeHomeClient::with_base_url(base.clone()),
            gate: CafeGateClient::with_base_url(base.clone()),
            menu: CafeMenuClient::with_base_url(base.clone()),
            post: CafeHttpClient::with_base_url(base.clone()),
            joined: JoinedCafesClient::with_base_url(base),
        }
    }

    // ---- 디스커버리 ----

    /// 카페 입력 문자열(URL/슬러그/숫자)을 숫자 cafeId로 해석한다.
    ///
    /// - 숫자 id를 직접 추출 가능 → 그대로 반환
    /// - vanity 슬러그 → 카페 홈 HTML을 받아 해석([`CafeHomeClient::resolve_slug`])
    /// - 어느 패턴도 아님 → `INVALID_CAFE_INPUT`
    pub async fn resolve_cafe_id(
        &self,
        input: &str,
        cookie_header: Option<&str>,
    ) -> Result<u64, CafeRefError> {
        match parse_cafe_ref(input) {
            Some(CafeRef::Id(id)) => Ok(id),
            Some(CafeRef::Vanity(slug)) => self.home.resolve_slug(&slug, cookie_header).await,
            None => Err(ErrorEnvelope {
                trace_id: String::new(),
                code: CODE_INVALID_CAFE_INPUT.to_string(),
                message: format!("카페 식별자를 인식하지 못했습니다: {:?}", input),
                error_data: Some(empty_common_error()),
            }),
        }
    }

    /// 숫자 cafeId로 카페 기본 정보(이름/슬러그 등)를 조회한다(UI 표시용).
    pub async fn fetch_cafe_info(
        &self,
        cafe_id: u64,
        cookie_header: Option<&str>,
    ) -> Result<CafeInfoView, CafeRefError> {
        self.gate.fetch_gate_info(cafe_id, cookie_header).await
    }

    /// 카페의 일반 게시판(글쓰기 가능) 목록을 조회한다(UI 게시판 선택용).
    pub async fn list_boards(
        &self,
        cafe_id: u64,
        cookie_header: Option<&str>,
    ) -> Result<Vec<Menu>, MenuError> {
        self.menu
            .fetch_general_writable_boards(&cafe_id.to_string(), cookie_header)
            .await
    }

    /// 현재 로그인 계정이 가입한 카페 목록을 조회한다(모든 페이지).
    pub async fn list_joined_cafes(
        &self,
        cookie_header: Option<&str>,
    ) -> Result<Vec<JoinedCafe>, JoinedCafesError> {
        self.joined.fetch_joined_cafes(cookie_header).await
    }

    // ---- 실행 ----

    /// 작업 1건을 실행한다 — 카페 해석 → 글 작성.
    ///
    /// 쿠키는 호출자가 주입한다([`run_post_jobs`]가 계정별로 읽어 전달).
    pub async fn post_one(
        &self,
        job: &PostJob,
        cookie_header: Option<&str>,
    ) -> Result<ArticleRegisterResult, PostError> {
        let cafe_id = self
            .resolve_cafe_id(&job.cafe, cookie_header)
            .await
            .map_err(cafe_ref_err_to_post_error)?;

        // boardType이 비어 있으면(예약 시점에 동결하지 못한 레거시/UI 경로) 게시 직전에
        // 게시판 목록을 조회해 menuId와 일치하는 메뉴의 boardType을 채운다. 기본값("L")을
        // 강제하지 않고, 일치 메뉴가 없으면 명시적으로 실패한다. 이미 채워져 있으면(기존
        // 동작) 추가 조회 없이 그대로 쓴다(하위호환).
        let board_type = if job.board_type.is_empty() {
            let boards = self
                .list_boards(cafe_id, cookie_header)
                .await
                .map_err(menu_err_to_post_error)?;
            boards
                .into_iter()
                .find(|m| m.menu_id == job.menu_id)
                .map(|m| m.board_type)
                .ok_or_else(|| menu_not_found_post_error(job.menu_id))?
        } else {
            job.board_type.clone()
        };

        let request = PostRequest {
            cafe_id: cafe_id.to_string(),
            menu_id: job.menu_id,
            board_type,
            subject: job.subject.clone(),
            body_text: job.body_text.clone(),
            tag_list: job.tag_list.clone(),
            open: None,
            naver_open: None,
            external_open: None,
            enable_comment: None,
            enable_scrap: None,
            enable_copy: None,
        };

        let mut ids = SequentialIdProvider::new();
        let body = build_article_write_body_with_content(&request, &mut ids)
            .map_err(serde_err_to_post_error)?;

        self.post
            .post_article(
                &request.cafe_id,
                request.menu_id,
                &request.board_type,
                &body,
                cookie_header,
            )
            .await
    }
}

impl Default for CafeOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 최상위 실행 함수 (계정 쿠키를 파일에서 읽어 N건 순차 실행)
// ---------------------------------------------------------------------------

/// N건의 작업을 순차 실행하고 각 건의 결과를 [`JobReport`]로 보고한다.
///
/// 각 작업마다 계정 쿠키를 읽어([`auth::read_account_cookies`]) 글 작성에 사용한다.
/// **한 건이 실패해도 중단하지 않고 다음 작업으로 넘어간다**(단순 건너뛰기).
///
/// 쿠키 값은 어떤 보고/로그에도 노출되지 않는다.
/// 계정 쿠키 해석 실패 사유 — 배치 캐시가 성공 헤더와 함께 보관한다.
#[derive(Clone)]
enum CookieFailure {
    /// 쿠키 없음(파일 없음, 또는 네이버 도메인 쿠키 미포함).
    Missing,
    /// 쿠키 파일 읽기/검증 오류(메시지 포함, 쿠키 값은 절대 포함하지 않음).
    Read(String),
}

/// 한 배치 동안 계정별로 해석한 쿠키 헤더를 캐시한다.
///
/// 같은 계정이 여러 건(글/댓글)을 가지면 [`auth::read_account_cookies`]로 같은 쿠키
/// 파일을 매번 읽고 파싱하던 것을 계정당 1회로 줄인다. 성공/실패 결과 모두 캐시하므로
/// 한 계정의 쿠키가 없으면 그 계정의 나머지 건도 즉시 동일하게 건너뛴다.
///
/// 보안: 쿠키 값은 이 캐시 메모리에만 머물며 로그·에러·`Debug`에 노출되지 않는다
/// (그래서 `Debug`를 파생하지 않는다).
#[derive(Default)]
struct CookieHeaderCache {
    by_account: std::collections::HashMap<String, Result<String, CookieFailure>>,
}

impl CookieHeaderCache {
    /// 계정의 쿠키 헤더를 반환한다(최초 1회만 파일을 읽고 이후엔 캐시). 성공 시 헤더
    /// 문자열의 복제본을 돌려준다 — 파일 I/O·JSON 파싱이 아니라 값 복제만 반복된다.
    fn resolve(&mut self, account_id: &str) -> Result<String, CookieFailure> {
        self.by_account
            .entry(account_id.to_string())
            .or_insert_with(|| match auth::read_account_cookies(account_id) {
                Ok(Some(value)) => {
                    cookie_header_from_storage_state(&value).ok_or(CookieFailure::Missing)
                }
                Ok(None) => Err(CookieFailure::Missing),
                Err(e) => Err(CookieFailure::Read(e.to_string())),
            })
            .clone()
    }
}

pub async fn run_post_jobs(jobs: &[PostJob]) -> Vec<JobReport> {
    run_post_jobs_with_progress(jobs, |_| {}).await
}

/// [`run_post_jobs`]와 같지만 글 1건을 끝낼 때마다 `on_each(누적_완료수)`를 호출해 진행률을
/// 보고한다 — 게시 큐 워커가 진행률을 0/N→1/N→…로 1건 단위로 갱신하는 데 쓴다(배치 완료만
/// 반영해 0/N에서 곧장 사라지지 않게).
pub async fn run_post_jobs_with_progress<F: FnMut(usize)>(
    jobs: &[PostJob],
    mut on_each: F,
) -> Vec<JobReport> {
    let orchestrator = CafeOrchestrator::new();
    let mut cookies = CookieHeaderCache::default();
    let total = jobs.len();
    let mut reports = Vec::with_capacity(total);
    for (index, job) in jobs.iter().enumerate() {
        // 각 글 등록의 시도/성공/실패를 tracing으로 남겨, 게시 실패 시 네이버 오류
        // 코드/사유를 로그에서 확인할 수 있게 한다(댓글 경로와 대칭). 쿠키 값·본문은
        // 절대 로그에 포함하지 않는다.
        tracing::info!(
            seq = index + 1,
            total,
            cafe = %job.cafe,
            menu_id = job.menu_id,
            "글 등록 시도"
        );
        // 계정 쿠키 해석(배치 내 1회 캐시). 없거나 오류면 건너뛴다.
        let report = match cookies.resolve(&job.account_id) {
            // 보안: cookie_header 값은 로그/보고에 노출하지 않는다.
            Ok(cookie_header) => {
                match orchestrator
                    .post_one(job, Some(cookie_header.as_str()))
                    .await
                {
                    Ok(result) => JobReport::success(job, result),
                    Err(err) => JobReport::failure(job, err),
                }
            }
            Err(CookieFailure::Missing) => {
                JobReport::failure(job, no_cookies_error(&job.account_id, None))
            }
            Err(CookieFailure::Read(msg)) => {
                JobReport::failure(job, no_cookies_error(&job.account_id, Some(msg)))
            }
        };
        match &report.error {
            None => tracing::info!(seq = index + 1, total, "글 등록 성공"),
            Some(err) => tracing::warn!(
                seq = index + 1,
                total,
                error_code = %err.code,
                error_message = %err.message,
                "글 등록 실패"
            ),
        }
        reports.push(report);
        on_each(reports.len());
    }
    reports
}

/// N건의 댓글 작업을 순차 실행하고 각 건의 결과를 [`CommentJobReport`]로 보고한다.
///
/// [`run_post_jobs`]의 댓글 버전이다. 각 작업마다 계정 쿠키를 읽어
/// ([`auth::read_account_cookies`]) 댓글 등록에 사용하며, **한 건이 실패해도
/// 중단하지 않고 다음 작업으로 넘어간다**. 쿠키 값은 어떤 보고/로그에도 노출되지 않는다.
/// 같은 계정이 한 게시글에 댓글을 연속으로 달 때, 네이버의 연속요청/도배 차단으로
/// 두 번째 이후가 거부되는 것을 피하려고 작업 사이에 두는 기본 간격.
const COMMENT_JOB_DELAY: Duration = Duration::from_millis(4000);

/// 댓글 러너가 작업 1건의 진행을 호출부에 알리는 이벤트(게시 큐의 라이브 단계 표시용).
pub enum CommentEvent<'a> {
    /// `index`번째 작업을 막 시작했다(대기 → 게시 중 표시 전환용).
    Started(usize),
    /// `index`번째 작업이 끝났다(결과 포함, 게시 중 → 완료/실패 표시 전환용).
    Finished(usize, &'a CommentJobReport),
}

pub async fn run_comment_jobs(jobs: &[CommentJob]) -> Vec<CommentJobReport> {
    run_comment_jobs_with_events(jobs, |_| {}).await
}

/// [`run_comment_jobs`]와 같지만 작업이 시작/완료될 때마다 [`CommentEvent`]를 호출부에
/// 흘려, 카페 1곳=1행 라이브 단계 표시를 가능케 한다. 작업 간 안티스팸 간격은 그대로 지킨다.
pub async fn run_comment_jobs_with_events<F: FnMut(CommentEvent)>(
    jobs: &[CommentJob],
    on_event: F,
) -> Vec<CommentJobReport> {
    run_comment_jobs_with_delay(jobs, COMMENT_JOB_DELAY, on_event).await
}

/// [`run_comment_jobs`]의 본체. 작업 간 간격을 인자로 받아 테스트에서 0으로 둘 수 있다.
///
/// 각 작업의 시도/성공/실패를 `tracing`으로 남겨, 두 번째 이후 댓글이 누락될 때
/// 네이버가 돌려준 오류 코드/사유를 로그에서 확인할 수 있게 한다. 쿠키 값은 절대
/// 로그에 포함하지 않는다(본문 `content`도 남기지 않는다).
async fn run_comment_jobs_with_delay<F: FnMut(CommentEvent)>(
    jobs: &[CommentJob],
    delay: Duration,
    mut on_event: F,
) -> Vec<CommentJobReport> {
    let client = CafeCommentClient::new();
    let mut cookies = CookieHeaderCache::default();
    let total = jobs.len();
    let mut reports = Vec::with_capacity(total);
    for (index, job) in jobs.iter().enumerate() {
        // 이 댓글을 곧 게시한다는 표시(게시 중…)를 먼저 띄우고, 그 상태로 연속요청 차단
        // 회피용 간격을 둔다. 간격을 Started 앞에 두면 그 시간 동안 행이 "게시 전"으로 남아
        // "게시 중"이 폴링에 거의 안 잡히므로(실제 게시 호출은 순식간), 순서를 뒤집는다.
        on_event(CommentEvent::Started(index));
        if index > 0 && !delay.is_zero() {
            sleep(delay).await;
        }
        tracing::info!(
            seq = index + 1,
            total,
            cafe_id = job.cafe_id,
            article_id = job.article_id,
            "댓글 등록 시도"
        );
        let report = run_single_comment_job(&client, job, &mut cookies).await;
        match &report.error {
            None => tracing::info!(
                seq = index + 1,
                total,
                article_id = job.article_id,
                "댓글 등록 성공"
            ),
            Some(err) => tracing::warn!(
                seq = index + 1,
                total,
                article_id = job.article_id,
                error_code = %err.code,
                error_message = %err.message,
                "댓글 등록 실패"
            ),
        }
        on_event(CommentEvent::Finished(index, &report));
        reports.push(report);
    }
    reports
}

async fn run_single_comment_job(
    client: &CafeCommentClient,
    job: &CommentJob,
    cookies: &mut CookieHeaderCache,
) -> CommentJobReport {
    // 계정 쿠키 해석(배치 내 1회 캐시). 없거나 오류면 건너뛴다.
    // 보안: cookie_header 값은 로그/보고에 노출하지 않는다.
    let cookie_header = match cookies.resolve(&job.account_id) {
        Ok(header) => header,
        Err(CookieFailure::Missing) => {
            return CommentJobReport::failure(job, no_cookies_comment_error(&job.account_id, None))
        }
        Err(CookieFailure::Read(msg)) => {
            return CommentJobReport::failure(
                job,
                no_cookies_comment_error(&job.account_id, Some(msg)),
            )
        }
    };

    let request = CommentRequest {
        cafe_id: job.cafe_id.to_string(),
        article_id: job.article_id.to_string(),
        content: job.content.clone(),
        sticker_id: None,
    };

    match client
        .post_comment(&request, Some(cookie_header.as_str()))
        .await
    {
        Ok(result) => CommentJobReport::success(job, result),
        Err(err) => CommentJobReport::failure(job, err),
    }
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{method, path, path_regex},
        Mock, MockServer, ResponseTemplate,
    };

    fn sample_job(cafe: &str) -> PostJob {
        PostJob {
            account_id: "tester".to_string(),
            cafe: cafe.to_string(),
            menu_id: 1,
            board_type: "L".to_string(),
            subject: "제목".to_string(),
            body_text: "본문".to_string(),
            tag_list: vec![],
        }
    }

    // ------------------------------------------------------------------
    // resolve_cafe_id
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn resolve_cafe_id_returns_numeric_directly_without_network() {
        // 숫자 입력은 네트워크 없이 즉시 해석된다(서버는 시작만 하고 mock 없음).
        let server = MockServer::start().await;
        let orch = CafeOrchestrator::with_base_url(server.uri());

        let id = orch
            .resolve_cafe_id("https://cafe.naver.com/ca-fe/cafes/31732304/articles", None)
            .await
            .expect("숫자 cafeId 해석 성공해야 함");
        assert_eq!(id, 31732304);
    }

    #[tokio::test]
    async fn list_joined_cafes_delegates_to_joined_client() {
        const FIXTURE: &str = include_str!("joined_cafes/fixtures/join_cafes_groups_success.json");
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/cafe-home-web/cafe-home/v1/config/join-cafes/groups/",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_string(FIXTURE))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let cafes = orch
            .list_joined_cafes(None)
            .await
            .expect("가입 카페 조회 성공해야 함");
        assert_eq!(cafes.len(), 3);
        assert_eq!(cafes[0].cafe_id, 31732304);
    }

    #[tokio::test]
    async fn resolve_cafe_id_resolves_vanity_via_home_html() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/bluegrayoc3uc"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"<script>var g_sClubId = "31732304";</script>"#),
            )
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let id = orch
            .resolve_cafe_id("cafe.naver.com/bluegrayoc3uc", None)
            .await
            .expect("vanity 해석 성공해야 함");
        assert_eq!(id, 31732304);
    }

    #[tokio::test]
    async fn resolve_cafe_id_invalid_input_returns_error() {
        let server = MockServer::start().await;
        let orch = CafeOrchestrator::with_base_url(server.uri());

        let err = orch
            .resolve_cafe_id("not-a-url-or-number!!", None)
            .await
            .expect_err("인식 불가 입력은 Err여야 함");
        assert_eq!(err.code, CODE_INVALID_CAFE_INPUT);
    }

    // ------------------------------------------------------------------
    // list_boards
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn list_boards_returns_general_writable_boards() {
        let server = MockServer::start().await;

        let response = json!({
            "result": [
                {
                    "cafeId": 31732304_u64, "menuId": 1_u64, "menuName": "자유게시판",
                    "menuType": "B", "boardType": "L", "writable": true,
                    "hidden": false, "separatorMenuType": false
                },
                {
                    "cafeId": 31732304_u64, "menuId": 2_u64, "menuName": "중고마켓",
                    "menuType": "M", "boardType": "L", "writable": true,
                    "hidden": false, "separatorMenuType": false
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe-cafeinfo-api/.*/editor/menus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let boards = orch.list_boards(31732304, None).await.expect("성공해야 함");

        assert_eq!(boards.len(), 1, "일반 게시판(menuType=B)만 반환되어야 함");
        assert_eq!(boards[0].menu_id, 1);
    }

    // ------------------------------------------------------------------
    // post_one — 숫자 카페 + 글 작성 성공
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_one_numeric_cafe_posts_successfully() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/editor/v2.0/cafes/31732304/menus/1/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 9_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let result = orch
            .post_one(&sample_job("31732304"), Some("NID_AUT=FAKE; NID_SES=FAKE"))
            .await
            .expect("글 작성 성공해야 함");

        assert_eq!(result.cafe_id, 31732304);
        assert_eq!(result.article_id, 9);
        assert_eq!(result.menu_id, 1);
    }

    // ------------------------------------------------------------------
    // post_one — 빈 boardType은 게시 직전 게시판 목록 조회로 해석
    // ------------------------------------------------------------------

    fn sample_job_no_board_type(cafe: &str, menu_id: u64) -> PostJob {
        PostJob {
            menu_id,
            board_type: String::new(),
            ..sample_job(cafe)
        }
    }

    #[tokio::test]
    async fn post_one_empty_board_type_resolves_from_menu_list() {
        // boardType이 비면 게시판 목록에서 menuId와 일치하는 메뉴의 boardType을 채워 게시한다.
        let server = MockServer::start().await;

        // 게시판 목록: menuId=2의 boardType은 "P"다(기본값 "L" 강제가 아님을 확인하려 다른 값).
        let menus = json!({
            "result": [
                {
                    "cafeId": 31732304_u64, "menuId": 2_u64, "menuName": "공지게시판",
                    "menuType": "B", "boardType": "P", "writable": true,
                    "hidden": false, "separatorMenuType": false
                }
            ]
        });
        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe-cafeinfo-api/.*/editor/menus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(menus))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/editor/v2.0/cafes/31732304/menus/2/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 11_u64, "menuId": 2_u64 }
            })))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let result = orch
            .post_one(
                &sample_job_no_board_type("31732304", 2),
                Some("NID_AUT=FAKE; NID_SES=FAKE"),
            )
            .await
            .expect("빈 boardType은 목록 조회로 해석돼 게시 성공해야 함");
        assert_eq!(result.article_id, 11);
    }

    #[tokio::test]
    async fn post_one_empty_board_type_errors_when_menu_absent() {
        // 목록에 menuId 일치 메뉴가 없으면 기본값을 강제하지 않고 명시적으로 실패한다.
        let server = MockServer::start().await;
        let menus = json!({
            "result": [
                {
                    "cafeId": 31732304_u64, "menuId": 1_u64, "menuName": "자유게시판",
                    "menuType": "B", "boardType": "L", "writable": true,
                    "hidden": false, "separatorMenuType": false
                }
            ]
        });
        Mock::given(method("GET"))
            .and(path_regex(r"/cafe-web/cafe-cafeinfo-api/.*/editor/menus"))
            .respond_with(ResponseTemplate::new(200).set_body_json(menus))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let err = orch
            .post_one(
                &sample_job_no_board_type("31732304", 999),
                Some("NID_AUT=FAKE"),
            )
            .await
            .expect_err("일치 메뉴 없으면 Err여야 함");
        assert_eq!(err.code, CODE_INVALID_CAFE_INPUT);
    }

    // ------------------------------------------------------------------
    // post_one — vanity 카페 → 홈 해석 후 글 작성
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_one_vanity_cafe_resolves_then_posts() {
        let server = MockServer::start().await;

        // 1) vanity 홈 HTML
        Mock::given(method("GET"))
            .and(path("/bluegrayoc3uc"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"var g_sClubId = "31732304";"#),
            )
            .mount(&server)
            .await;

        // 2) 글 작성 (해석된 cafeId 경로)
        Mock::given(method("POST"))
            .and(path("/editor/v2.0/cafes/31732304/menus/1/articles"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": { "cafeId": 31732304_u64, "articleId": 3_u64, "menuId": 1_u64 }
            })))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let result = orch
            .post_one(
                &sample_job("cafe.naver.com/bluegrayoc3uc"),
                Some("NID_AUT=FAKE; NID_SES=FAKE"),
            )
            .await
            .expect("vanity 해석 후 글 작성 성공해야 함");
        assert_eq!(result.article_id, 3);
    }

    // ------------------------------------------------------------------
    // post_one — 실패가 PostError로 전달됨
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn post_one_propagates_post_error_on_http_failure() {
        let server = MockServer::start().await;

        let real_body = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"abc123"}}}"#;
        Mock::given(method("POST"))
            .and(path("/editor/v2.0/cafes/31732304/menus/1/articles"))
            .respond_with(ResponseTemplate::new(500).set_body_string(real_body))
            .mount(&server)
            .await;

        let orch = CafeOrchestrator::with_base_url(server.uri());
        let err = orch
            .post_one(&sample_job("31732304"), Some("NID_AUT=FAKE; NID_SES=FAKE"))
            .await
            .expect_err("HTTP 실패는 Err여야 함");

        assert_eq!(err.code, "REGISTER_HTTP_ERROR");
        assert_eq!(err.trace_id, "abc123");
    }

    #[tokio::test]
    async fn post_one_invalid_cafe_input_returns_post_error() {
        let server = MockServer::start().await;
        let orch = CafeOrchestrator::with_base_url(server.uri());

        let err = orch
            .post_one(&sample_job("???"), Some("NID_AUT=FAKE"))
            .await
            .expect_err("인식 불가 카페는 Err여야 함");
        assert_eq!(err.code, CODE_INVALID_CAFE_INPUT);
    }

    // ------------------------------------------------------------------
    // run_post_jobs — 쿠키 없는 계정은 건너뛰고 보고 (네트워크 없음)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn run_post_jobs_reports_failure_for_each_job_without_cookies() {
        // 존재하지 않는 계정 → 쿠키 없음 → 실패 보고. 글 작성 단계까지 가지 않으므로
        // 네트워크 요청이 발생하지 않는다.
        let jobs = vec![
            sample_job_for_account("no-such-account-1", "31732304"),
            sample_job_for_account("no-such-account-2", "31732304"),
        ];

        let reports = run_post_jobs(&jobs).await;

        assert_eq!(reports.len(), 2, "작업 수만큼 보고가 나와야 함");
        for report in &reports {
            assert!(!report.success, "쿠키 없는 계정은 실패여야 함");
            assert_eq!(
                report.error.as_ref().map(|e| e.code.as_str()),
                Some(CODE_NO_COOKIES),
                "쿠키 없음 코드여야 함"
            );
        }
    }

    fn sample_job_for_account(account_id: &str, cafe: &str) -> PostJob {
        PostJob {
            account_id: account_id.to_string(),
            ..sample_job(cafe)
        }
    }

    // ------------------------------------------------------------------
    // JobReport 직렬화 라운드트립
    // ------------------------------------------------------------------

    #[test]
    fn job_report_round_trips() {
        let job = sample_job("31732304");
        let report = JobReport::success(
            &job,
            ArticleRegisterResult {
                cafe_id: 31732304,
                article_id: 5,
                menu_id: 1,
            },
        );
        let serialized = serde_json::to_string(&report).expect("직렬화 실패");
        let restored: JobReport = serde_json::from_str(&serialized).expect("역직렬화 실패");
        assert_eq!(report, restored);
    }

    #[test]
    fn post_job_deserializes_camel_case() {
        let raw = json!({
            "accountId": "tester",
            "cafe": "cafe.naver.com/bluegrayoc3uc",
            "menuId": 1,
            "boardType": "L",
            "subject": "제목",
            "bodyText": "본문",
            "tagList": ["태그1"]
        });
        let job: PostJob = serde_json::from_value(raw).expect("역직렬화 실패");
        assert_eq!(job.account_id, "tester");
        assert_eq!(job.board_type, "L");
        assert_eq!(job.tag_list, vec!["태그1"]);
    }

    // ------------------------------------------------------------------
    // run_comment_jobs — 쿠키 없는 계정은 건너뛰고 보고 (네트워크 없음)
    // ------------------------------------------------------------------

    fn sample_comment_job(account_id: &str) -> CommentJob {
        CommentJob {
            account_id: account_id.to_string(),
            cafe_id: 31732304,
            article_id: 9,
            content: "안녕하세요".to_string(),
        }
    }

    #[tokio::test]
    async fn run_comment_jobs_reports_failure_for_each_job_without_cookies() {
        // 존재하지 않는 계정 → 쿠키 없음 → 실패 보고. 댓글 등록 단계까지 가지 않으므로
        // 네트워크 요청이 발생하지 않는다.
        let jobs = vec![
            sample_comment_job("no-such-account-1"),
            sample_comment_job("no-such-account-2"),
        ];

        // 지연 0으로 둬 테스트가 작업 간 간격을 기다리지 않게 한다(여러 건이어도 즉시 보고).
        let reports = run_comment_jobs_with_delay(&jobs, Duration::ZERO, |_| {}).await;

        assert_eq!(reports.len(), 2, "작업 수만큼 보고가 나와야 함");
        for report in &reports {
            assert!(!report.success, "쿠키 없는 계정은 실패여야 함");
            assert_eq!(
                report.error.as_ref().map(|e| e.code.as_str()),
                Some(CODE_NO_COOKIES),
                "쿠키 없음 코드여야 함"
            );
            assert!(report.result.is_none(), "실패 보고에는 result가 없어야 함");
        }
    }

    #[test]
    fn comment_job_deserializes_camel_case() {
        let raw = json!({
            "accountId": "tester",
            "cafeId": 31732304_u64,
            "articleId": 9_u64,
            "content": "댓글 본문"
        });
        let job: CommentJob = serde_json::from_value(raw).expect("역직렬화 실패");
        assert_eq!(job.account_id, "tester");
        assert_eq!(job.cafe_id, 31732304);
        assert_eq!(job.article_id, 9);
        assert_eq!(job.content, "댓글 본문");
    }

    #[test]
    fn comment_job_report_round_trips() {
        let job = sample_comment_job("tester");
        let report = CommentJobReport::success(
            &job,
            CommentResult {
                comment_id: 62628988,
                ref_comment_id: 62628988,
            },
        );
        let serialized = serde_json::to_string(&report).expect("직렬화 실패");
        let restored: CommentJobReport = serde_json::from_str(&serialized).expect("역직렬화 실패");
        assert_eq!(report, restored);
    }
}
