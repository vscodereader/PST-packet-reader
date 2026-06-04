//! 사용자가 입력한 URL 또는 문자열에서 카페 식별자를 추출하는 순수 함수 모듈.
//!
//! 네트워크 통신 없이 문자열 파싱만 수행한다. 숫자 cafeId를 직접 추출할 수 없고
//! vanity 이름만 있는 경우에는 [`CafeRef::Vanity`]를 반환하며, vanity → 숫자 id
//! 해석은 아직 확인된 API 엔드포인트가 없으므로 이 모듈 범위 밖이다.
//!
//! # 지원 입력 형식
//!
//! - `/cafes/{digits}` 경로 세그먼트를 포함하는 모든 URL:
//!   예) `https://cafe.naver.com/ca-fe/cafes/31732304/articles/write?boardType=L`
//! - 쿼리 파라미터 `clubid={digits}` (레거시)
//! - 쿼리 파라미터 `cafeId={digits}`
//! - 순수 숫자 문자열 (예: `31732304`)
//! - Vanity URL: `cafe.naver.com/<name>` (숫자 id 없음, `<name>`이 첫 번째 경로 세그먼트)
//!
//! # 우선순위
//!
//! 숫자 cafeId(`/cafes/{id}`, `clubid=`, `cafeId=`)가 vanity 이름보다 항상 우선이다.

// ---------------------------------------------------------------------------
// 공개 타입
// ---------------------------------------------------------------------------

/// 사용자가 입력한 URL/문자열에서 추출한 카페 식별자.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CafeRef {
    /// 숫자 cafeId를 직접 확보한 경우.
    Id(u64),
    /// 숫자 id 없이 vanity 이름만 있는 경우 (예: `cafe.naver.com/<name>`).
    ///
    /// 숫자 id 해석은 별도 네트워크 요청이 필요하다 —
    /// [`super::home::CafeHomeClient::resolve_slug`]가 카페 홈 HTML을 받아
    /// `g_sClubId`/`clubid`에서 숫자 cafeId를 파싱한다.
    Vanity(String),
}

// ---------------------------------------------------------------------------
// 공개 파싱 함수
// ---------------------------------------------------------------------------

/// 입력 문자열에서 카페 식별자를 파싱한다.
///
/// 앞뒤 공백을 제거한 후 다음 순서로 식별자를 탐색한다:
///
/// 1. `/cafes/{digits}` 경로 세그먼트 → [`CafeRef::Id`]
/// 2. 쿼리 파라미터 `clubid={digits}` → [`CafeRef::Id`]
/// 3. 쿼리 파라미터 `cafeId={digits}` → [`CafeRef::Id`]
/// 4. 전체 문자열이 숫자 → [`CafeRef::Id`]
/// 5. `cafe.naver.com/<name>` vanity 패턴 → [`CafeRef::Vanity`]
/// 6. 위 어디에도 해당하지 않으면 `None`
///
/// # 예시
///
/// ```rust
/// # use pstmacro_lib::naver_cafe::cafe_ref::parser::{parse_cafe_ref, CafeRef};
/// let r = parse_cafe_ref("https://cafe.naver.com/ca-fe/cafes/31732304/articles/write?boardType=L");
/// assert_eq!(r, Some(CafeRef::Id(31732304)));
///
/// let r = parse_cafe_ref("cafe.naver.com/bluegrayoc3uc");
/// assert_eq!(r, Some(CafeRef::Vanity("bluegrayoc3uc".to_string())));
///
/// let r = parse_cafe_ref("31732304");
/// assert_eq!(r, Some(CafeRef::Id(31732304)));
/// ```
pub fn parse_cafe_ref(input: &str) -> Option<CafeRef> {
    let s = input.trim();

    // 1. /cafes/{digits} 경로 세그먼트
    if let Some(id) = extract_cafes_path_id(s) {
        return Some(CafeRef::Id(id));
    }

    // 2. 쿼리 파라미터 clubid={digits} (레거시)
    if let Some(id) = extract_query_param_id(s, "clubid") {
        return Some(CafeRef::Id(id));
    }

    // 3. 쿼리 파라미터 cafeId={digits}
    if let Some(id) = extract_query_param_id(s, "cafeId") {
        return Some(CafeRef::Id(id));
    }

    // 4. 전체 문자열이 순수 숫자
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(id) = s.parse::<u64>() {
            return Some(CafeRef::Id(id));
        }
        // u64 오버플로 → 파싱 불가, 계속 진행
    }

    // 5. cafe.naver.com/<name> vanity 패턴
    if let Some(name) = extract_vanity_name(s) {
        return Some(CafeRef::Vanity(name));
    }

    None
}

