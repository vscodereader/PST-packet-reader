//! 네이버 카페 쓰기 API 공용 요청 헤더.
//!
//! 글 작성([`post`](crate::naver_cafe::post))과 댓글 작성
//! ([`comment`](crate::naver_cafe::comment)) 요청은 `Content-Type`과 `Referer`만
//! 다르고 나머지 브라우저 위장 헤더(Origin·x-cafe-product·sec-fetch-*·accept-language)
//! 와 상수가 동일하다. 두 request_builder가 같은 세트를 복제하던 것을 한곳에 모은다.
//!
//! `User-Agent`는 reqwest/상위 레이어가 붙이며 여기 포함하지 않는다.

/// Origin 헤더 값 — 패킷 캡처에서 확인된 값.
const ORIGIN: &str = "https://cafe.naver.com";

/// `x-cafe-product` 헤더 값.
///
/// `apis.cafe.naver.com/editor/*` 에디터 서비스가 이 헤더를 요구하며, 없으면
/// errorCode 10404(Page Not Found) 또는 11001을 반환한다(댓글 API도 동일 헤더 사용).
pub(crate) const CAFE_PRODUCT_PC: &str = "pc";

/// 네이버 카페 **읽기**(GET) 요청 공용 헤더 세트를 반환한다.
///
/// `referer`만 호출부가 정하고, 나머지(Accept·Origin·x-cafe-product·sec-fetch-*·
/// accept-language)는 공통으로 채운다. `cafe-boardlist-api`(최신글)·`cafe2`
/// 인기글 등 조회 API는 `x-cafe-product: pc`가 없으면 HTTP 500(errorCode 9999)을
/// 반환하므로(실패킷 확인) 이 헤더가 필수다.
pub(crate) fn cafe_read_headers(referer: String) -> Vec<(String, String)> {
    vec![
        (
            "Accept".to_string(),
            "application/json, text/plain, */*".to_string(),
        ),
        ("Origin".to_string(), ORIGIN.to_string()),
        ("Referer".to_string(), referer),
        ("x-cafe-product".to_string(), CAFE_PRODUCT_PC.to_string()),
        ("sec-fetch-site".to_string(), "same-site".to_string()),
        ("sec-fetch-mode".to_string(), "cors".to_string()),
        ("sec-fetch-dest".to_string(), "empty".to_string()),
        (
            "accept-language".to_string(),
            "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7".to_string(),
        ),
    ]
}

/// 네이버 카페 쓰기 요청 공용 헤더 세트를 반환한다.
///
/// 읽기 헤더([`cafe_read_headers`])에 `Content-Type`만 앞에 더한 형태로,
/// `content_type`과 `referer`만 호출부가 정한다.
pub(crate) fn cafe_write_headers(content_type: &str, referer: String) -> Vec<(String, String)> {
    let mut headers = cafe_read_headers(referer);
    headers.insert(0, ("Content-Type".to_string(), content_type.to_string()));
    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn cafe_write_headers_sets_content_type_and_referer_from_args() {
        let h = cafe_write_headers("application/json", "https://ref".to_string());
        assert_eq!(value(&h, "Content-Type"), Some("application/json"));
        assert_eq!(value(&h, "Referer"), Some("https://ref"));
    }

    #[test]
    fn cafe_write_headers_includes_common_disguise_headers() {
        let h = cafe_write_headers("x", "y".to_string());
        assert_eq!(value(&h, "Origin"), Some("https://cafe.naver.com"));
        assert_eq!(value(&h, "x-cafe-product"), Some("pc"));
        assert_eq!(value(&h, "sec-fetch-site"), Some("same-site"));
        assert_eq!(value(&h, "sec-fetch-mode"), Some("cors"));
        assert_eq!(value(&h, "sec-fetch-dest"), Some("empty"));
        assert_eq!(
            value(&h, "accept-language"),
            Some("ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7")
        );
    }
}
