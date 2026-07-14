//! 네이버 블로그 댓글 게시(#271). 블로그는 **댓글 전용**이며, 별도 로그인 없이 네이버 카페와
//! 동일한 저장 쿠키(cookies/{loginId}.json)를 그대로 재사용한다. 카페/밴드 로직은 건드리지 않고
//! 블로그 경로만 추가한다(ADD ONLY).

pub mod comment_client;
pub mod domain_client;
pub mod error;
pub(crate) mod headers;
pub mod post_list;
pub mod write_client;

pub use comment_client::{BlogCommentClient, BlogCommentResult};
pub use domain_client::BlogDomainClient;
pub use error::BlogError;
pub use post_list::{BlogPost, BlogPostList, BlogPostListClient};
pub use write_client::{
    BlogPublishSettings, BlogWriteClient, BlogWriteResult, OpenType, PublishTime,
};

/// 저장된 네이버 쿠키로 블로그 글에 댓글 1건을 등록한다(계정 단위 진입점, 큐 워커용).
///
/// `account_id`(= loginId)로 저장 쿠키(cookies/{loginId}.json)를 읽어 카페 댓글과 동일하게
/// Cookie 헤더를 만든 뒤, [`BlogCommentClient::create_comment`]로 3단계를 수행한다. 쿠키가
/// 없거나 만료됐으면(재로그인 필요) `BlogError`로 알린다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn create_blog_comment_for_account(
    account_id: &str,
    blog_id: &str,
    log_no: &str,
    contents: &str,
) -> Result<BlogCommentResult, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = BlogCommentClient::new();
    client
        .create_comment(blog_id, log_no, contents, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 한 블로그의 최신 글 `count`개를 조회한다(#279, "최신 N개" 모드).
///
/// `account_id`(= loginId)의 저장 쿠키를 댓글 경로와 동일하게 읽어 [`BlogPostListClient`]에
/// 넘긴다. `category_no`는 카테고리(전체=0). 모은 글(최신 우선)과 카테고리 전체 글 수를
/// 돌려줘, 호출부가 "글이 모자라면 그만큼 실패로 남기는" 처리를 할 수 있게 한다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_latest_blog_posts_for_account(
    account_id: &str,
    blog_id: &str,
    category_no: u32,
    count: usize,
) -> Result<BlogPostList, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    let client = BlogPostListClient::new();
    client
        .fetch_latest_posts(blog_id, category_no, count, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 블로그명(도메인) 사용 가능 여부를 조회한다(계정 단위 진입점).
///
/// `true`=사용 가능, `false`=이미 사용 중. 봇탐지 토큰이 필요 없는 단순 조회라 CDP 없이 HTTP로
/// 처리한다(블로그 생성·글 발행은 별도 CDP 경로). 쿠키가 없으면 `BlogError`로 알린다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn check_blog_name_for_account(
    account_id: &str,
    domain_id: &str,
) -> Result<bool, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    BlogDomainClient::new()
        .check_availability(domain_id, Some(&cookie_header))
        .await
}

/// 저장된 네이버 쿠키로 블로그 새 글을 발행한다(계정 단위 진입점, HTTP RabbitWrite 경로).
///
/// `blog_id`(그 계정의 블로그명)에 제목/내용/발행설정으로 새 글을 올린다. 봇탐지 tokenId는
/// 클라이언트가 생성해 실측하며(서버가 강하게 검증하면 CDP 경로로 대체), 성공하면 게시글
/// 번호(logNo)와 링크를 돌려준다. 쿠키가 없으면 `BlogError`로 알린다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn publish_blog_post_for_account(
    account_id: &str,
    blog_id: &str,
    title: &str,
    content: &str,
    settings: &BlogPublishSettings,
) -> Result<BlogWriteResult, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    BlogWriteClient::new()
        .publish(blog_id, title, content, settings, &cookie_header)
        .await
}

/// 저장된 네이버 쿠키로 대체 블로그명 추천 목록을 조회한다(사용 중일 때 UI 제안용, best-effort).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn recommend_blog_names_for_account(
    account_id: &str,
    domain_id: &str,
) -> Result<Vec<String>, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    BlogDomainClient::new()
        .recommend(domain_id, Some(&cookie_header))
        .await
}

/// 계정의 저장 세션 쿠키를 Cookie 헤더 문자열로 해석한다(카페 article_list와 동일 규약).
/// 쿠키 값은 반환 오류/로그에 절대 노출되지 않는다.
fn resolve_cookie_header(account_id: &str) -> Result<String, BlogError> {
    let cookie_value = match crate::auth::read_account_cookies(account_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return Err(BlogError::new(format!(
                "계정 '{account_id}'의 세션 쿠키가 없거나 만료되었습니다. 다시 로그인하세요."
            )))
        }
        Err(e) => {
            return Err(BlogError::new(format!(
                "계정 '{account_id}'의 쿠키를 읽지 못했습니다: {e}"
            )))
        }
    };
    crate::naver_cafe::post::cookie_header_from_storage_state(&cookie_value).ok_or_else(|| {
        BlogError::new(format!(
            "계정 '{account_id}'의 네이버 세션 쿠키를 찾지 못했습니다. 다시 로그인하세요."
        ))
    })
}