/// 숫자 cafeId를 직접 얻을 수 있는 경우에만 `Some`을 반환한다.
///
/// [`CafeRef::Vanity`] 케이스(cafe.naver.com/<name>)는 `None`을 반환한다.
/// vanity → 숫자 id 해석은 아직 확인된 API 엔드포인트가 없으므로 지원하지 않는다.
///
/// # 예시
///
/// ```rust
/// # use pstmacro_lib::naver_cafe::cafe_ref::parser::parse_cafe_id;
/// assert_eq!(parse_cafe_id("31732304"), Some(31732304));
/// assert_eq!(parse_cafe_id("cafe.naver.com/myclub"), None);
/// ```
pub fn parse_cafe_id(input: &str) -> Option<u64> {
    match parse_cafe_ref(input)? {
        CafeRef::Id(id) => Some(id),
        CafeRef::Vanity(_) => None,
    }
}

// ---------------------------------------------------------------------------
// 내부 헬퍼
// ---------------------------------------------------------------------------

/// 경로 내 `/cafes/{digits}` 세그먼트에서 숫자 id를 추출한다.
///
/// `/cafes/` 다음에 오는 연속 ASCII 숫자를 읽으며, u64로 파싱한다.
/// 오버플로 시 `None`을 반환한다.
fn extract_cafes_path_id(s: &str) -> Option<u64> {
    // URL에서 경로 부분만 사용 (쿼리/프래그먼트 제거)
    let path_part = strip_query_and_fragment(s);

    // "/cafes/" 부분 문자열 탐색
    let marker = "/cafes/";
    let start = path_part.find(marker)?;
    let after_marker = &path_part[start + marker.len()..];

    // 연속 숫자 추출
    let digits: &str = {
        let end = after_marker
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_marker.len());
        &after_marker[..end]
    };

    if digits.is_empty() {
        return None;
    }

    digits.parse::<u64>().ok()
}

/// 쿼리 문자열에서 `key={digits}` 파라미터 값을 추출한다.
///
/// 대소문자를 구분하며, `u64` 파싱 실패(오버플로 포함) 시 `None`을 반환한다.
fn extract_query_param_id(s: &str, key: &str) -> Option<u64> {
    // '?' 이후 쿼리 문자열 찾기
    let query = s.find('?').map(|i| &s[i + 1..])?;

    // '#' 이전까지만 사용
    let query = query.find('#').map(|i| &query[..i]).unwrap_or(query);

    // '&'로 파라미터 분할
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        let k = parts.next().unwrap_or("");
        let v = parts.next().unwrap_or("");

        if k == key && !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(id) = v.parse::<u64>() {
                return Some(id);
            }
        }
    }

    None
}

