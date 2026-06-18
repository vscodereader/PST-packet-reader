// 카페 글/댓글 에러 타입(CafeError 계열)은 네이버 응답 본문을 통째로 담아 변형이
// 큰 편이라, Result 의 Err 변형이 크다는 clippy::result_large_err 가 여러 함수에서
// 발화한다. 에러를 Box 로 감싸는 대신(API 표면 변경) 모듈 전체에서 이 린트를 허용한다.
#![allow(clippy::result_large_err)]

use std::sync::OnceLock;

pub mod article_list;
pub mod cafe_ref;
pub mod comment;
pub mod distribute;
pub mod error;
pub(crate) mod headers;
pub mod joined_cafes;
pub mod menu;
pub mod models;
pub mod orchestrator;
pub mod post;
pub mod response;

/// 프로세스 전역 공용 reqwest 클라이언트를 반환한다.
///
/// 하위 카페 클라이언트들이 매 IPC 호출마다 `reqwest::Client::new()`로 새 클라이언트를
/// 만들면, 그때마다 커넥션 풀이 비어 있어 TLS 핸드셰이크를 새로 한다. `reqwest::Client`는
/// 내부가 `Arc`라 `clone`이 저렴하고 재사용을 전제로 설계됐으므로, 전역 하나를 공유해
/// keep-alive 커넥션을 호출·배치 간에 재활용한다. base_url은 클라이언트마다 따로 보관하므로
/// 하나의 클라이언트로 모든 호스트(실서버·wiremock)에 안전하게 요청할 수 있다.
pub(crate) fn shared_http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

pub use article_list::{
    fetch_article_list_for_account, fetch_latest_articles_for_account_up_to, Article,
    ArticleListClient, ArticleListError, ArticleListResponse, SortBy, ARTICLE_LIST_API_HOST,
};
pub use cafe_ref::{
    cafe_gate_info_path, cafe_home_path, parse_cafe_id, parse_cafe_ref, parse_club_id_from_html,
    CafeGateClient, CafeGateInfoResponse, CafeHomeClient, CafeInfoView, CafeRef, CafeRefError,
    CAFE_API_HOST, CAFE_HOME_HOST,
};
pub use comment::{
    build_comment_preview, build_reply_preview, execute_comment, execute_reply,
    parse_comment_result, CafeCommentClient, CommentError, CommentErrorData, CommentExecutionMode,
    CommentOutcome, CommentPreview, CommentRequest, CommentResult, ReplyRequest,
};
pub use error::{ErrorEnvelope, NaverCafeCommonErrorData, ValidationError};
pub use joined_cafes::{
    join_cafes_path, JoinedCafe, JoinedCafesClient, JoinedCafesError, JOINED_CAFES_HOST,
};
pub use menu::{
    general_writable_boards, menu_list_path, CafeMenuClient, Menu, MenuError, MENU_API_HOST,
};
pub use models::CafeTarget;
pub use orchestrator::{
    run_comment_jobs, run_comment_jobs_with_events, run_post_jobs, run_post_jobs_with_progress,
    CafeOrchestrator, CommentEvent, CommentJob, CommentJobReport, JobReport, PostJob,
    CODE_INVALID_CAFE_INPUT, CODE_NO_COOKIES,
};
pub use post::{PostError, PostErrorData, PostRequest};
pub use response::{
    NaverApiEnvelope, NaverApiError, NaverApiErrorBody, NaverApiErrorMore, NaverApiMessage,
    ResultEnvelope,
};
