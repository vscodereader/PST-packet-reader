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

/// ⚠️ 미확인 스키마: 패킷 캡처에 `message`/`status` 봉투가 실제로 사용되는
/// 응답이 포착되지 않았다. 유일하게 확인된 최상위 봉투는 `{"result":{...}}`
/// 형태([`ResultEnvelope`])이다. 이 구조체는 write-info 및 실패 응답에
/// 쓰일 것으로 추정하지만, 실제 응답으로 반드시 검증 후 확정할 것.
///
/// `message` 오브젝트 자체를 나타낸다.
/// 최상위 봉투는 [`NaverApiEnvelope`]를 참조하라.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiMessage<T> {
    /// HTTP 상태 코드를 나타내는 문자열 (예: "200", "403").
    pub status: String,
    /// 응답 본문 — 성공 또는 실패 형태 모두 가능.
    pub result: T,
}

/// ⚠️ 미확인 스키마: 패킷 캡처에서 이 `{"message":{"status":...,"result":...}}`
/// 봉투 형태가 실제로 사용되는 응답은 포착되지 않았다. 확인된 봉투는
/// `{"result":{...}}`([`ResultEnvelope`]) 뿐이다. write-info 및 실패
/// 응답에 이 봉투가 쓰일 것으로 추정하나, 실제 응답으로 반드시 검증 후 확정할 것.
///
/// 성공/실패 공통으로 사용되며, `T`에 구체적인 result 타입을 넣는다.
/// 예: `NaverApiEnvelope<WriteInfo>`, `NaverApiEnvelope<ApiFailure>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NaverApiEnvelope<T> {
    /// API 응답 메시지 래퍼.
    pub message: NaverApiMessage<T>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

        let envelope: NaverApiEnvelope<Inner> =
            serde_json::from_str(&raw).expect("역직렬화 실패");
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
}
