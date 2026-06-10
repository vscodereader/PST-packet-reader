//! band.us `api-kr.band.us` 요청의 `md` 서명 헤더 계산.
//!
//! band-web 클라이언트(`boot.bundle.js`의 `createMd` → `BandWebAuthModule.makeMd`,
//! `auth/all.js`의 sjcl HMAC)를 그대로 미러한 순수 함수다.
//!
//! ```text
//! md = base64_standard( HMAC-SHA256( key = utf8(secretKey), msg = path ) )
//!   path = "/v2.1.0/join_band?ts=<ms>"   (스킴+호스트 제거, ts 쿼리 포함, 원문 그대로)
//! ```
//!
//! `secretKey`는 `auth.band.us/s/login/getKey` JSONP 응답으로 서버가 세션마다 주는
//! 동적값이며(세션 중 로테이션됨), 일반 웹 클라이언트(`isJwtType=false`)에서는
//! 그 문자열을 UTF-8 바이트 그대로 HMAC 키로 쓴다.
//!
//! # 보안
//! `secret_key`는 사용자 세션 자격 증명이다. 로그·에러·`Debug`에 절대 노출하지 않는다.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// 고정 앱 키. `akey` 헤더 값으로 쓰이며 **HMAC 키가 아니다**(band-web `APP_KEY` 상수).
pub const APP_KEY: &str = "bbc59b0b5f7a1c6efe950f6236ccda35";

/// 임의의 키 바이트로 `path`에 대한 `md` 서명을 만든다(코어).
///
/// 출력은 표준 base64(`=` 패딩 포함, base64url 아님).
pub fn make_md_bytes(key: &[u8], path: &str) -> String {
    // HMAC은 임의 키 길이를 허용하므로 `new_from_slice`는 실패하지 않는다.
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC은 모든 키 길이를 허용");
    mac.update(path.as_bytes());
    STANDARD.encode(mac.finalize().into_bytes())
}

/// `secret_key`로 `path`(스킴/호스트 제거된 경로+쿼리)에 대한 `md` 서명을 만든다.
///
/// band-web 일반 웹 케이스(`isJwtType=false`)를 미러한다: 키는 `secret_key`의
/// UTF-8 바이트 그대로.
pub fn make_md(secret_key: &str, path: &str) -> String {
    make_md_bytes(secret_key.as_bytes(), path)
}

/// JWT/인앱 케이스(`isJwtType=true`)의 `md` 서명: `secret_key`를 base64url 디코드한
/// 바이트를 HMAC 키로 쓴다. 디코드 실패 시 UTF-8 폴백(웹 동작 보존).
pub fn make_md_jwt(secret_key: &str, path: &str) -> String {
    match base64::engine::general_purpose::URL_SAFE.decode(secret_key) {
        Ok(bytes) => make_md_bytes(&bytes, path),
        Err(_) => make_md(secret_key, path),
    }
}

/// 전체 URL에서 서명 대상 경로를 추출한다(band-web `createMd`+`extractPath` 미러).
///
/// 1. `'` → `%27` 치환(비 IE/Edge 분기와 동일)
/// 2. 스킴(`scheme://`) 제거 후 첫 호스트 토큰 제거 → 선두 `/`부터의 경로+쿼리만 남김
///
/// 이미 경로 형태(`/v2...`)를 넘기면 `'` 치환만 적용되고 그대로 반환된다.
pub fn extract_path(url: &str) -> String {
    let replaced = url.replace('\'', "%27");
    // 스킴 제거: 첫 "://" 앞부분을 버린다.
    let after_scheme = match replaced.find("://") {
        Some(idx) => &replaced[idx + 3..],
        None => replaced.as_str(),
    };
    // 호스트 제거: 첫 '/' 부터가 경로다. '/'가 없으면(경로 없음) 빈 문자열.
    match after_scheme.find('/') {
        Some(idx) => after_scheme[idx..].to_string(),
        None => {
            // 이미 경로만 들어온 경우(스킴/호스트 없음)는 원문 유지.
            if replaced.starts_with('/') {
                replaced
            } else {
                String::new()
            }
        }
    }
}

