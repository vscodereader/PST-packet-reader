//! 네이버 블로그 댓글 게시(#271). 블로그는 **댓글 전용**이며, 별도 로그인 없이 네이버 카페와
//! 동일한 저장 쿠키(cookies/{loginId}.json)를 그대로 재사용한다. 카페/밴드 로직은 건드리지 않고
//! 블로그 경로만 추가한다(ADD ONLY).

pub mod comment_client;
pub mod document_model;
pub mod domain_client;
pub mod editor_api;
pub mod error;
pub(crate) mod headers;
pub mod post_list;
pub mod write_client;

pub use comment_client::{BlogCommentClient, BlogCommentResult};
pub use document_model::Block;
pub use domain_client::BlogDomainClient;
// EnsureBlogResult는 이 모듈에서 정의(발행 전 블로그 존재확인/자동생성 결과).
pub use editor_api::{
    BlogEditorApiClient, OglinkMeta, PlaceResult, StaticMapResult, StickerPack, UploadedFile,
    UploadedImage,
};
pub use error::BlogError;
pub use post_list::{BlogPost, BlogPostList, BlogPostListClient};
pub use write_client::{
    build_document_model_with_components, BlogPublishSettings, BlogWriteClient, BlogWriteResult,
    OpenType, PublishTime,
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

/// 블로그 존재 보장 결과. `existed`=이미 있었음, `created`=이번 호출로 자동 생성함.
/// 발행 전 사전확인(프론트 표시)과 발행 흐름 내부 배선에서 공유한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureBlogResult {
    pub existed: bool,
    pub created: bool,
}

/// 블로그 존재를 보장한다: 있으면 그대로, 없으면 자동 생성한다(발행 전 선행 단계).
///
/// SeOptions(`fetch_editor_session`)로 세션 토큰을 받으면 블로그가 있는 것으로 본다(존재확인 겸용).
/// 토큰이 없으면(블로그 없음/로그인 만료 등) 지시대로 "블로그 없음"으로 해석하고
/// `BlogDomainRegistration`으로 블로그를 자동 생성한다(`domainId=naverId=blog_id`, 계정 기본값).
/// 각 단계 원문 로그는 하위 클라이언트(`fetch_editor_session`/`register`)가 남긴다.
///
/// # 쿠키 보안
/// `cookie`는 사용자 인증 자격 증명이며 에러/로그에 노출하지 않는다.
async fn ensure_blog_exists_with_cookie(
    blog_id: &str,
    naver_id: &str,
    cookie: &str,
) -> Result<EnsureBlogResult, BlogError> {
    match editor_api::fetch_editor_session(blog_id, Some(cookie)).await {
        Ok(_) => Ok(EnsureBlogResult {
            existed: true,
            created: false,
        }),
        Err(_) => {
            // 토큰 없음 = 블로그 없음으로 해석하고 자동 생성한다(생성 실패는 에러로 전파).
            // 실측 확정: domainId=새 블로그명, naverId=**로그인 계정 아이디**(서로 다름).
            BlogDomainClient::new()
                .register(blog_id, naver_id, Some(cookie))
                .await?;
            Ok(EnsureBlogResult {
                existed: false,
                created: true,
            })
        }
    }
}

/// 저장된 네이버 쿠키로 계정에 블로그가 있는지 확인하고, 없으면 자동 생성한다(계정 단위 진입점).
///
/// 프론트가 발행 전에 사전확인/결과표시할 수 있게 존재/생성 여부를 돌려준다. `blog_id`는 계정
/// 기본값(loginId)을 넘긴다. 쿠키가 없으면 `BlogError`로 알린다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn ensure_blog_exists_for_account(
    account_id: &str,
    blog_id: &str,
) -> Result<EnsureBlogResult, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    // naverId = 로그인 계정(account_id), domainId = 만들 블로그명(blog_id).
    ensure_blog_exists_with_cookie(blog_id, account_id, &cookie_header).await
}

