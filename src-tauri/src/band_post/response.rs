//! band api 응답 파싱. 모든 응답은 `{"result_code":N,"result_data":{...}}` 형태다.
//!
//! 성공은 `result_code == 1`. 그 외는 실패로 보고, `result_data`에서 가능한 한
//! 사람이 읽을 메시지를 뽑는다(쿠키/키 값은 절대 포함하지 않는다).

use serde_json::Value;

/// band api 호출 실패.
#[derive(Debug, Clone, PartialEq)]
pub struct BandApiError {
    /// band `result_code`(성공 1 외의 값). HTTP 오류 시 `None`.
    pub result_code: Option<i64>,
    /// 사람이 읽을 수 있는 오류 설명.
    pub message: String,
}

impl std::fmt::Display for BandApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.result_code {
            Some(code) => write!(f, "band api 오류(result_code={code}): {}", self.message),
            None => write!(f, "band api 오류: {}", self.message),
        }
    }
}

impl std::error::Error for BandApiError {}

/// 응답 본문을 파싱해 `result_code==1`이면 `result_data`를 반환한다.
///
/// `result_code`가 1이 아니거나 JSON 파싱이 실패하면 [`BandApiError`]를 반환한다.
pub fn parse_band_result(body: &str) -> Result<Value, BandApiError> {
    let value: Value = serde_json::from_str(body).map_err(|_| BandApiError {
        result_code: None,
        message: format!("응답 JSON 파싱 실패: {}", truncate(body, 200)),
    })?;

    let code = value.get("result_code").and_then(Value::as_i64);
    if code == Some(1) {
        Ok(value.get("result_data").cloned().unwrap_or(Value::Null))
    } else {
        Err(BandApiError {
            result_code: code,
            message: extract_error_message(&value),
        })
    }
}

/// 성공 응답(`result_data`)에서 생성된 게시물 번호(`post.post_no`)를 추출한다.
pub fn post_no_from_result(result_data: &Value) -> Option<u64> {
    result_data
        .get("post")
        .and_then(|p| p.get("post_no"))
        .and_then(Value::as_u64)
}

/// 성공 응답에서 게시물 web_url을 추출한다.
pub fn web_url_from_result(result_data: &Value) -> Option<String> {
    result_data
        .get("post")
        .and_then(|p| p.get("web_url"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 성공 응답에서 실제 밴드 이름(`post.band.name`)을 추출한다.
///
/// 예: band_no 103043410 → `"데일밴드"`. 프론트가 게시 결과에 진짜 밴드명을 표시하는 데 쓴다.
pub fn band_name_from_result(result_data: &Value) -> Option<String> {
    result_data
        .get("post")
        .and_then(|p| p.get("band"))
        .and_then(|b| b.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// `get_band_information` 응답에서 밴드 이름(`result_data.name`)을 추출한다.
///
/// 게시 전에 링크(band_no)로 밴드명을 미리 확인하는 데 쓴다. 예: 103043410 → `"데일밴드"`.
pub fn name_from_band_info(result_data: &Value) -> Option<String> {
    result_data
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_error_message(value: &Value) -> String {
    // band 오류는 result_data.message 또는 message에 담기는 경우가 있다.
    value
        .get("result_data")
        .and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        .or_else(|| value.get("message").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| "알 수 없는 오류".to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_success_returns_result_data() {
        // 캡처: {"result_code":1,"result_data":{"message":"밴드에 가입했습니다."}}
        let data = parse_band_result(r#"{"result_code":1,"result_data":{"message":"밴드에 가입했습니다."}}"#)
            .expect("성공이어야 함");
        assert_eq!(data.get("message").unwrap(), "밴드에 가입했습니다.");
    }

    #[test]
    fn create_post_success_extracts_post_no_url_and_band_name() {
        // 캡처 형태 축약: result_data.post.{post_no, web_url, band.name}
        let body = r#"{"result_code":1,"result_data":{"post":{"post_no":2,"web_url":"https://band.us/band/103043410/post/2","band":{"band_no":103043410,"name":"데일밴드"}}}}"#;
        let data = parse_band_result(body).unwrap();
        assert_eq!(post_no_from_result(&data), Some(2));
        assert_eq!(
            web_url_from_result(&data).as_deref(),
            Some("https://band.us/band/103043410/post/2")
        );
        assert_eq!(band_name_from_result(&data).as_deref(), Some("데일밴드"));
    }

    #[test]
    fn band_name_none_when_absent() {
        let data = serde_json::json!({"post": {"post_no": 1}});
        assert!(band_name_from_result(&data).is_none());
    }

    #[test]
    fn name_from_band_info_extracts_name() {
        // 캡처: get_band_information → {"result_code":1,"result_data":{"name":"데일밴드",...}}
        let data = parse_band_result(
            r#"{"result_code":1,"result_data":{"band_no":103043410,"name":"데일밴드"}}"#,
        )
        .unwrap();
        assert_eq!(name_from_band_info(&data).as_deref(), Some("데일밴드"));
    }

    #[test]
    fn non_one_result_code_is_error() {
        let err = parse_band_result(r#"{"result_code":0,"result_data":{"message":"권한 없음"}}"#)
            .expect_err("실패여야 함");
        assert_eq!(err.result_code, Some(0));
        assert_eq!(err.message, "권한 없음");
    }

    #[test]
    fn invalid_json_is_error_without_result_code() {
        let err = parse_band_result("<html>error</html>").expect_err("실패여야 함");
        assert!(err.result_code.is_none());
        assert!(err.message.contains("파싱 실패"));
    }

    #[test]
    fn missing_message_falls_back() {
        let err = parse_band_result(r#"{"result_code":401}"#).expect_err("실패여야 함");
        assert_eq!(err.result_code, Some(401));
        assert_eq!(err.message, "알 수 없는 오류");
    }

    #[test]
    fn post_no_none_when_absent() {
        let data = serde_json::json!({"something": 1});
        assert!(post_no_from_result(&data).is_none());
    }
}
