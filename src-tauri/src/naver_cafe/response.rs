use serde::{Deserialize, Serialize};

/// 게시글 등록 성공 응답의 최상위 봉투 — 패킷 캡처로 확인된 실제 형태.
///
/// 캡처된 실제 응답:
/// ```json
/// {"result":{"cafeId":31732304,"articleId":4,"menuId":1}}
/// ```
/// `message`/`status` 래퍼 없이 최상위에 `result` 키만 있다.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResultEnvelope<T> {
    /// 응답 본문.
    pub result: T,
}

/// `message` 오브젝트 — 최상위 봉투는 [`NaverApiEnvelope`] 참조.
///
/// ⚠️ 미확인 스키마: 이 `message`/`status` 봉투가 실제로 쓰이는 응답은 캡처되지
/// 않았다. 확인된 최상위 봉투는 `{"result":{...}}`([`ResultEnvelope`]) 뿐이며,
/// write-info·실패 응답에 쓰일 것으로 추정한다. 실제 응답으로 검증 후 확정할 것.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiMessage<T> {
    /// HTTP 상태 코드를 나타내는 문자열 (예: "200", "403").
    pub status: String,
    /// 응답 본문 — 성공 또는 실패 형태 모두 가능.
    pub result: T,
}

/// `{"message":{"status":...,"result":...}}` 봉투. `T`에 구체적 result 타입을
/// 넣는다 (예: `NaverApiEnvelope<WriteInfo>`).
///
/// ⚠️ 미확인 스키마: [`NaverApiMessage`]와 동일하게, 이 봉투가 실제로 쓰이는
/// 응답은 캡처되지 않았다(추정). 실제 응답으로 검증 후 확정할 것.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiEnvelope<T> {
    /// API 응답 메시지 래퍼.
    pub message: NaverApiMessage<T>,
}

/// 네이버 카페 API 공통 실패 응답 봉투 — 실측 캡처 기준.
///
/// 실측 캡처된 실제 응답 (HTTP 500):
/// ```json
/// {"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiErrorBody {
    /// API 오류 상세 정보.
    pub error: NaverApiError,
}

/// 네이버 카페 API 오류 상세 — `error` 키 아래의 오브젝트.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiError {
    /// 네이버 API 오류 코드 (예: "10404").
    pub error_code: String,
    /// 사람이 읽을 수 있는 오류 메시지 (예: "Page Not Found").
    pub message: String,
    /// 추가 메타데이터 (선택).
    pub more: Option<NaverApiErrorMore>,
}

/// 네이버 카페 API 오류 `more` 필드 — 요청 추적 ID 등을 포함.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiErrorMore {
    /// 요청 추적 ID (예: "cf4ee2db355d4584b6e0add8f8743048").
    pub request_id: Option<String>,
}

impl NaverApiErrorBody {
    /// 원본 JSON 문자열을 [`NaverApiErrorBody`]로 파싱한다.
    ///
    /// 형태가 맞지 않으면 `None`을 반환한다 (패닉 없음).
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
}

/// 오류에 담는 원본 응답 본문의 최대 길이(바이트). 이를 넘으면 잘라낸다.
pub const RAW_BODY_MAX_LEN: usize = 2000;