/// `cafe.naver.com/<name>` 패턴에서 vanity 이름을 추출한다.
///
/// 스킴(`https://`, `http://`), `www.`, `m.` 접두사를 무시하며,
/// 첫 번째 경로 세그먼트가 `ca-fe`이거나 순수 숫자이면 `None`을 반환한다.
fn extract_vanity_name(s: &str) -> Option<String> {
    // 스킴 제거
    let s = strip_scheme(s);

    // www. / m. 접두사 제거
    let s = s
        .strip_prefix("www.")
        .or_else(|| s.strip_prefix("m."))
        .unwrap_or(s);

    // 호스트가 cafe.naver.com 인지 확인
    let after_host = s.strip_prefix("cafe.naver.com")?;

    // '/' 로 시작하는 경로가 있어야 함
    let path = after_host.strip_prefix('/')?;

    // 쿼리/프래그먼트/슬래시 이전 첫 세그먼트 추출
    let segment_end = path.find(['/', '?', '#']).unwrap_or(path.len());
    let segment = &path[..segment_end];

    if segment.is_empty() {
        return None;
    }

    // 'ca-fe' 는 카페 UI 경로 접두사이지 vanity 이름이 아님
    if segment == "ca-fe" {
        return None;
    }

    // 순수 숫자 세그먼트도 vanity가 아님 (숫자 id 경로)
    if segment.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    Some(segment.to_string())
}

/// `https://` 또는 `http://` 스킴을 제거하고 나머지를 반환한다.
fn strip_scheme(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("https://") {
        rest
    } else if let Some(rest) = s.strip_prefix("http://") {
        rest
    } else {
        s
    }
}

/// 쿼리(`?`) 및 프래그먼트(`#`) 이후 부분을 제거하고 경로만 반환한다.
fn strip_query_and_fragment(s: &str) -> &str {
    let end = s.find(['?', '#']).unwrap_or(s.len());
    &s[..end]
}