/// 저장된 네이버 쿠키로 블로그 새 글을 발행한다(계정 단위 진입점, HTTP RabbitWrite 경로).
///
/// `blog_id`(그 계정의 블로그명)에 제목/내용/발행설정으로 새 글을 올린다. 발행 전에 SeOptions로
/// 블로그 존재를 확인하고, 없으면 자동 생성한다(존재확인→없으면 생성→게시). 봇탐지 tokenId는
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
    // 발행 검증 토큰(editorSource) + 세션쿠키 보강(위 블록 발행과 동일 이유 — 없으면 비공개 강제).
    let (enriched_cookie, editor_source) =
        editor_api::prepare_publish_session(blog_id, settings.category_id, Some(&cookie_header))
            .await
            .unwrap_or_else(|_| (cookie_header.clone(), None));
    let mut settings = settings.clone();
    settings.editor_source = editor_source;
    BlogWriteClient::new()
        .publish(blog_id, title, content, &settings, &enriched_cookie)
        .await
}

/// 저장된 네이버 쿠키로 툴바 블록(텍스트/서식/삽입)으로 만든 새 글을 발행한다(HTTP RabbitWrite).
///
/// 네이버 편집기와 동일한 documentModel을 만들기 위해, 프론트가 보낸 블록 배열을
/// [`document_model::blocks_to_components`]로 `components[]`에 옮기고 제목과 합쳐 발행한다.
/// 사진/파일/링크/스티커/장소 블록은 프론트가 보조 API로 이미 해석한 데이터를 담고 온다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn publish_blog_post_blocks_for_account(
    account_id: &str,
    blog_id: &str,
    title: &str,
    blocks: &[Block],
    settings: &BlogPublishSettings,
) -> Result<BlogWriteResult, BlogError> {
    let cookie_header = resolve_cookie_header(account_id)?;
    // 발행 검증 토큰(editorSource) + 세션쿠키 보강을 발행 직전에 받는다. editorSource가 없으면
    // 네이버가 공개범위(openType)를 무시하고 **비공개로 강제**하므로 반드시 실어야 한다(실측 2026-07-14).
    let (enriched_cookie, editor_source) =
        editor_api::prepare_publish_session(blog_id, settings.category_id, Some(&cookie_header))
            .await
            .unwrap_or_else(|_| (cookie_header.clone(), None));
    let mut settings = settings.clone();
    settings.editor_source = editor_source;
    let components = document_model::blocks_to_components(blocks);
    let document_model = build_document_model_with_components(title, components);
    BlogWriteClient::new()
        .publish_document(blog_id, document_model, &settings, &enriched_cookie)
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

/// 편집기 보조 API 클라이언트를 세션(se-authorization/se-app-id)까지 실어 준비한다.
///
/// `platform.editor.naver.com` API는 쿠키만으론 401("the token must not be empty")이라, 먼저
/// `PostWriteFormSeOptions.naver`로 세션 토큰을 발급받아 클라이언트에 싣는다(#블로그 편집기 401 수정).
///
/// 세션 발급은 그 계정 **자기 블로그**의 글쓰기 폼을 여는 것이므로 `blogId`가 필요하다. 이 편집기
/// 보조 커맨드들은 프론트에서 `blogId`를 따로 안 넘겨(account만) 온다 — 네이버 기본값대로
/// `blogId == loginId(account_id)`로 발급한다(대부분의 계정이 그렇다). 커스텀 blogId 계정에서 실패가
/// 나오면 발급 응답 원문 로그로 확인해 blogId를 넘기도록 확장한다.
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
async fn editor_client_for_account(
    account_id: &str,
) -> Result<(BlogEditorApiClient, String), BlogError> {
    let cookie = resolve_cookie_header(account_id)?;
    // 글쓰기 폼 워밍업으로 세션쿠키(JSESSIONID/BUC)까지 보강한 쿠키를 받아, 이후 편집기/업로드
    // 호출에 그대로 쓴다(그래야 platform.editor.naver.com 이 401 없이 통과한다).
    let (session, enriched) =
        editor_api::establish_editor_session(account_id, Some(&cookie)).await?;
    Ok((BlogEditorApiClient::new().with_session(session), enriched))
}

/// 저장된 네이버 쿠키로 링크(oglink) 메타데이터를 조회한다(링크 블록 삽입용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_oglink_for_account(
    account_id: &str,
    url: &str,
) -> Result<OglinkMeta, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    client.oglink(url, Some(&cookie)).await
}