/// 원본 응답 본문을 [`RAW_BODY_MAX_LEN`] 바이트로 잘라낸다(문자 경계에서 안전하게).
///
/// 알 수 없는 형태의 실패 응답을 오류 메시지에 담을 때, 본문이 과도하게 길거나
/// UTF-8 경계를 깨뜨리지 않도록 공통으로 사용한다. 네이버 카페 클라이언트 6개
/// 모듈이 동일 로직을 복제하던 것을 한곳으로 모은 것.
pub fn truncate_body(raw: String) -> String {
    if raw.len() <= RAW_BODY_MAX_LEN {
        raw
    } else {
        // 문자 경계에서 안전하게 잘라낸다.
        let cutoff = raw
            .char_indices()
            .take_while(|(i, _)| *i < RAW_BODY_MAX_LEN)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(RAW_BODY_MAX_LEN);
        format!("{} [truncated]", &raw[..cutoff])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ------------------------------------------------------------------
    // NaverApiErrorBody — 실측 캡처 기준 파싱 테스트
    // ------------------------------------------------------------------

    const REAL_FAILURE_JSON: &str = r#"{"error":{"errorCode":"10404","message":"Page Not Found","more":{"requestId":"cf4ee2db355d4584b6e0add8f8743048"}}}"#;

    #[test]
    fn naver_api_error_body_parses_real_failure_json() {
        let body = NaverApiErrorBody::parse(REAL_FAILURE_JSON).expect("파싱 실패");
        assert_eq!(body.error.error_code, "10404");
        assert_eq!(body.error.message, "Page Not Found");
        let more = body.error.more.expect("more 필드가 없음");
        assert_eq!(
            more.request_id.as_deref(),
            Some("cf4ee2db355d4584b6e0add8f8743048")
        );
    }

    #[test]
    fn naver_api_error_body_round_trips() {
        let body = NaverApiErrorBody::parse(REAL_FAILURE_JSON).expect("파싱 실패");
        let json = serde_json::to_string(&body).expect("직렬화 실패");
        let restored: NaverApiErrorBody = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(body, restored);
    }

    #[test]
    fn naver_api_error_body_parse_returns_none_for_unknown_shape() {
        let unknown = r#"{"some":"unknown error shape"}"#;
        let result = NaverApiErrorBody::parse(unknown);
        assert!(result.is_none(), "알 수 없는 형태는 None이어야 함");
    }

    #[test]
    fn naver_api_error_body_parse_returns_none_for_empty_string() {
        let result = NaverApiErrorBody::parse("");
        assert!(result.is_none(), "빈 문자열은 None이어야 함");
    }

    #[test]
    fn naver_api_error_body_more_is_optional() {
        let raw = r#"{"error":{"errorCode":"9999","message":"Some Error"}}"#;
        let body = NaverApiErrorBody::parse(raw).expect("파싱 실패");
        assert_eq!(body.error.error_code, "9999");
        assert!(body.error.more.is_none(), "more는 선택 필드여야 함");
    }

    // ------------------------------------------------------------------
    // NaverApiEnvelope / NaverApiMessage — 기존 테스트
    // ------------------------------------------------------------------

    #[test]
    fn envelope_deserializes_status_and_result() {
        let raw = json!({
            "message": {
                "status": "200",
                "result": { "value": 42 }
            }
        })
        .to_string();

        #[derive(Debug, Deserialize, PartialEq)]
        struct Inner {
            value: u32,
        }

        let envelope: NaverApiEnvelope<Inner> = serde_json::from_str(&raw).expect("역직렬화 실패");
        assert_eq!(envelope.message.status, "200");
        assert_eq!(envelope.message.result.value, 42);
    }

    #[test]
    fn envelope_round_trips() {
        let original = NaverApiEnvelope {
            message: NaverApiMessage {
                status: "200".to_string(),
                result: "payload".to_string(),
            },
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: NaverApiEnvelope<String> =
            serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    // ------------------------------------------------------------------
    // truncate_body — 공통 본문 절단 헬퍼
    // ------------------------------------------------------------------

    #[test]
    fn truncate_body_short_string_unchanged() {
        let s = "short body".to_string();
        assert_eq!(truncate_body(s.clone()), s);
    }

    #[test]
    fn truncate_body_long_string_is_truncated() {
        let s = "x".repeat(RAW_BODY_MAX_LEN + 100);
        let result = truncate_body(s);
        assert!(result.ends_with(" [truncated]"));
        assert!(result.len() < RAW_BODY_MAX_LEN + 100);
    }

    #[test]
    fn truncate_body_respects_utf8_char_boundary() {
        // 멀티바이트 문자가 경계에 걸쳐도 깨진 바이트로 자르지 않는다.
        let s = "가".repeat(RAW_BODY_MAX_LEN);
        let result = truncate_body(s);
        assert!(result.ends_with(" [truncated]"));
    }
}
