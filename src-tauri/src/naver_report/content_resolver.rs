//! 링크 → 신고 파라미터 해석. **순수 파싱**(itemCode·postId·contentId)은 이 파일에서 유닛
//! 테스트로 고정하고, `writer.profileId` → `encryptedUserId` 해석은 저장 쿠키만으로 하는
//! best-effort HTTP(설계서 §2.2 체인)로 둔다.
//!
//! 체인(설계서 §2.2):
//! ```text
//! https://stock.naver.com/domestic/stock/000660/discussion/425406371
//!   → itemCode=000660, postId=425406371
//!   → GET .../posts/by-item?...&itemCode={code}  에서 id==postId → writer.profileId
//!   → GET .../profile/users/{profileId}          → encryptedUserId
//!   → contentId = "FIN_001;item;board;" + postId
//! ```

use serde_json::Value;

use super::error::ReportError;
use super::report_client::ReportHttp;

/// 종목토론방 글 링크에서 뽑은 (itemCode, postId). 이후 조회·contentId 빌드에 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscussionLink {
    /// 종목 코드(예: `000660`, ETF는 `0193T0` 같은 영숫자).
    pub item_code: String,
    /// 글 ID(예: `425406371`).
    pub post_id: String,
}

/// contentId 접두(설계서 §2.2). `FIN_001;item;board;{postId}` 형태로 붙는다.
const CONTENT_ID_PREFIX: &str = "FIN_001;item;board;";

/// 종목토론방 글 링크를 파싱해 (itemCode, postId)를 뽑는다(순수 함수).
///
/// 허용 형태: `https://stock.naver.com/domestic/stock/{itemCode}/discussion/{postId}`
/// (앞뒤 공백·쿼리스트링·끝 슬래시는 무시). itemCode는 영숫자, postId는 숫자다.
pub fn parse_discussion_link(url: &str) -> Result<DiscussionLink, ReportError> {
    let trimmed = url.trim();
    // 경로에서 "/stock/{code}/discussion/{id}" 조각을 찾는다. 쿼리/프래그먼트는 잘라낸다.
    let path = trimmed
        .split(['?', '#'])
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    let after_stock = path
        .split("/stock/")
        .nth(1)
        .ok_or_else(|| ReportError::InvalidLink(format!("'/stock/'가 없는 링크: {url}")))?;
    let (item_code, rest) = after_stock
        .split_once("/discussion/")
        .ok_or_else(|| ReportError::InvalidLink(format!("'/discussion/'가 없는 링크: {url}")))?;
    // discussion 뒤 첫 경로 조각만 postId로 취한다(추가 경로가 붙어도 안전).
    let post_id = rest.split('/').next().unwrap_or(rest);

    if item_code.is_empty() || !item_code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ReportError::InvalidLink(format!(
            "종목 코드가 비었거나 영숫자가 아님: {url}"
        )));
    }
    if post_id.is_empty() || !post_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(ReportError::InvalidLink(format!(
            "글 ID가 비었거나 숫자가 아님: {url}"
        )));
    }

    Ok(DiscussionLink {
        item_code: item_code.to_owned(),
        post_id: post_id.to_owned(),
    })
}

/// postId로 신고 contentId를 만든다(순수 함수). 설계서 §2.2: `FIN_001;item;board;{postId}`.
pub fn build_content_id(post_id: &str) -> String {
    format!("{CONTENT_ID_PREFIX}{post_id}")
}

/// by-item 목록 JSON에서 `id == post_id`인 글의 `writer.profileId`를 뽑는다(순수 함수 — 실제
/// HTTP 없이 파싱만 테스트 가능). 응답 스키마 변동에 대비해 여러 형태(`profileId`가 writer 안/밖)를
/// 관대하게 훑는다.
pub fn extract_profile_id(by_item: &Value, post_id: &str) -> Option<String> {
    let posts = find_posts_array(by_item)?;
    for post in posts {
        let id = post.get("id").map(value_to_id_string).unwrap_or_default();
        if id != post_id {
            continue;
        }
        if let Some(profile_id) = post
            .get("writer")
            .and_then(|w| w.get("profileId"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return Some(profile_id.to_owned());
        }
        // 일부 응답은 profileId를 글 최상위에 둘 수 있다 — 폴백.
        if let Some(profile_id) = post
            .get("profileId")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return Some(profile_id.to_owned());
        }
    }
    None
}