/// 저장된 네이버 쿠키로 장소를 검색한다(장소 블록 삽입용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn search_places_for_account(
    account_id: &str,
    query: &str,
) -> Result<Vec<PlaceResult>, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    client.places(query, Some(&cookie)).await
}

/// 저장된 네이버 쿠키로 장소 좌표의 정적 지도 URL을 얻는다(장소 블록 썸네일용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_staticmap_for_account(
    account_id: &str,
    latitude: &str,
    longitude: &str,
) -> Result<StaticMapResult, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    client.staticmap(latitude, longitude, Some(&cookie)).await
}

/// 저장된 네이버 쿠키로 스티커 팩 목록을 조회한다(스티커 블록 삽입용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_sticker_packs_for_account(
    account_id: &str,
) -> Result<Vec<StickerPack>, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    client.stickers(Some(&cookie)).await
}

/// 저장된 네이버 쿠키로 한 스티커 팩의 seq 목록을 조회한다(스티커 블록 삽입용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn fetch_sticker_seqs_for_account(
    account_id: &str,
    pack_code: &str,
) -> Result<Vec<u32>, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    client.sticker_pack_seqs(pack_code, Some(&cookie)).await
}

/// 저장된 네이버 쿠키로 로컬 파일을 업로드하고 fileId 등을 얻는다(파일 블록 삽입용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn upload_blog_file_for_account(
    account_id: &str,
    file_path: &str,
) -> Result<UploadedFile, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    let (file_name, bytes) = read_local_file(file_path)?;
    // userId = 블로그 계정(=account_id). 실측 파트 `userId`+`file`.
    client
        .upload_file(account_id, &file_name, bytes, Some(&cookie))
        .await
}

/// 저장된 네이버 쿠키로 로컬 이미지를 업로드하고 image 컴포넌트에 필요한 값을 얻는다(사진 블록용).
///
/// # 쿠키 보안
/// 계정 쿠키는 내부에서만 사용되며 반환 오류/로그에 절대 노출되지 않는다.
pub async fn upload_blog_photo_for_account(
    account_id: &str,
    file_path: &str,
) -> Result<UploadedImage, BlogError> {
    let (client, cookie) = editor_client_for_account(account_id).await?;
    let (file_name, bytes) = read_local_file(file_path)?;
    // 실측(photo.pcapng): 본문 사진은 2단계 — 1) photo-uploader 세션키 발급,
    // 2) upphoto simpleUpload(sessionKey URL 인증). `upload_file`(파일)과 다른 경로다.
    let session_key = client.photo_session_key(Some(&cookie)).await?;
    client
        .upload_photo(account_id, &session_key, &file_name, bytes, Some(&cookie))
        .await
}

/// 로컬 파일을 읽어 (파일명, 바이트)로 돌려준다. 경로/IO 오류는 사용자 메시지로 감싼다.
fn read_local_file(file_path: &str) -> Result<(String, Vec<u8>), BlogError> {
    let path = std::path::Path::new(file_path);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| BlogError::new(format!("파일 경로가 올바르지 않습니다: {file_path}")))?;
    let bytes = std::fs::read(path)
        .map_err(|e| BlogError::new(format!("파일을 읽지 못했습니다: {e}")))?;
    Ok((file_name, bytes))
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
