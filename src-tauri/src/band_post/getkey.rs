//! band `md` 서명 키(`secretKey`) 발급 — `auth.band.us/s/login/getKey` JSONP.
//!
//! band-web은 API 호출 전에 이 엔드포인트를 호출해 세션별 `secretKey`를 받는다.
//! 응답은 JSONP 형태의 **JavaScript 코드**(유효한 JSON 아님)다:
//!
//! ```js
//! authCallBack_1780890966553(new BandWebAuthModule({
//!     secretKey: 'krYc6CZR5GYpPFSld8a/nPYnYMZ/Y2YHYGo5gYHHLSs=',
//!     timeCorrection: 1780890896326 - new Date().getTime(),
//!     authenticateState : "USER",
//!     inAppWebViewToken : false,
//!     isJwtType : false
//! }));
//! ```
//!
//! `secretKey`는 **세션 중 로테이션**되므로 각 API 호출 직전에 새로 받아야 안전하다.
//! 로그인 쿠키로 인증된다.
//!
//! # 보안
//! `secret_key`는 세션 자격 증명이다. 로그·에러·`Debug`에 절대 노출하지 않는다.

use crate::band_post::util::now_millis;

/// getKey 응답에서 파싱한 서명 파라미터.
#[derive(Clone)]
pub struct BandAuthKey {
    /// HMAC 서명 키. band-web `BandWebAuthModule.secretKey`.
    pub secret_key: String,
    /// 키 인코딩 분기 플래그. 일반 웹은 `false`(키를 UTF-8 그대로 사용).
    pub is_jwt_type: bool,
}

// 보안: secretKey가 Debug로 새지 않도록 수동 구현.
impl std::fmt::Debug for BandAuthKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BandAuthKey")
            .field("secret_key", &"<redacted>")
            .field("is_jwt_type", &self.is_jwt_type)
            .finish()
    }
}

/// getKey 엔드포인트 호스트.
pub const GETKEY_HOST: &str = "auth.band.us";

/// 밀리초 타임스탬프로 getKey 요청 경로를 만든다(band-web과 동일 형식).
///
/// 형식: `/s/login/getKey?_t={ts}&callback=authCallBack_{ts}&_={ts}`
pub fn getkey_path(ts: u128) -> String {
    format!("/s/login/getKey?_t={ts}&callback=authCallBack_{ts}&_={ts}")
}

/// 현재 시각 기준 getKey 요청 경로를 만든다.
pub fn getkey_path_now() -> String {
    getkey_path(now_millis())
}

/// JSONP 응답 텍스트에서 `secretKey`/`isJwtType`을 추출한다(순수 함수).
///
/// 응답이 유효한 JSON이 아니므로(`new BandWebAuthModule(...)`, JS 표현식 포함)
/// 키별 토큰 스캔으로 값을 뽑는다. `secretKey`는 작은/큰따옴표 양쪽을 허용한다.
///
/// `secretKey`를 찾지 못하면 `None`을 반환한다. `isJwtType`은 누락 시 `false`로 본다
/// (일반 웹 기본값).
pub fn parse_getkey_response(text: &str) -> Option<BandAuthKey> {
    let secret_key = extract_quoted_value(text, "secretKey")?;
    if secret_key.is_empty() {
        return None;
    }
    let is_jwt_type = extract_bool_value(text, "isJwtType").unwrap_or(false);
    Some(BandAuthKey {
        secret_key,
        is_jwt_type,
    })
}

/// `key : 'value'` 또는 `key : "value"` 형태에서 따옴표 안 값을 추출한다.
fn extract_quoted_value(text: &str, key: &str) -> Option<String> {
    let after_key = slice_after_key(text, key)?;
    let mut chars = after_key.char_indices();
    // 여는 따옴표를 찾는다.
    let (quote_char, start) = loop {
        let (idx, ch) = chars.next()?;
        match ch {
            '\'' | '"' => break (ch, idx + 1),
            c if c.is_whitespace() => continue,
            _ => return None, // 값이 따옴표로 시작하지 않음(예상 밖 형식)
        }
    };
    let rest = &after_key[start..];
    let end = rest.find(quote_char)?;
    Some(rest[..end].to_string())
}