/// profile/users/{profileId} 응답 JSON에서 `encryptedUserId`(=cwriterenc)를 뽑는다(순수 함수).
/// 응답이 `{...}` 또는 `{"result":{...}}`로 감싸일 수 있어 두 경로를 모두 본다.
pub fn extract_encrypted_user_id(profile: &Value) -> Option<String> {
    profile
        .get("encryptedUserId")
        .or_else(|| profile.get("result").and_then(|r| r.get("encryptedUserId")))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// by-item 응답에서 글 배열을 찾는다. 실측은 `{"posts":[...]}`이나, 스키마 변동에 대비해
/// 흔한 래핑(`result.posts`, 최상위 배열)도 관대하게 훑는다.
fn find_posts_array(value: &Value) -> Option<&Vec<Value>> {
    if let Some(arr) = value.get("posts").and_then(Value::as_array) {
        return Some(arr);
    }
    if let Some(arr) = value
        .get("result")
        .and_then(|r| r.get("posts"))
        .and_then(Value::as_array)
    {
        return Some(arr);
    }
    value.as_array()
}

/// `id` 필드는 숫자/문자열 어느 쪽으로도 올 수 있어, postId(문자열)와 비교할 수 있게 문자열로 만든다.
fn value_to_id_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// 저장 쿠키만으로 링크의 encryptedUserId를 해석한다(best-effort HTTP — 실기기 검증 대상).
/// by-item(itemCode) → id==postId → profileId → profile/users → encryptedUserId 체인을 탄다.
/// 순수 파싱은 위 함수들로 분리·테스트되고, 여기서는 네트워크 왕복만 담당한다.
pub fn resolve_encrypted_user_id(
    http: &ReportHttp,
    link: &DiscussionLink,
) -> Result<String, ReportError> {
    // 실측(신고 패킷 + 400 응답 원문 확정): by-item 은 bool 파라미터 isHolderOnly/excludesItemNews/
    // isItemNewsOnly 를 **필수**로 요구한다(누락 시 400 `{"fieldErrors":{...:["Required"]}}`). 브라우저와
    // 동일하게 셋 다 false 로 붙인다. isCleanbotPassedOnly=false 는 서버가 그대로 받아들인다(에러 없음).
    let by_item_url = format!(
        "https://stock.naver.com/api/community/discussion/posts/by-item\
?discussionType=domesticStock&itemCode={}\
&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false\
&isCleanbotPassedOnly=false&pageSize=30",
        link.item_code
    );
    let by_item = http
        .get_json(&by_item_url)
        .map_err(|e| ReportError::Resolve(format!("by-item 조회 실패({}): {e}", link.item_code)))?;
    let profile_id = extract_profile_id(&by_item, &link.post_id).ok_or_else(|| {
        ReportError::Resolve(format!(
            "by-item 목록에서 글 id={} 의 profileId를 찾지 못함",
            link.post_id
        ))
    })?;

    let profile_url = format!("https://stock.naver.com/api/community/profile/users/{profile_id}");
    let profile = http
        .get_json(&profile_url)
        .map_err(|e| ReportError::Resolve(format!("profile 조회 실패({profile_id}): {e}")))?;
    extract_encrypted_user_id(&profile).ok_or_else(|| {
        ReportError::Resolve(format!(
            "profile 응답에 encryptedUserId 없음(profileId={profile_id})"
        ))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_item_code_and_post_id_from_canonical_link() {
        let link = parse_discussion_link(
            "https://stock.naver.com/domestic/stock/000660/discussion/425406371",
        )
        .unwrap();
        assert_eq!(link.item_code, "000660");
        assert_eq!(link.post_id, "425406371");
    }

    #[test]
    fn parses_link_with_query_and_trailing_slash() {
        let link = parse_discussion_link(
            "  https://stock.naver.com/domestic/stock/005930/discussion/424274129/?tab=all#c  ",
        )
        .unwrap();
        assert_eq!(link.item_code, "005930");
        assert_eq!(link.post_id, "424274129");
    }

    #[test]
    fn parses_alphanumeric_etf_item_code() {
        let link = parse_discussion_link(
            "https://stock.naver.com/domestic/stock/0193T0/discussion/900000001",
        )
        .unwrap();
        assert_eq!(link.item_code, "0193T0");
        assert_eq!(link.post_id, "900000001");
    }

    #[test]
    fn rejects_link_without_discussion_segment() {
        let err =
            parse_discussion_link("https://stock.naver.com/domestic/stock/000660").unwrap_err();
        assert!(matches!(err, ReportError::InvalidLink(_)));
    }

    #[test]
    fn rejects_non_numeric_post_id() {
        let err =
            parse_discussion_link("https://stock.naver.com/domestic/stock/000660/discussion/abc")
                .unwrap_err();
        assert!(matches!(err, ReportError::InvalidLink(_)));
    }

    #[test]
    fn builds_content_id_with_fin_prefix() {
        assert_eq!(
            build_content_id("425406371"),
            "FIN_001;item;board;425406371"
        );
    }

    #[test]
    fn extracts_profile_id_for_matching_numeric_post_id() {
        // 실측: id는 숫자, writer.profileId는 문자열.
        let body = json!({
            "posts": [
                { "id": 111, "writer": { "profileId": "wrong" } },
                { "id": 425406371, "writer": { "profileId": "profile-abc" } }
            ]
        });
        assert_eq!(
            extract_profile_id(&body, "425406371"),
            Some("profile-abc".to_owned())
        );
    }

    #[test]
    fn extracts_profile_id_returns_none_when_absent() {
        let body = json!({ "posts": [ { "id": "1", "writer": { "profileId": "x" } } ] });
        assert_eq!(extract_profile_id(&body, "425406371"), None);
    }

    #[test]
    fn extracts_encrypted_user_id_top_level_and_wrapped() {
        let flat = json!({ "encryptedUserId": "vAOdyIfnpaqA=" });
        assert_eq!(
            extract_encrypted_user_id(&flat),
            Some("vAOdyIfnpaqA=".to_owned())
        );
        let wrapped = json!({ "result": { "encryptedUserId": "enc-123" } });
        assert_eq!(
            extract_encrypted_user_id(&wrapped),
            Some("enc-123".to_owned())
        );
        let missing = json!({ "nickname": "x" });
        assert_eq!(extract_encrypted_user_id(&missing), None);
    }
}
