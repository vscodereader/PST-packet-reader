use serde::{Deserialize, Serialize};

use crate::naver_cafe::error::ErrorEnvelope;

use super::error::CommentError;

// ---------------------------------------------------------------------------
// 응답 모델
// ---------------------------------------------------------------------------

/// 댓글/대댓글 등록 성공 응답 — 패킷 캡처로 확인된 실제 형태.
///
/// 글 작성과 달리 `{"result":{...}}` 봉투 없이 최상위에 두 ID만 있다.
/// ```json
/// {"commentId":62598128,"refCommentId":62598128}
/// ```
/// - 일반 댓글: `commentId == refCommentId` (자기 자신).
/// - 대댓글: `commentId`는 새 대댓글 ID, `refCommentId`는 부모 댓글 ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentResult {
    /// 새로 생성된 댓글(또는 대댓글) ID.
    pub comment_id: u64,
    /// 참조 댓글 ID — 일반 댓글이면 자기 자신, 대댓글이면 부모 댓글 ID.
    pub ref_comment_id: u64,
}

impl CommentResult {
    /// 대댓글(답글) 응답이면 `true`.
    ///
    /// 일반 원댓글은 `comment_id == ref_comment_id`이고, 대댓글은 서로 다르다.
    pub fn is_reply(&self) -> bool {
        self.comment_id != self.ref_comment_id
    }
}

// ---------------------------------------------------------------------------
// 실패 응답 모델 (실서버 E2E 실측)
// ---------------------------------------------------------------------------

/// 댓글/대댓글 등록 실패 응답 — **실서버 E2E로 실측한 형태**.
///
/// ⚠️ 글 작성(`post`)의 실패 봉투와 다르다:
/// - `{"error":{...}}` 래퍼가 **없고** 필드가 최상위에 있다.
/// - 메시지 키가 `message`가 아니라 **`reason`**.
/// - `more`에 `requestId` 대신 카페 정보가 담긴다.
///
/// 실측 응답 (HTTP 404, 없는 게시글):
/// ```json
/// {"errorCode":"4003","reason":"삭제되었거나 존재하지 않는 게시글입니다.",
///  "more":{"cafeUrl":"...","cafeName":"...","pcCafeName":"...","cafeId":31732304}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentApiFailure {
    /// 네이버 API 오류 코드 (예: "4003").
    pub error_code: String,
    /// 사람이 읽을 수 있는 오류 사유 (한국어). post의 `message`에 대응.
    pub reason: String,
    /// 추가 메타데이터 (선택).
    pub more: Option<CommentApiFailureMore>,
}

/// 댓글 실패 응답 `more` 필드 — 카페 정보.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommentApiFailureMore {
    /// 카페 vanity URL (예: "bluegrayoc3uc").
    pub cafe_url: Option<String>,
    /// 카페 이름.
    pub cafe_name: Option<String>,
    /// PC 카페 이름.
    pub pc_cafe_name: Option<String>,
    /// 카페 ID (숫자).
    pub cafe_id: Option<u64>,
}

impl CommentApiFailure {
    /// 원본 JSON 문자열을 [`CommentApiFailure`]로 파싱한다.
    ///
    /// 형태가 맞지 않으면 `None`을 반환한다 (패닉 없음). 성공 응답
    /// (`{"commentId":...}`)은 `errorCode`/`reason`이 없어 `None`이 된다.
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

// ---------------------------------------------------------------------------
// 오류 코드 / 내부 헬퍼
// ---------------------------------------------------------------------------

/// JSON 역직렬화 실패 오류 코드.
pub const CODE_PARSE_ERROR: &str = "PARSE_ERROR";

/// JSON 역직렬화 실패를 [`CommentError`]로 감싼다.
fn json_error_to_comment_error(err: serde_json::Error) -> CommentError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_PARSE_ERROR.to_string(),
        message: err.to_string(),
        error_data: None,
    }
}

