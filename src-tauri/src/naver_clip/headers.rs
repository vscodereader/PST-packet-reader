//! 네이버 클립 요청 공용 위장 헤더(#클립). 블로그(`naver_blog::headers`)와 같은 이유로,
//! 로그인 세션인데 브라우저 지문(Referer·sec-fetch 등)이 없는 요청은 봇 차단/오작동할 수 있어
//! 실제 패킷에서 확인된 헤더 세트를 싣는다. `User-Agent`는 호출부가 BROWSER_USER_AGENT로 붙인다.

/// 클립 웹 Origin(cbox·graphql same-site/same-origin 요청용).
pub(crate) const CLIP_ORIGIN: &str = "https://clip.naver.com";
/// 모바일 메인 Origin(creatorhub 프로필 API는 m.naver.com 컨텍스트에서 호출된다).
pub(crate) const MOBILE_ORIGIN: &str = "https://m.naver.com";

const ACCEPT_LANGUAGE: &str = "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7";

/// cbox 댓글 API(apis.naver.com, same-site) GET/POST용 위장 헤더. `referer`는 클립 contents URL.
pub(crate) fn clip_cbox_headers(referer: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Accept", "*/*".to_string()),
        ("Origin", CLIP_ORIGIN.to_string()),
        ("Referer", referer.to_string()),
        ("sec-fetch-site", "same-site".to_string()),
        ("sec-fetch-mode", "cors".to_string()),
        ("sec-fetch-dest", "empty".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

/// clip.naver.com/api/graphql(same-origin) POST용 위장 헤더(Content-Type=JSON).
pub(crate) fn clip_graphql_headers(referer: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Accept", "*/*".to_string()),
        ("Content-Type", "application/json".to_string()),
        ("Origin", CLIP_ORIGIN.to_string()),
        ("Referer", referer.to_string()),
        ("sec-fetch-site", "same-origin".to_string()),
        ("sec-fetch-mode", "cors".to_string()),
        ("sec-fetch-dest", "empty".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

/// `clip.naver.com/@<handle>` 문서 GET용 위장 헤더(navigation/document).
pub(crate) fn clip_document_headers() -> Vec<(&'static str, String)> {
    vec![
        (
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"
                .to_string(),
        ),
        ("Upgrade-Insecure-Requests", "1".to_string()),
        ("sec-fetch-site", "none".to_string()),
        ("sec-fetch-mode", "navigate".to_string()),
        ("sec-fetch-user", "?1".to_string()),
        ("sec-fetch-dest", "document".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

/// creatorhub-api.naver.com 프로필 API용 헤더. `x-creator-hub-sid: clip`이 필수(실측).
pub(crate) fn creatorhub_headers() -> Vec<(&'static str, String)> {
    vec![
        ("Accept", "*/*".to_string()),
        ("x-creator-hub-sid", "clip".to_string()),
        ("Origin", MOBILE_ORIGIN.to_string()),
        ("Referer", format!("{}/", MOBILE_ORIGIN)),
        ("sec-fetch-site", "same-site".to_string()),
        ("sec-fetch-mode", "cors".to_string()),
        ("sec-fetch-dest", "empty".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(h: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        h.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    #[test]
    fn cbox_headers_set_clip_origin_and_cors() {
        let h = clip_cbox_headers("https://clip.naver.com/contents?x");
        assert_eq!(value(&h, "Origin"), Some("https://clip.naver.com"));
        assert_eq!(value(&h, "sec-fetch-site"), Some("same-site"));
        assert_eq!(
            value(&h, "Referer"),
            Some("https://clip.naver.com/contents?x")
        );
    }

    #[test]
    fn graphql_headers_are_json_same_origin() {
        let h = clip_graphql_headers("https://clip.naver.com/@x");
        assert_eq!(value(&h, "Content-Type"), Some("application/json"));
        assert_eq!(value(&h, "sec-fetch-site"), Some("same-origin"));
    }

    #[test]
    fn creatorhub_headers_carry_sid() {
        let h = creatorhub_headers();
        assert_eq!(value(&h, "x-creator-hub-sid"), Some("clip"));
        assert_eq!(value(&h, "Origin"), Some("https://m.naver.com"));
    }

    #[test]
    fn document_headers_navigate() {
        let h = clip_document_headers();
        assert_eq!(value(&h, "sec-fetch-mode"), Some("navigate"));
        assert!(value(&h, "Accept").unwrap().starts_with("text/html"));
    }
}
