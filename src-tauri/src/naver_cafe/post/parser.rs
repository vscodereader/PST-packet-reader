use serde::{Deserialize, Serialize};

use crate::naver_cafe::{
    error::{ErrorEnvelope, NaverCafeCommonErrorData},
    response::{NaverApiEnvelope, ResultEnvelope},
};

use super::error::{PostError, PostErrorData};

// ---------------------------------------------------------------------------
// 응답 모델
// ---------------------------------------------------------------------------

/// ⚠️ 미확인 스키마: 패킷 캡처에 write-info 응답 바디가 없어 구조를 추정함.
/// 실제 응답으로 반드시 검증 후 확정할 것.
///
/// 게시글 작성 폼(Form) 제약 정보 — 글쓰기 화면 응답 내 `articleWriteForm`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArticleWriteForm {
    /// 제목 필수 여부.
    pub subject_required: bool,
    /// 제목 최대 글자 수.
    pub subject_max_length: u32,
    /// 본문 최대 글자 수.
    pub content_max_length: u32,
    /// 태그 입력 활성화 여부.
    pub tag_input_enabled: bool,
    /// 최대 태그 개수.
    pub tag_max_count: u32,
}

/// ⚠️ 미확인 스키마: 패킷 캡처에 write-info 응답 바디가 없어 구조를 추정함.
/// 실제 응답으로 반드시 검증 후 확정할 것.
///
/// 말머리(머리글) 항목 — `headList` 배열의 각 원소.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    /// 말머리 ID.
    pub head_id: u64,
    /// 말머리 표시 이름.
    pub head_name: String,
    /// 사용 여부.
    pub use_yn: bool,
}

/// ⚠️ 미확인 스키마: 패킷 캡처에 write-info 응답 바디가 없어 구조를 추정함.
/// 실제 응답으로 반드시 검증 후 확정할 것.
///
/// 글쓰기 화면(Write-Info) 응답의 `result` 본문.
///
/// `GET .../cafes/{cafeId}/menus/{menuId}/articles/write-info` 성공 시 반환.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriteInfo {
    /// 카페 ID (숫자).
    pub cafe_id: u64,
    /// 게시판(메뉴) ID (숫자).
    pub menu_id: u64,
    /// 게시판 이름.
    pub menu_name: String,
    /// 게시판 유형 ("B" = 일반게시판 등).
    pub menu_type: String,
    /// 글쓰기 권한 여부.
    pub write_permission: bool,
    /// 글쓰기 폼 제약 정보.
    pub article_write_form: ArticleWriteForm,
    /// 말머리(머리글) 목록.
    pub head_list: Vec<Head>,
}

/// 게시글 등록 성공 응답의 `result` 본문 — 패킷 캡처로 확인된 실제 형태.
///
/// 확인된 응답:
/// ```json
/// {"result":{"cafeId":31732304,"articleId":4,"menuId":1}}
/// ```
/// `POST .../cafes/{cafeId}/menus/{menuId}/articles` 성공 시 반환.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArticleRegisterResult {
    /// 카페 ID (숫자).
    pub cafe_id: u64,
    /// 등록된 게시글 ID (숫자).
    pub article_id: u64,
    /// 게시판(메뉴) ID (숫자).
    pub menu_id: u64,
}

/// 글쓰기 화면(write-info) 실패 분기에서 사용하는 `result` 본문(추정).
///
/// ⚠️ 미확인 스키마: write-info 응답 자체가 미확인이라 이 형태도 추정이다.
/// 실측 확인된 카페 API 공통 실패 봉투는 `{"error":{...}}`(`NaverApiErrorBody`)이며
/// client.rs에서 처리한다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiFailure {
    /// 네이버 API 오류 코드 (예: "NO_PERMISSION").
    pub error_code: String,
    /// 사람이 읽을 수 있는 오류 메시지 (한국어).
    pub error_message: String,
}

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// HTTP 상태 문자열을 `u16`으로 변환한다.
/// 변환에 실패하면 `None`을 반환한다.
fn parse_status_code(status: &str) -> Option<u16> {
    status.parse().ok()
}

