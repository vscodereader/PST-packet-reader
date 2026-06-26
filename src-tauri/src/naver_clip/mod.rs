//! 네이버 클립 댓글 게시(#클립). 클립은 **댓글 전용**이며, 별도 로그인 없이 네이버 카페/블로그와
//! 동일한 저장 쿠키(cookies/{loginId}.json)를 그대로 재사용한다. 단 클립은 댓글 전 **프로필 생성**
//! (네이버 로그인만으론 부족)이 1회 필요해, 게시 직전 계정마다 [`ensure_clip_profile_for_account`]로
//! 보장한다. 카페/블로그/밴드 로직은 건드리지 않고 클립 경로만 추가한다(ADD ONLY).

pub mod clip_list;
pub mod comment_client;
pub mod error;
pub(crate) mod headers;
pub mod profile;

pub use clip_list::{ClipListClient, ClipMedia, ClipMediaType};
pub use comment_client::{ClipCommentClient, ClipCommentResult};
pub use error::ClipError;
pub use profile::ClipProfileClient;

/// 저장된 네이버 쿠키로 클립 미디어에 댓글 1건을 등록한다(계정 단위 진입점, 큐 워커용).
///
/// `object_id`는 미디어 HEX id(cbox objectId), `profile_id`는 그 미디어를 올린 창작자 profileId
/// (Referer/objectUrl 구성용). 쿠키가 없거나 만료됐으면 `ClipError`로 알린다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn create_clip_comment_for_account(
    account_id: &str,
    object_id: &str,
    profile_id: &str,
    contents: &str,
) -> Result<ClipCommentResult, ClipError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ClipCommentClient::new();
    client
        .create_comment(object_id, profile_id, contents, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 한 창작자(profileId)의 최신 미디어 `count`개를 조회한다("최신 N개" 모드).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_latest_clips_for_account(
    account_id: &str,
    profile_id: &str,
    media_type: ClipMediaType,
    count: usize,
) -> Result<Vec<ClipMedia>, ClipError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ClipListClient::new();
    client
        .fetch_latest_clips(profile_id, media_type, count, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 창작자 핸들(`@handle`)을 profileId로 해석한다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn resolve_clip_profile_id_for_account(
    account_id: &str,
    handle: &str,
) -> Result<String, ClipError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ClipListClient::new();
    client
        .resolve_profile_id(handle, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 계정의 클립 댓글 프로필을 보장한다(없으면 생성). 게시 직전 1회 호출.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn ensure_clip_profile_for_account(account_id: &str) -> Result<(), ClipError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ClipProfileClient::new();
    client.ensure_profile(Some(&cookie_header)).await
}

/// 계정의 저장 세션 쿠키를 Cookie 헤더 문자열로 해석한다(블로그 resolve_cookie_header와 동일 규약).
/// 쿠키 값은 반환 오류/로그에 절대 노출되지 않는다.
fn resolve_cookie_header(account_id: &str) -> Result<String, ClipError> {
    let cookie_value = match crate::auth::read_account_cookies(account_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Err(ClipError::new(format!(
                "계정 '{account_id}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요."
            )))
        }
        Err(e) => {
            return Err(ClipError::new(format!(
                "계정 '{account_id}'의 쿠키를 읽지 못했습니다: {e}"
            )))
        }
    };
    crate::naver_cafe::post::cookie_header_from_storage_state(&cookie_value).ok_or_else(|| {
        ClipError::new(format!(
            "계정 '{account_id}'의 네이버 세션 쿠키를 찾지 못했습니다. 다시 로그인하세요."
        ))
    })
}