// ---------------------------------------------------------------------------
// 공개 파싱 함수
// ---------------------------------------------------------------------------

/// 댓글/대댓글 등록 API 응답 JSON을 파싱한다.
///
/// 성공 응답(`{"commentId":...,"refCommentId":...}`)이면 [`CommentResult`]를 반환한다.
/// 실패 응답은 실측 확인된 `{"error":{...}}` 봉투(`NaverApiErrorBody`)이며,
/// 이 형태는 `commentId`가 없어 `Err(CommentError { code: "PARSE_ERROR" })`가 된다.
/// 역직렬화에 실패해도 패닉 없이 `Err`를 반환한다.
pub fn parse_comment_result(raw: &str) -> Result<CommentResult, CommentError> {
    serde_json::from_str(raw).map_err(json_error_to_comment_error)
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENT_POST_SUCCESS: &str = include_str!("fixtures/comment_post_success.json");
    const COMMENT_REPLY_SUCCESS: &str = include_str!("fixtures/comment_reply_success.json");
    const COMMENT_FAILURE: &str = include_str!("fixtures/comment_failure.json");

    #[test]
    fn parses_comment_post_success() {
        let result = parse_comment_result(COMMENT_POST_SUCCESS).expect("파싱 실패");
        assert_eq!(result.comment_id, 62598128);
        assert_eq!(result.ref_comment_id, 62598128);
    }

    #[test]
    fn comment_post_is_not_reply() {
        let result = parse_comment_result(COMMENT_POST_SUCCESS).expect("파싱 실패");
        assert!(!result.is_reply(), "원댓글은 두 ID가 같아 is_reply=false");
    }

    #[test]
    fn parses_comment_reply_success() {
        let result = parse_comment_result(COMMENT_REPLY_SUCCESS).expect("파싱 실패");
        assert_eq!(result.comment_id, 62598725);
        assert_eq!(result.ref_comment_id, 62598693);
    }

    #[test]
    fn comment_reply_is_reply() {
        let result = parse_comment_result(COMMENT_REPLY_SUCCESS).expect("파싱 실패");
        assert!(result.is_reply(), "대댓글은 두 ID가 달라 is_reply=true");
    }

    #[test]
    fn failure_body_returns_parse_error() {
        // 실패 응답에는 commentId가 없으므로 성공 파서는 PARSE_ERROR.
        let err = parse_comment_result(COMMENT_FAILURE).expect_err("Err를 기대함");
        assert_eq!(err.code, CODE_PARSE_ERROR);
    }

    #[test]
    fn comment_api_failure_parses_real_body() {
        let failure = CommentApiFailure::parse(COMMENT_FAILURE).expect("실패 응답 파싱 실패");
        assert_eq!(failure.error_code, "4003");
        assert_eq!(failure.reason, "삭제되었거나 존재하지 않는 게시글입니다.");
        let more = failure.more.expect("more가 없음");
        assert_eq!(more.cafe_url.as_deref(), Some("bluegrayoc3uc"));
        assert_eq!(more.cafe_id, Some(31732304));
    }

    #[test]
    fn comment_api_failure_parse_returns_none_for_success_body() {
        // 성공 응답은 errorCode/reason이 없어 실패 파서로는 None.
        assert!(CommentApiFailure::parse(COMMENT_POST_SUCCESS).is_none());
    }

    #[test]
    fn comment_api_failure_parse_returns_none_for_garbage() {
        assert!(CommentApiFailure::parse("not json {{{").is_none());
    }

    #[test]
    fn garbage_input_returns_err() {
        assert!(parse_comment_result("not json {{{").is_err());
    }

    #[test]
    fn empty_string_returns_err() {
        assert!(parse_comment_result("").is_err());
    }

    #[test]
    fn result_round_trips() {
        let original = CommentResult {
            comment_id: 62598725,
            ref_comment_id: 62598693,
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CommentResult = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }
}