/// `status` 문자열 기반으로 재시도 가능 여부를 결정한다.
///
/// - 5xx → `true` (서버 일시 오류, 재시도 권장)
/// - 4xx 및 나머지 → `false` (클라이언트 오류, 재시도 무의미)
fn is_retryable(status: &str) -> bool {
    parse_status_code(status).is_some_and(|code| code >= 500)
}

/// `ApiFailure`(write-info 실패 분기, 추정)와 상태 코드를 `PostError`로 변환한다.
///
/// ⚠️ write-info 응답이 미확인이라 이 변환 경로도 추정이다. 실측 확인된 공통
/// 실패 봉투(`{"error":{...}}`)는 client.rs에서 `NaverApiErrorBody`로 처리한다.
///
/// # trace_id
/// 이 경로의 입력에는 요청 ID가 없어 빈 문자열("")을 사용한다.
fn failure_to_post_error(failure: ApiFailure, status: &str) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: failure.error_code.clone(),
        message: failure.error_message.clone(),
        error_data: Some(PostErrorData {
            cafe: NaverCafeCommonErrorData {
                target: None,
                http_status: parse_status_code(status),
                api_error_code: Some(failure.error_code),
                api_error_message: Some(failure.error_message),
                retryable: is_retryable(status),
            },
            menu_id: None,
            subject: None,
            validation_errors: vec![],
        }),
    }
}

/// JSON 역직렬화 실패를 `PostError`로 감싼다.
fn json_error_to_post_error(err: serde_json::Error) -> PostError {
    ErrorEnvelope {
        trace_id: String::new(),
        code: "PARSE_ERROR".to_string(),
        message: err.to_string(),
        error_data: None,
    }
}

// ---------------------------------------------------------------------------
// 공개 파싱 함수
// ---------------------------------------------------------------------------

/// ⚠️ 미확인 스키마: 패킷 캡처에 write-info 응답 바디가 없어 봉투 구조 및
/// `WriteInfo` 필드 모두 추정이다. 실제 응답으로 반드시 검증 후 확정할 것.
///
/// 글쓰기 화면(Write-Info) API 응답 JSON을 파싱한다.
///
/// 성공(`status == "200"`)이면 [`WriteInfo`]를 반환하고,
/// 실패(그 외 상태 코드)이면 [`PostError`]를 반환한다.
/// 역직렬화에 실패해도 패닉 없이 `Err(PostError)`를 반환한다.
pub fn parse_write_info(raw: &str) -> Result<WriteInfo, PostError> {
    let envelope: NaverApiEnvelope<serde_json::Value> =
        serde_json::from_str(raw).map_err(json_error_to_post_error)?;

    let status = envelope.message.status.as_str();
    if status != "200" {
        let failure_envelope: NaverApiEnvelope<ApiFailure> =
            serde_json::from_str(raw).map_err(json_error_to_post_error)?;
        return Err(failure_to_post_error(
            failure_envelope.message.result,
            status,
        ));
    }

    let result: WriteInfo =
        serde_json::from_value(envelope.message.result).map_err(json_error_to_post_error)?;
    Ok(result)
}

