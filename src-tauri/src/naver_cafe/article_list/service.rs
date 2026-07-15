//! 게시글 목록 조회 서비스 — 계정 쿠키 해석 + 클라이언트 호출.
//!
//! IPC 핸들러([`crate::ipc::cafes`])가 호출하는 진입점이다. 계정의 저장된
//! 세션 쿠키를 읽어 [`ArticleListClient`]에 넘긴다. 쿠키 값은 에러/로그에
//! 절대 노출되지 않는다.

use crate::auth::read_account_cookies;
use crate::naver_cafe::article_list::client::ArticleListClient;
use crate::naver_cafe::article_list::models::{
    Article, ArticleListError, ArticleListResponse, SortBy,
};
use crate::naver_cafe::error::{ErrorEnvelope, NaverCafeCommonErrorData};
use crate::naver_cafe::post::cookie_header_from_storage_state;

/// 쿠키 없음/읽기 실패 오류 코드.
pub const CODE_NO_COOKIES: &str = "NO_COOKIES";

/// 계정의 세션 쿠키를 읽지 못했을 때의 오류 봉투. 쿠키 값은 포함되지 않는다.
fn no_cookies_error(account_id: &str, detail: Option<String>) -> ArticleListError {
    let message = match detail {
        Some(d) => format!("계정 '{}'의 쿠키를 읽지 못했습니다: {}", account_id, d),
        None => format!(
            "계정 '{}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요.",
            account_id
        ),
    };
    ErrorEnvelope {
        trace_id: String::new(),
        code: CODE_NO_COOKIES.to_string(),
        message,
        error_data: Some(NaverCafeCommonErrorData {
            target: None,
            http_status: None,
            api_error_code: None,
            api_error_message: None,
            retryable: false,
        }),
    }
}

/// 계정의 세션 쿠키를 Cookie 헤더 문자열로 해석한다. 쿠키 값은 반환 오류/로그에
/// 절대 노출되지 않는다.
fn resolve_cookie_header(account_id: &str) -> Result<String, ArticleListError> {
    let cookie_value = match read_account_cookies(account_id) {
        Ok(Some(value)) => value,
        Ok(None) => return Err(no_cookies_error(account_id, None)),
        Err(e) => return Err(no_cookies_error(account_id, Some(e.to_string()))),
    };
    cookie_header_from_storage_state(&cookie_value)
        .ok_or_else(|| no_cookies_error(account_id, None))
}

/// `account_id`의 세션 쿠키로 카페 게시글 목록을 정렬 기준·페이지에 따라 조회한다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환되는 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_article_list_for_account(
    cafe_id: &str,
    menu_id: u32,
    sort_by: SortBy,
    page: u32,
    account_id: &str,
) -> Result<ArticleListResponse, ArticleListError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ArticleListClient::new();
    client
        .fetch_article_list(cafe_id, menu_id, sort_by, page, Some(cookie_header.as_str()))
        .await
}

/// `account_id`의 세션 쿠키로 최신글을 `want`개 모일 때까지 페이지를 이어 조회한다.
/// 페이징 정책은 [`ArticleListClient::fetch_latest_up_to`] 참고.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환되는 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_latest_articles_for_account_up_to(
    cafe_id: &str,
    menu_id: u32,
    account_id: &str,
    want: usize,
) -> Result<Vec<Article>, ArticleListError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = ArticleListClient::new();
    client
        .fetch_latest_up_to(cafe_id, menu_id, want, Some(cookie_header.as_str()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fetch_without_session_returns_no_cookies() {
        // 존재하지 않는 계정 → 쿠키 없음/읽기 실패 → NO_COOKIES (네트워크 미발생).
        let err = fetch_article_list_for_account(
            "no-such-account-xyz",
            0,
            SortBy::Latest,
            1,
            "no-such-account-xyz",
        )
        .await
        .expect_err("쿠키 없는 계정은 Err여야 함");
        assert_eq!(err.code, CODE_NO_COOKIES);
        // 쿠키/세션 값이 오류 메시지에 노출되지 않아야 한다.
        assert!(!err.message.contains("NID_AUT"));
    }
}