// ---------------------------------------------------------------------------
// 테스트
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // /cafes/{id} 경로 세그먼트 — 모던 URL 형식
    // ------------------------------------------------------------------

    #[test]
    fn parses_write_page_referer_exact() {
        // 패킷 캡처에서 실측된 Referer 그대로
        let url = "https://cafe.naver.com/ca-fe/cafes/31732304/articles/write?boardType=L";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_cafes_path_without_scheme() {
        let url = "/cafes/31732304/editor/menus";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_editor_v2_path() {
        let url = "/editor/v2.0/cafes/31732304/menus/1/articles";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_cafes_path_full_url_with_trailing_slash() {
        let url = "https://cafe.naver.com/ca-fe/cafes/31732304/";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_cafes_path_with_query_string() {
        let url = "https://cafe.naver.com/ca-fe/cafes/31732304?foo=bar";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_cafes_path_with_www_prefix() {
        let url = "https://www.cafe.naver.com/ca-fe/cafes/99999999";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(99_999_999)));
    }

    #[test]
    fn parses_cafes_path_with_m_prefix() {
        let url = "https://m.cafe.naver.com/ca-fe/cafes/12345678";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(12_345_678)));
    }

    #[test]
    fn parses_cafes_path_http_scheme() {
        let url = "http://cafe.naver.com/ca-fe/cafes/31732304/articles";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    // ------------------------------------------------------------------
    // 레거시 쿼리 파라미터 clubid=
    // ------------------------------------------------------------------

    #[test]
    fn parses_legacy_clubid_query_param() {
        let url = "https://cafe.naver.com/ArticleList.nhn?clubid=31732304&menuId=1";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_clubid_only_query_string() {
        assert_eq!(
            parse_cafe_ref("?clubid=31732304"),
            Some(CafeRef::Id(31732304))
        );
    }

    #[test]
    fn parses_clubid_first_in_query() {
        let url = "https://cafe.naver.com/index.nhn?clubid=42&menuId=7";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(42)));
    }

    // ------------------------------------------------------------------
    // 쿼리 파라미터 cafeId=
    // ------------------------------------------------------------------

    #[test]
    fn parses_cafe_id_query_param() {
        let url = "https://apis.naver.com/cafe-web/cafe2/CafeGateInfo.json?cafeId=31732304";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_cafe_id_query_param_with_other_params() {
        let url = "https://example.com/api?foo=bar&cafeId=55555&baz=qux";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(55555)));
    }

    #[test]
    fn parses_cafe_id_query_param_first_param() {
        let url = "https://example.com/api?cafeId=1234&other=value";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(1234)));
    }

    // ------------------------------------------------------------------
    // 순수 숫자 문자열 (bare numeric input)
    // ------------------------------------------------------------------

    #[test]
    fn parses_bare_numeric_string() {
        assert_eq!(parse_cafe_ref("31732304"), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn parses_bare_numeric_string_small() {
        assert_eq!(parse_cafe_ref("1"), Some(CafeRef::Id(1)));
    }

    #[test]
    fn parses_bare_numeric_string_with_whitespace() {
        assert_eq!(parse_cafe_ref("  31732304  "), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn bare_numeric_u64_max_parses_correctly() {
        // u64::MAX = 18446744073709551615 — 파싱 가능
        let max = u64::MAX.to_string();
        assert_eq!(parse_cafe_ref(&max), Some(CafeRef::Id(u64::MAX)));
    }

    #[test]
    fn bare_numeric_overflow_returns_none() {
        // u64::MAX + 1 — 오버플로
        let overflow = "18446744073709551616";
        assert_eq!(parse_cafe_ref(overflow), None);
    }

    // ------------------------------------------------------------------
    // Vanity URL — cafe.naver.com/<name>
    // ------------------------------------------------------------------

    #[test]
    fn parses_vanity_url_with_scheme() {
        let url = "https://cafe.naver.com/bluegrayoc3uc";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("bluegrayoc3uc".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_without_scheme() {
        let url = "cafe.naver.com/myclub";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("myclub".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_with_trailing_slash() {
        let url = "https://cafe.naver.com/myclub/";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("myclub".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_with_trailing_query() {
        let url = "https://cafe.naver.com/myclub?tab=main";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("myclub".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_with_www_prefix() {
        let url = "https://www.cafe.naver.com/myclub";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("myclub".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_with_m_prefix() {
        let url = "https://m.cafe.naver.com/myclub";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("myclub".to_string()))
        );
    }

    #[test]
    fn parses_vanity_url_http_scheme() {
        let url = "http://cafe.naver.com/legacyclub";
        assert_eq!(
            parse_cafe_ref(url),
            Some(CafeRef::Vanity("legacyclub".to_string()))
        );
    }

    // ca-fe는 vanity 이름이 아니라 UI 경로 접두사
    #[test]
    fn ca_fe_segment_is_not_vanity() {
        // /cafes/{id} 도 없고 ca-fe만 있는 경우
        let url = "https://cafe.naver.com/ca-fe";
        // ca-fe 세그먼트는 vanity로 취급하지 않으며, cafes/{id}도 없으므로 None
        assert_eq!(parse_cafe_ref(url), None);
    }

    #[test]
    fn ca_fe_prefix_with_cafes_id_returns_id() {
        // ca-fe 접두사 + /cafes/{id} → Id 우선
        let url = "https://cafe.naver.com/ca-fe/cafes/31732304";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    // ------------------------------------------------------------------
    // 우선순위: 숫자 id > vanity
    // ------------------------------------------------------------------

    #[test]
    fn cafes_path_id_takes_precedence_over_clubid() {
        // /cafes/{id} 와 clubid= 가 함께 있으면 /cafes/{id} 우선
        let url = "https://cafe.naver.com/ca-fe/cafes/31732304/articles?clubid=99999";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    #[test]
    fn clubid_takes_precedence_over_cafe_id_query() {
        // clubid= 가 cafeId= 보다 우선 (탐색 순서 2 < 3)
        let url = "https://example.com/api?clubid=111&cafeId=222";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(111)));
    }

    // ------------------------------------------------------------------
    // 오류 / 엣지 케이스
    // ------------------------------------------------------------------

    #[test]
    fn empty_string_returns_none() {
        assert_eq!(parse_cafe_ref(""), None);
    }

    #[test]
    fn whitespace_only_returns_none() {
        assert_eq!(parse_cafe_ref("   "), None);
    }

    #[test]
    fn malformed_string_returns_none() {
        assert_eq!(parse_cafe_ref("not-a-url-or-number!!"), None);
    }

    #[test]
    fn random_domain_with_no_matching_pattern_returns_none() {
        // 호스트와 무관하게 /cafes/{id} 경로 세그먼트는 추출된다 (경로 기반 파싱).
        // 아래는 어떤 패턴도 해당하지 않는 진정한 None 케이스들이다.
        assert_eq!(parse_cafe_ref("https://example.com/foo/bar"), None);
        assert_eq!(parse_cafe_ref("https://example.com"), None);
        assert_eq!(parse_cafe_ref("https://naver.com/something"), None);
    }

    #[test]
    fn path_based_extraction_is_host_agnostic() {
        // /cafes/{id} 경로 세그먼트 파싱은 호스트에 무관하다.
        // (파싱 설계 원칙: 경로만 보기 때문에 어떤 호스트의 URL이든 동작함)
        assert_eq!(
            parse_cafe_ref("https://example.com/cafes/12345"),
            Some(CafeRef::Id(12345))
        );
    }

    #[test]
    fn naver_cafe_root_without_path_returns_none() {
        assert_eq!(parse_cafe_ref("https://cafe.naver.com"), None);
        assert_eq!(parse_cafe_ref("cafe.naver.com"), None);
    }

    #[test]
    fn naver_cafe_root_with_slash_only_returns_none() {
        assert_eq!(parse_cafe_ref("https://cafe.naver.com/"), None);
    }

    #[test]
    fn non_numeric_cafe_ref_cannot_give_id() {
        // cafe.naver.com/<name> → Vanity 이지 Id 가 아님
        let url = "cafe.naver.com/myvanityclub";
        let result = parse_cafe_ref(url);
        assert_eq!(result, Some(CafeRef::Vanity("myvanityclub".to_string())));
    }

    // ------------------------------------------------------------------
    // parse_cafe_id — Vanity는 None
    // ------------------------------------------------------------------

    #[test]
    fn parse_cafe_id_returns_id_for_numeric_url() {
        assert_eq!(
            parse_cafe_id("https://cafe.naver.com/ca-fe/cafes/31732304"),
            Some(31732304)
        );
    }

    #[test]
    fn parse_cafe_id_returns_id_for_bare_number() {
        assert_eq!(parse_cafe_id("31732304"), Some(31732304));
    }

    #[test]
    fn parse_cafe_id_returns_none_for_vanity() {
        assert_eq!(parse_cafe_id("cafe.naver.com/bluegrayoc3uc"), None);
    }

    #[test]
    fn parse_cafe_id_returns_none_for_unrecognized() {
        assert_eq!(parse_cafe_id("not-a-thing"), None);
    }

    // ------------------------------------------------------------------
    // /cafes/{id} 경로가 example.com에도 동작하는지 확인
    // (경로 기반 파싱이므로 호스트 무관)
    // ------------------------------------------------------------------

    #[test]
    fn cafes_path_id_works_for_any_host() {
        // /editor/v2.0/cafes/{id}/menus/1/articles 는 내부 API 경로이므로
        // 호스트에 무관하게 경로 세그먼트만으로 추출
        let url = "https://apis.naver.com/editor/v2.0/cafes/31732304/menus/1/articles";
        assert_eq!(parse_cafe_ref(url), Some(CafeRef::Id(31732304)));
    }

    // ------------------------------------------------------------------
    // 입력 공백 처리
    // ------------------------------------------------------------------

    #[test]
    fn trims_leading_and_trailing_whitespace() {
        assert_eq!(
            parse_cafe_ref("  https://cafe.naver.com/ca-fe/cafes/31732304  "),
            Some(CafeRef::Id(31732304))
        );
    }
}