/// 게시글 등록 API 응답 JSON을 파싱한다.
///
/// 확인된 성공 응답 형태: `{"result":{"cafeId":...,"articleId":...,"menuId":...}}`
///
/// 성공이면 [`ArticleRegisterResult`]를 반환한다.
///
/// 성공 응답(`ResultEnvelope`)만 파싱한다. 실패 응답은 실측 확인된
/// `{"error":{...}}` 봉투(`NaverApiErrorBody`)이며 client.rs에서 처리한다.
/// 역직렬화에 실패해도 패닉 없이 `Err(PostError)`를 반환한다.
pub fn parse_article_register(raw: &str) -> Result<ArticleRegisterResult, PostError> {
    let envelope: ResultEnvelope<ArticleRegisterResult> =
        serde_json::from_str(raw).map_err(json_error_to_post_error)?;
    Ok(envelope.result)
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // write_info_success.assumed.json — 미확인 스키마 기준 픽스처
    const WRITE_INFO_SUCCESS: &str = include_str!("fixtures/write_info_success.assumed.json");
    // article_register_success.json — 패킷 캡처로 확인된 실제 형태
    const ARTICLE_REGISTER_SUCCESS: &str = include_str!("fixtures/article_register_success.json");
    // article_register_failure.json — 실측 캡처된 실제 실패 형태 (HTTP 500)
    const ARTICLE_REGISTER_FAILURE: &str = include_str!("fixtures/article_register_failure.json");

    // ------------------------------------------------------------------
    // parse_write_info — 성공 케이스 (미확인 스키마 기준 테스트)
    // ------------------------------------------------------------------

    #[test]
    fn write_info_success_parses_cafe_id() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.cafe_id, 12345678);
    }

    #[test]
    fn write_info_success_parses_menu_id() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.menu_id, 10);
    }

    #[test]
    fn write_info_success_parses_menu_name() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.menu_name, "자유게시판");
    }

    #[test]
    fn write_info_success_parses_menu_type() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.menu_type, "B");
    }

    #[test]
    fn write_info_success_write_permission_is_true() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert!(info.write_permission);
    }

    #[test]
    fn write_info_success_subject_max_length() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.article_write_form.subject_max_length, 100);
    }

    #[test]
    fn write_info_success_content_max_length() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.article_write_form.content_max_length, 50000);
    }

    #[test]
    fn write_info_success_tag_max_count() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.article_write_form.tag_max_count, 10);
    }

    #[test]
    fn write_info_success_head_list_length() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        assert_eq!(info.head_list.len(), 2);
    }

    #[test]
    fn write_info_success_first_head_use_yn() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        let first = info.head_list.first().expect("headList가 비어 있음");
        assert_eq!(first.head_id, 1);
        assert_eq!(first.head_name, "공지");
        assert!(first.use_yn);
    }

    #[test]
    fn write_info_success_second_head() {
        let info = parse_write_info(WRITE_INFO_SUCCESS).expect("파싱 실패");
        let second = &info.head_list[1];
        assert_eq!(second.head_id, 2);
        assert_eq!(second.head_name, "질문");
        assert!(second.use_yn);
    }

    // ------------------------------------------------------------------
    // parse_article_register — 성공 케이스 (패킷 캡처 확인 데이터 기준)
    // ------------------------------------------------------------------

    #[test]
    fn article_register_success_parses_cafe_id() {
        let result = parse_article_register(ARTICLE_REGISTER_SUCCESS).expect("파싱 실패");
        assert_eq!(result.cafe_id, 31732304);
    }

    #[test]
    fn article_register_success_parses_article_id() {
        let result = parse_article_register(ARTICLE_REGISTER_SUCCESS).expect("파싱 실패");
        assert_eq!(result.article_id, 4);
    }

    #[test]
    fn article_register_success_parses_menu_id() {
        let result = parse_article_register(ARTICLE_REGISTER_SUCCESS).expect("파싱 실패");
        assert_eq!(result.menu_id, 1);
    }

    // ------------------------------------------------------------------
    // parse_article_register — 실패 케이스 (실측 캡처 기준)
    //
    // 실측 캡처된 실패 응답은 {"error":{"errorCode","message","more":{"requestId"}}}
    // 형태이며, ResultEnvelope의 `result` 키가 없으므로 parse_article_register는
    // PARSE_ERROR(Err)를 반환한다. 실패 응답 처리(스키마 파싱)는 client.rs에서 수행.
    // ------------------------------------------------------------------

    #[test]
    fn article_register_failure_returns_err() {
        // 실측 캡처된 실패 봉투는 ResultEnvelope와 맞지 않으므로 PARSE_ERROR 반환
        let result = parse_article_register(ARTICLE_REGISTER_FAILURE);
        assert!(result.is_err(), "실패 봉투 형태는 Err를 반환해야 함");
    }

    #[test]
    fn article_register_failure_parse_error_code() {
        // 실측 캡처된 실패 봉투는 ResultEnvelope와 맞지 않으므로 PARSE_ERROR 발생
        let err = parse_article_register(ARTICLE_REGISTER_FAILURE).expect_err("Err를 기대함");
        assert_eq!(
            err.code, "PARSE_ERROR",
            "실패 봉투 형태(error 키) → PARSE_ERROR 예상"
        );
    }

    // ------------------------------------------------------------------
    // 엣지 케이스 — 잘못된 입력
    // ------------------------------------------------------------------

    #[test]
    fn parse_write_info_garbage_input_returns_err() {
        let result = parse_write_info("not valid json at all {{{{");
        assert!(result.is_err(), "잘못된 입력이 Ok를 반환해서는 안 됨");
    }

    #[test]
    fn parse_article_register_empty_string_returns_err() {
        let result = parse_article_register("");
        assert!(result.is_err(), "빈 문자열이 Ok를 반환해서는 안 됨");
    }

    #[test]
    fn parse_write_info_missing_fields_returns_err() {
        // status는 200이지만 result가 잘못된 형태 (미확인 스키마 기준 테스트)
        let raw = r#"{"message":{"status":"200","result":{}}}"#;
        let result = parse_write_info(raw);
        assert!(result.is_err(), "필드 누락 시 Err를 반환해야 함");
    }

    // ------------------------------------------------------------------
    // 엣지 케이스 — status 기반 분기 (미확인 스키마 기준 테스트)
    // ------------------------------------------------------------------

    /// status가 "200"이지만 result가 WriteInfo 형태가 아닐 때 → 파싱 실패로 Err.
    /// (write-info 응답 자체가 미확인 추정 스키마 기준.)
    #[test]
    fn parse_write_info_status_200_but_failure_shaped_result_returns_err() {
        let raw = r#"{
            "message": {
                "status": "200",
                "result": {
                    "errorCode": "UNEXPECTED",
                    "errorMessage": "예상치 못한 오류"
                }
            }
        }"#;
        // result가 WriteInfo 형태가 아니므로 파싱 실패 → Err
        let result = parse_write_info(raw);
        assert!(result.is_err(), "WriteInfo 형태가 아닌 result는 Err여야 함");
    }

    /// parse_article_register는 ResultEnvelope를 사용하므로 message/status
    /// 봉투 형태 입력은 `result` 키가 없어 PARSE_ERROR(Err)를 반환한다.
    #[test]
    fn parse_article_register_non_result_envelope_returns_err() {
        // message/status 봉투는 ResultEnvelope와 맞지 않으므로 PARSE_ERROR
        let raw = r#"{
            "message": {
                "status": "500",
                "result": {
                    "errorCode": "INTERNAL_ERROR",
                    "errorMessage": "서버 내부 오류"
                }
            }
        }"#;
        let result = parse_article_register(raw);
        assert!(
            result.is_err(),
            "ResultEnvelope가 아닌 봉투 형태는 Err여야 함"
        );
    }

    /// write_info(추정) 파싱 경로로 5xx retryable 규칙을 검증한다.
    /// 실측 확인된 공통 실패 봉투는 client.rs(NaverApiErrorBody)에서 처리·검증한다.
    #[test]
    fn parse_write_info_500_is_retryable() {
        let raw = r#"{
            "message": {
                "status": "500",
                "result": {
                    "errorCode": "SERVER_ERROR",
                    "errorMessage": "서버 오류입니다."
                }
            }
        }"#;
        let err = parse_write_info(raw).expect_err("Err를 기대함");
        let cafe = &err.error_data.expect("errorData가 없음").cafe;
        assert!(cafe.retryable, "500은 재시도 가능이어야 함");
        assert_eq!(cafe.http_status, Some(500));
    }
}
