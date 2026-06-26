//! 네이버 블로그 조회 요청 공용 위장 헤더(#312).
//!
//! 네이버는 **유효한 로그인 세션인데 브라우저 지문(Referer·Accept·sec-fetch-*)이 없는**
//! 본문 요청을 자동화로 보고, `PostView.naver`에 봇 인터스티셜(HTTP 200이지만 `var blogNo`가
//! 없는 빈 페이지)을 돌려준다. 그러면 [`crate::naver_blog::comment_client::parse_group_id`]가
//! groupId를 못 찾아 "groupId를 찾지 못했습니다"로 실패한다(댓글이 안 달리던 근본 원인).
//!
//! 익명 요청은 "그냥 크롤러"라 통과되지만(헤더 없이도 blogNo 옴), 저장 쿠키를 실은 로그인
//! 세션은 차단된다 — 실제 패킷에서 로그인 브라우저는 아래 헤더를 모두 싣고 정상 응답을 받았다.
//! 카페 경로([`crate::naver_cafe::headers`])가 같은 이유로 위장 헤더를 싣는 것의 블로그 버전이다.
//!
//! `User-Agent`는 호출부가 [`crate::naver_cafe::post::BROWSER_USER_AGENT`]로 따로 붙인다.

/// 블로그 Origin 헤더 값(cbox 등 same-site 요청용).
pub(crate) const BLOG_ORIGIN: &str = "https://blog.naver.com";

/// 공용 `accept-language`(실측 브라우저 값).
const ACCEPT_LANGUAGE: &str = "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7";

/// **문서 내비게이션**(PostView.naver·PostTitleListAsync.naver) GET용 위장 헤더.
///
/// 브라우저가 블로그 본문을 직접 여는 것처럼 보이게 한다(`sec-fetch-mode: navigate` +
/// `dest: document` + `Accept: text/html`). `referer`만 호출부가 정하고(같은 오리진의 블로그
/// 홈을 권장 → `sec-fetch-site: same-origin`과 일관), 나머지는 공통으로 채운다.
pub(crate) fn blog_document_headers(referer: &str) -> Vec<(&'static str, String)> {
    vec![
        (
            "Accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"
                .to_string(),
        ),
        ("Referer", referer.to_string()),
        ("Upgrade-Insecure-Requests", "1".to_string()),
        ("sec-fetch-site", "same-origin".to_string()),
        ("sec-fetch-mode", "navigate".to_string()),
        ("sec-fetch-user", "?1".to_string()),
        ("sec-fetch-dest", "document".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

/// **cbox XHR**(apis.naver.com web_naver_* GET) 용 위장 헤더.
///
/// 블로그 페이지의 스크립트가 댓글 API를 부르는 것처럼 보이게 한다(`Origin: blog.naver.com`,
/// `sec-fetch-site: same-site`, `mode: cors`, `dest: empty`). `referer`만 호출부가 정한다.
pub(crate) fn blog_cbox_headers(referer: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Accept", "*/*".to_string()),
        ("Origin", BLOG_ORIGIN.to_string()),
        ("Referer", referer.to_string()),
        ("sec-fetch-site", "same-site".to_string()),
        ("sec-fetch-mode", "cors".to_string()),
        ("sec-fetch-dest", "empty".to_string()),
        ("accept-language", ACCEPT_LANGUAGE.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(headers: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn document_headers_set_referer_and_browser_fingerprint() {
        let h = blog_document_headers("https://blog.naver.com/press02");
        assert_eq!(value(&h, "Referer"), Some("https://blog.naver.com/press02"));
        assert!(value(&h, "Accept").unwrap().starts_with("text/html"));
        assert_eq!(value(&h, "sec-fetch-mode"), Some("navigate"));
        assert_eq!(value(&h, "sec-fetch-dest"), Some("document"));
        assert_eq!(value(&h, "Upgrade-Insecure-Requests"), Some("1"));
        assert!(value(&h, "accept-language").is_some());
    }

    #[test]
    fn cbox_headers_set_origin_referer_and_cors_fingerprint() {
        let h = blog_cbox_headers("https://blog.naver.com/PostView.naver?blogId=b&logNo=1");
        assert_eq!(value(&h, "Origin"), Some("https://blog.naver.com"));
        assert_eq!(
            value(&h, "Referer"),
            Some("https://blog.naver.com/PostView.naver?blogId=b&logNo=1")
        );
        assert_eq!(value(&h, "sec-fetch-site"), Some("same-site"));
        assert_eq!(value(&h, "sec-fetch-mode"), Some("cors"));
        assert_eq!(value(&h, "Accept"), Some("*/*"));
    }
}