/// `key : true|false` 형태에서 불리언을 추출한다.
fn extract_bool_value(text: &str, key: &str) -> Option<bool> {
    let after_key = slice_after_key(text, key)?;
    let trimmed = after_key.trim_start();
    if trimmed.starts_with("true") {
        Some(true)
    } else if trimmed.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// `key` 토큰을 찾아 그 뒤의 `:` 바로 다음부터의 슬라이스를 반환한다.
///
/// `key` 앞뒤가 식별자 문자가 아닌 위치만 매칭해(`isJwtType`이 `XisJwtType`에
/// 오매칭되지 않도록) 정확한 키를 찾는다.
fn slice_after_key<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut search_start = 0;
    while let Some(rel) = text[search_start..].find(key) {
        let idx = search_start + rel;
        let before_ok = idx == 0
            || !text[..idx]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let after = &text[idx + key.len()..];
        let after_ok = after
            .chars()
            .next()
            .is_some_and(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if before_ok && after_ok {
            // 키 다음의 ':' 위치를 찾아 그 뒤를 반환.
            let colon = after.find(':')?;
            return Some(&after[colon + 1..]);
        }
        search_start = idx + key.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // 자체 계정 패킷 캡처의 실제 getKey JSONP 응답(2026-06-08).
    const CAPTURED_GETKEY: &str = r#"authCallBack_1780890966553(new BandWebAuthModule({
        secretKey: 'krYc6CZR5GYpPFSld8a/nPYnYMZ/Y2YHYGo5gYHHLSs=',
        timeCorrection: 1780890896326 -new Date().getTime(),
        intervalRefreshRenewal: 129600000,
        signedUser: true,
        authenticateState : "USER",
        webRefreshUrl: "https://auth.band.us/_auth/refresh.jsonp",
        inAppWebViewToken : false,
        isJwtType : false
    })); }"#;

    #[test]
    fn parses_captured_secret_key() {
        let key = parse_getkey_response(CAPTURED_GETKEY).expect("파싱 성공해야 함");
        assert_eq!(key.secret_key, "krYc6CZR5GYpPFSld8a/nPYnYMZ/Y2YHYGo5gYHHLSs=");
        assert!(!key.is_jwt_type);
    }

    #[test]
    fn parsed_key_signs_captured_md() {
        // getKey 파싱 → 서명 → 캡처 md 일치까지 end-to-end로 확인.
        let key = parse_getkey_response(CAPTURED_GETKEY).unwrap();
        let md = crate::band_post::signature::make_md(
            &key.secret_key,
            "/v2.1.0/join_band?ts=1780890965873",
        );
        assert_eq!(md, "Nm+oy+Qc79phiYPT8igP3/Z/aY9r10qZDiDZpdS7s2o=");
    }

    #[test]
    fn parses_double_quoted_secret_key() {
        let text = r#"x(new BandWebAuthModule({ secretKey: "abc123=", isJwtType: false }))"#;
        let key = parse_getkey_response(text).unwrap();
        assert_eq!(key.secret_key, "abc123=");
    }

    #[test]
    fn parses_is_jwt_type_true() {
        let text = r#"({ secretKey: 'k', isJwtType : true })"#;
        let key = parse_getkey_response(text).unwrap();
        assert!(key.is_jwt_type);
    }

    #[test]
    fn missing_is_jwt_type_defaults_false() {
        let text = r#"({ secretKey: 'k' })"#;
        let key = parse_getkey_response(text).unwrap();
        assert!(!key.is_jwt_type);
    }

    #[test]
    fn returns_none_without_secret_key() {
        let text = r#"({ authenticateState: "USER", isJwtType: false })"#;
        assert!(parse_getkey_response(text).is_none());
    }

    #[test]
    fn returns_none_for_empty_secret_key() {
        let text = r#"({ secretKey: '' })"#;
        assert!(parse_getkey_response(text).is_none());
    }

    #[test]
    fn key_does_not_mismatch_similar_identifier() {
        // `mySecretKeyX` 같은 유사 식별자에 오매칭되지 않아야 함.
        let text = r#"({ mySecretKeyX: 'WRONG', secretKey: 'RIGHT=' })"#;
        let key = parse_getkey_response(text).unwrap();
        assert_eq!(key.secret_key, "RIGHT=");
    }

    #[test]
    fn getkey_path_has_callback_and_ts() {
        let path = getkey_path(1780890966553);
        assert_eq!(
            path,
            "/s/login/getKey?_t=1780890966553&callback=authCallBack_1780890966553&_=1780890966553"
        );
    }

    #[test]
    fn debug_does_not_leak_secret_key() {
        let key = parse_getkey_response(CAPTURED_GETKEY).unwrap();
        let dbg = format!("{key:?}");
        assert!(!dbg.contains("krYc6CZR"), "Debug에 secretKey가 노출됨: {dbg}");
        assert!(dbg.contains("redacted"));
    }
}
