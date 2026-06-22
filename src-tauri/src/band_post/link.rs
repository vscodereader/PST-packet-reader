//! 밴드 링크에서 `band_no`(밴드 번호)를 추출한다.
//!
//! 사용자가 입력하는 밴드 링크는 보통 `https://band.us/band/103043410` 형태이며,
//! 가입(`join_band`)은 이 `band_no`를 `join_value`로 사용한다.

/// 밴드 링크에서 `band_no` 숫자를 추출한다.
///
/// 지원 형태:
/// - `https://band.us/band/103043410`
/// - `https://www.band.us/band/103043410/post/2` (뒤 경로 무시)
/// - `band.us/band/103043410`
/// - `103043410` (숫자만)
///
/// 추출 실패 시 `None`. (초대 전용 단축링크 `band.us/n/...`는 미지원 — 후속 과제.)
pub fn band_no_from_link(link: &str) -> Option<String> {
    let trimmed = link.trim();

    // 숫자만 들어온 경우.
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Some(trimmed.to_string());
    }

    // `/band/{digits}` 패턴을 찾는다.
    let marker = "/band/";
    let start = trimmed.find(marker)? + marker.len();
    let rest = &trimmed[start..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

/// 밴드 **게시물** 링크에서 `post_no`(글 번호)를 추출한다.
///
/// "특정 게시글" 댓글 대상은 `band.us/band/{band_no}/post/{post_no}` 형태이며,
/// `create_comment(band_no, post_no, …)`에 그대로 쓸 수 있다. `/post/{digits}`가 없으면
/// (밴드 홈 링크 등) `None` — 특정 글이 아니므로 호출부가 댓글을 막는다.
pub fn post_no_from_link(link: &str) -> Option<u64> {
    let trimmed = link.trim();
    let marker = "/post/";
    let start = trimmed.find(marker)? + marker.len();
    let rest = &trimmed[start..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_post_no_from_post_url() {
        assert_eq!(
            post_no_from_link("https://www.band.us/band/103043410/post/57"),
            Some(57)
        );
        assert_eq!(
            post_no_from_link("band.us/band/103043410/post/2?ref=feed"),
            Some(2)
        );
    }

    #[test]
    fn post_no_is_none_without_post_segment() {
        // 밴드 홈 링크는 특정 글이 아니므로 post_no가 없다.
        assert_eq!(post_no_from_link("https://band.us/band/103043410"), None);
        assert_eq!(post_no_from_link("103043410"), None);
    }

    #[test]
    fn extracts_from_full_https_url() {
        assert_eq!(
            band_no_from_link("https://band.us/band/103043410").as_deref(),
            Some("103043410")
        );
    }

    #[test]
    fn extracts_with_www_and_trailing_path() {
        assert_eq!(
            band_no_from_link("https://www.band.us/band/103043410/post/2").as_deref(),
            Some("103043410")
        );
    }

    #[test]
    fn extracts_without_scheme() {
        assert_eq!(
            band_no_from_link("band.us/band/103043410").as_deref(),
            Some("103043410")
        );
    }

    #[test]
    fn accepts_bare_digits() {
        assert_eq!(band_no_from_link("103043410").as_deref(), Some("103043410"));
    }

    #[test]
    fn trims_whitespace() {
        assert_eq!(
            band_no_from_link("  https://band.us/band/103043410  ").as_deref(),
            Some("103043410")
        );
    }

    #[test]
    fn none_for_invitation_short_link() {
        assert!(band_no_from_link("https://band.us/n/abcd1234").is_none());
    }

    #[test]
    fn none_for_garbage() {
        assert!(band_no_from_link("not a link").is_none());
        assert!(band_no_from_link("").is_none());
    }
}
