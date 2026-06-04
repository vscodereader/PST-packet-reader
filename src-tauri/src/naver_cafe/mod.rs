// 카페 글/댓글 에러 타입(CafeError 계열)은 네이버 응답 본문을 통째로 담아 변형이
// 큰 편이라, Result 의 Err 변형이 크다는 clippy::result_large_err 가 여러 함수에서
// 발화한다. 에러를 Box 로 감싸는 대신(API 표면 변경) 모듈 전체에서 이 린트를 허용한다.
#![allow(clippy::result_large_err)]

pub mod cafe_ref;
pub mod comment;
pub mod error;
pub mod joined_cafes;
pub mod menu;
pub mod models;
pub mod orchestrator;
pub mod post;
pub mod response;

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
    run_comment_jobs, run_post_jobs, CafeOrchestrator, CommentJob, CommentJobReport, JobReport,
    PostJob, CODE_INVALID_CAFE_INPUT, CODE_NO_COOKIES,
};
pub use post::{PostError, PostErrorData, PostRequest};
pub use response::{
    NaverApiEnvelope, NaverApiError, NaverApiErrorBody, NaverApiErrorMore, NaverApiMessage,
    ResultEnvelope,
};
