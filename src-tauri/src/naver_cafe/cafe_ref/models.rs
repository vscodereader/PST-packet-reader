//! CafeGateInfo API 응답 모델.
//!
//! 실측 캡처된 응답 형태:
//! ```json
//! {"message":{"result":{"cafeInfoView":{"cafeId":31732304,"cafeUrl":"bluegrayoc3uc","cafeName":"test64979381"}}}}
//! ```

use serde::{Deserialize, Serialize};

use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};

// ---------------------------------------------------------------------------
// 공개 타입 별칭
// ---------------------------------------------------------------------------

/// `cafe_ref` 모듈의 오류 타입 — 공통 봉투에 HTTP/API 오류 상세를 담는다.
pub type CafeRefError = ErrorEnvelope<NaverCafeCommonErrorData>;

// ---------------------------------------------------------------------------
// 응답 모델
// ---------------------------------------------------------------------------

/// 카페 기본 정보 뷰 — CafeGateInfo API 응답의 핵심 데이터.
///
/// 실측 캡처된 `cafeInfoView` 오브젝트:
/// ```json
/// {"cafeId":31732304,"cafeUrl":"bluegrayoc3uc","cafeName":"test64979381"}
/// ```
///
/// - `cafe_id`: 숫자 카페 ID. 모든 API 호출에서 재사용된다.
/// - `cafe_url`: vanity 이름 (예: `bluegrayoc3uc`). `cafe.naver.com/<cafe_url>` 형식으로 접근 가능.
/// - `cafe_name`: 카페 표시 이름.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CafeInfoView {
    /// 숫자 카페 ID.
    pub cafe_id: u64,
    /// 카페 vanity URL 이름 (예: `bluegrayoc3uc`).
    pub cafe_url: String,
    /// 카페 표시 이름 (예: `test64979381`).
    pub cafe_name: String,
}

/// CafeGateInfo 응답의 `result` 오브젝트.
///
/// 실측 캡처 기준:
/// ```json
/// {"cafeInfoView": {...}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CafeGateResult {
    /// 카페 기본 정보.
    pub cafe_info_view: CafeInfoView,
}

/// CafeGateInfo 응답의 `message` 오브젝트.
///
/// 실측 캡처 기준:
/// ```json
/// {"result": {"cafeInfoView": {...}}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CafeGateMessage {
    /// 응답 결과 데이터.
    pub result: CafeGateResult,
}

/// CafeGateInfo API 최상위 응답 봉투.
///
/// 실측 캡처된 전체 응답:
/// ```json
/// {"message":{"result":{"cafeInfoView":{"cafeId":31732304,"cafeUrl":"bluegrayoc3uc","cafeName":"test64979381"}}}}
/// ```
///
/// 엔드포인트: `GET https://apis.naver.com/cafe-web/cafe2/CafeGateInfo.json?cafeId={cafeId}`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CafeGateInfoResponse {
    /// 응답 메시지 래퍼.
    pub message: CafeGateMessage,
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 실측 캡처된 픽스처 JSON.
    const FIXTURE: &str = include_str!("fixtures/cafe_gate_info_success.json");

    // ------------------------------------------------------------------
    // 픽스처 역직렬화 — 실측 데이터 기준
    // ------------------------------------------------------------------

    #[test]
    fn deserializes_real_fixture_correctly() {
        let response: CafeGateInfoResponse =
            serde_json::from_str(FIXTURE).expect("픽스처 역직렬화 실패");

        let view = &response.message.result.cafe_info_view;
        assert_eq!(view.cafe_id, 31732304, "cafeId가 틀림");
        assert_eq!(view.cafe_url, "bluegrayoc3uc", "cafeUrl이 틀림");
        assert_eq!(view.cafe_name, "test64979381", "cafeName이 틀림");
    }

    // ------------------------------------------------------------------
    // CafeInfoView 직렬화 키 이름 확인
    // ------------------------------------------------------------------

    #[test]
    fn cafe_info_view_serializes_camel_case_keys() {
        let view = CafeInfoView {
            cafe_id: 31732304,
            cafe_url: "bluegrayoc3uc".to_string(),
            cafe_name: "test64979381".to_string(),
        };
        let json = serde_json::to_value(&view).expect("직렬화 실패");

        assert!(json.get("cafeId").is_some(), "cafeId 키가 없음");
        assert!(json.get("cafeUrl").is_some(), "cafeUrl 키가 없음");
        assert!(json.get("cafeName").is_some(), "cafeName 키가 없음");

        // snake_case 키가 없어야 함
        assert!(
            json.get("cafe_id").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
        assert!(
            json.get("cafe_url").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
        assert!(
            json.get("cafe_name").is_none(),
            "snake_case 키가 있으면 안 됨"
        );
    }

    // ------------------------------------------------------------------
    // CafeInfoView 라운드트립
    // ------------------------------------------------------------------

    #[test]
    fn cafe_info_view_round_trips() {
        let original = CafeInfoView {
            cafe_id: 31732304,
            cafe_url: "bluegrayoc3uc".to_string(),
            cafe_name: "test64979381".to_string(),
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CafeInfoView = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    // ------------------------------------------------------------------
    // CafeGateInfoResponse 라운드트립
    // ------------------------------------------------------------------

    #[test]
    fn cafe_gate_info_response_round_trips() {
        let original = CafeGateInfoResponse {
            message: CafeGateMessage {
                result: CafeGateResult {
                    cafe_info_view: CafeInfoView {
                        cafe_id: 99999,
                        cafe_url: "testclub".to_string(),
                        cafe_name: "테스트 카페".to_string(),
                    },
                },
            },
        };
        let json = serde_json::to_string(&original).expect("직렬화 실패");
        let restored: CafeGateInfoResponse = serde_json::from_str(&json).expect("역직렬화 실패");
        assert_eq!(original, restored);
    }

    // ------------------------------------------------------------------
    // CafeGateInfoResponse 직렬화 키 이름 확인
    // ------------------------------------------------------------------

    #[test]
    fn cafe_gate_info_response_serializes_camel_case_keys() {
        let response: CafeGateInfoResponse =
            serde_json::from_str(FIXTURE).expect("픽스처 역직렬화 실패");
        let json = serde_json::to_value(&response).expect("직렬화 실패");

        // 최상위에 message 키가 있어야 함
        let message = json.get("message").expect("message 키가 없음");

        // message 아래에 result 키가 있어야 함
        let result = message.get("result").expect("result 키가 없음");

        // result 아래에 cafeInfoView 키가 있어야 함
        let cafe_info_view = result.get("cafeInfoView").expect("cafeInfoView 키가 없음");

        assert_eq!(
            cafe_info_view.get("cafeId").and_then(|v| v.as_u64()),
            Some(31732304)
        );
        assert_eq!(
            cafe_info_view.get("cafeUrl").and_then(|v| v.as_str()),
            Some("bluegrayoc3uc")
        );
        assert_eq!(
            cafe_info_view.get("cafeName").and_then(|v| v.as_str()),
            Some("test64979381")
        );
    }
}