/// `base_path`(예: `/v2.1.0/join_band`)와 밀리초 타임스탬프로 서명 대상 경로를 만든다.
///
/// 형식: `"{base_path}?ts={ts}"`. band api는 모든 요청에 `ts` 쿼리를 요구하며 이 값이
/// 서명에 포함된다.
pub fn signed_path(base_path: &str, ts: u128) -> String {
    format!("{base_path}?ts={ts}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // 자체 계정 패킷 캡처에서 추출한 실측값(2026-06-08). secretKey는 getKey 응답값.
    // 이 3쌍이 알고리즘의 정답지(ground truth)다.
    const CAPTURED_SECRET_KEY: &str = "krYc6CZR5GYpPFSld8a/nPYnYMZ/Y2YHYGo5gYHHLSs=";

    #[test]
    fn make_md_matches_captured_join_band() {
        let md = make_md(CAPTURED_SECRET_KEY, "/v2.1.0/join_band?ts=1780890965873");
        assert_eq!(md, "Nm+oy+Qc79phiYPT8igP3/Z/aY9r10qZDiDZpdS7s2o=");
    }

    #[test]
    fn make_md_matches_captured_create_post() {
        let md = make_md(CAPTURED_SECRET_KEY, "/v2.0.2/create_post?ts=1780890984348");
        assert_eq!(md, "S+G4y0pmebWiNqr7V+oxEuoIyD1q5OM+Ph1vH+tw9Gs=");
    }

    #[test]
    fn make_md_matches_captured_create_comment() {
        let md = make_md(
            CAPTURED_SECRET_KEY,
            "/v2.3.0/create_comment?ts=1780890998911",
        );
        assert_eq!(md, "nojSU1DGFjhI6Nw9v0QCqMNFMtUcTebqWROqEKoh57Q=");
    }

    #[test]
    fn make_md_is_standard_base64_with_padding() {
        // 표준 base64는 32바이트(SHA-256) → 44자, 끝에 '=' 패딩.
        let md = make_md(CAPTURED_SECRET_KEY, "/v2.1.0/join_band?ts=1780890965873");
        assert_eq!(md.len(), 44);
        assert!(md.ends_with('='));
        // base64url이 아님을 확인(+/ 사용, -_ 아님).
        assert!(!md.contains('-') && !md.contains('_'));
    }

    #[test]
    fn make_md_changes_with_path() {
        let a = make_md(CAPTURED_SECRET_KEY, "/v2.1.0/join_band?ts=1");
        let b = make_md(CAPTURED_SECRET_KEY, "/v2.1.0/join_band?ts=2");
        assert_ne!(a, b, "ts가 다르면 md도 달라야 함");
    }

    #[test]
    fn make_md_changes_with_secret_key() {
        let a = make_md(CAPTURED_SECRET_KEY, "/v2.1.0/join_band?ts=1780890965873");
        let b = make_md(
            "2WOPaTlBF5aAaaqorjrT3oCU884Anieu3LZKLbpxLpA=",
            "/v2.1.0/join_band?ts=1780890965873",
        );
        assert_ne!(a, b, "secretKey가 다르면 md도 달라야 함");
    }

    #[test]
    fn extract_path_strips_scheme_and_host() {
        assert_eq!(
            extract_path("https://api-kr.band.us/v2.1.0/join_band?ts=1780890965873"),
            "/v2.1.0/join_band?ts=1780890965873"
        );
    }

    #[test]
    fn extract_path_keeps_bare_path() {
        assert_eq!(
            extract_path("/v2.0.2/create_post?ts=1"),
            "/v2.0.2/create_post?ts=1"
        );
    }

    #[test]
    fn extract_path_replaces_single_quote() {
        // band-web은 서명 전 작은따옴표를 %27로 치환한다.
        assert_eq!(
            extract_path("https://api-kr.band.us/v2/x?q=a'b"),
            "/v2/x?q=a%27b"
        );
    }

    #[test]
    fn extract_path_then_make_md_matches_capture() {
        // 전체 URL → extract_path → make_md 가 캡처값과 일치해야 함.
        let path = extract_path("https://api-kr.band.us/v2.1.0/join_band?ts=1780890965873");
        assert_eq!(
            make_md(CAPTURED_SECRET_KEY, &path),
            "Nm+oy+Qc79phiYPT8igP3/Z/aY9r10qZDiDZpdS7s2o="
        );
    }

    #[test]
    fn signed_path_appends_ts_query() {
        assert_eq!(
            signed_path("/v2.1.0/join_band", 1780890965873),
            "/v2.1.0/join_band?ts=1780890965873"
        );
    }

    #[test]
    fn signed_path_then_make_md_matches_capture() {
        let path = signed_path("/v2.0.2/create_post", 1780890984348);
        assert_eq!(
            make_md(CAPTURED_SECRET_KEY, &path),
            "S+G4y0pmebWiNqr7V+oxEuoIyD1q5OM+Ph1vH+tw9Gs="
        );
    }
}
