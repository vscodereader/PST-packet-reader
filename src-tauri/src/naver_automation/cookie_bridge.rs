use serde_json::{json, Value};

// 로그인 자동화(CDP)가 저장한 쿠키 하나를 Chrome DevTools Protocol의
// Network.setCookie 파라미터로 변환하는 순수 함수입니다.
// name/value가 없으면 주입할 수 없으므로 None을 반환합니다.
pub(super) fn cookie_to_cdp_param(cookie: &Value) -> Option<Value> {
    let name = cookie.get("name").and_then(Value::as_str)?;
    let value = cookie.get("value").and_then(Value::as_str)?;

    if name.is_empty() {
        return None;
    }

    let mut params = serde_json::Map::new();
    params.insert("name".to_owned(), json!(name));
    params.insert("value".to_owned(), json!(value));

    if let Some(domain) = cookie.get("domain").and_then(Value::as_str) {
        params.insert("domain".to_owned(), json!(domain));
    }

    // path는 없으면 "/"로 둡니다.
    let path = cookie.get("path").and_then(Value::as_str).unwrap_or("/");
    params.insert("path".to_owned(), json!(path));

    if let Some(secure) = cookie.get("secure").and_then(Value::as_bool) {
        params.insert("secure".to_owned(), json!(secure));
    }

    if let Some(http_only) = cookie.get("httpOnly").and_then(Value::as_bool) {
        params.insert("httpOnly".to_owned(), json!(http_only));
    }

    if let Some(same_site) = cookie.get("sameSite").and_then(Value::as_str) {
        // CDP는 "Strict" | "Lax" | "None" 만 허용합니다.
        if matches!(same_site, "Strict" | "Lax" | "None") {
            params.insert("sameSite".to_owned(), json!(same_site));
        }
    }

    // 세션 쿠키는 expires = -1 로 표시됩니다. 양수일 때만 만료시간을 넣습니다.
    if let Some(expires) = cookie.get("expires").and_then(Value::as_f64) {
        if expires > 0.0 {
            params.insert("expires".to_owned(), json!(expires));
        }
    }

    Some(Value::Object(params))
}

// 저장된 쿠키 JSON({"cookies": [...]})에서 주입 가능한 CDP 파라미터 목록을 만드는 함수입니다.
pub(super) fn cdp_params_from_saved_cookies(saved: &Value) -> Vec<Value> {
    saved
        .get("cookies")
        .and_then(Value::as_array)
        .map(|cookies| cookies.iter().filter_map(cookie_to_cdp_param).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_playwright_cookie_to_cdp_param() {
        let cookie = json!({
            "name": "NID_AUT",
            "value": "abc123",
            "domain": ".naver.com",
            "path": "/",
            "secure": true,
            "httpOnly": true,
            "sameSite": "Lax",
            "expires": 1_900_000_000.0
        });

        let param = cookie_to_cdp_param(&cookie).expect("should convert");

        assert_eq!(param["name"], json!("NID_AUT"));
        assert_eq!(param["value"], json!("abc123"));
        assert_eq!(param["domain"], json!(".naver.com"));
        assert_eq!(param["secure"], json!(true));
        assert_eq!(param["httpOnly"], json!(true));
        assert_eq!(param["sameSite"], json!("Lax"));
        assert_eq!(param["expires"], json!(1_900_000_000.0));
    }

    #[test]
    fn omits_expires_for_session_cookie() {
        let cookie = json!({
            "name": "NID_SES",
            "value": "ses",
            "domain": ".naver.com",
            "expires": -1.0
        });

        let param = cookie_to_cdp_param(&cookie).expect("should convert");

        assert!(param.get("expires").is_none());
        assert_eq!(param["path"], json!("/"));
    }

    #[test]
    fn skips_cookie_without_name_or_value() {
        assert!(cookie_to_cdp_param(&json!({ "value": "x" })).is_none());
        assert!(cookie_to_cdp_param(&json!({ "name": "x" })).is_none());
        assert!(cookie_to_cdp_param(&json!({ "name": "", "value": "x" })).is_none());
    }

    #[test]
    fn drops_invalid_same_site() {
        let cookie = json!({
            "name": "NID_AUT",
            "value": "v",
            "sameSite": "Unspecified"
        });

        let param = cookie_to_cdp_param(&cookie).expect("should convert");

        assert!(param.get("sameSite").is_none());
    }

    #[test]
    fn collects_all_cookies_from_saved_file() {
        let saved = json!({
            "accountId": "user@naver.com",
            "cookies": [
                { "name": "NID_AUT", "value": "a", "domain": ".naver.com" },
                { "name": "NID_SES", "value": "b", "domain": ".naver.com" },
                { "value": "no-name" }
            ]
        });

        let params = cdp_params_from_saved_cookies(&saved);

        assert_eq!(params.len(), 2);
        assert_eq!(params[0]["name"], json!("NID_AUT"));
        assert_eq!(params[1]["name"], json!("NID_SES"));
    }
}
