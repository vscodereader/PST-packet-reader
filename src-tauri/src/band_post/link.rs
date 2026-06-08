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

#[cfg(test)]
mod tests {
    use super::*;

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
