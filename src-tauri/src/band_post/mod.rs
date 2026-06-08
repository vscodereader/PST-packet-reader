//! band.us 가입·글쓰기·댓글을 순수 HTTP로 수행하는 모듈(사수 지시: CDP 폐기, 패킷분석).
//!
//! 로그인은 기존 [`band_auth`](crate::band_auth)(CDP)로 쿠키를 확보하고, 이 모듈은
//! 그 쿠키로 `api-kr.band.us`에 직접 요청한다. 모든 요청은 `md` 서명([`signature`])이
//! 필요하며, 서명 키(`secretKey`)는 [`getkey`]로 받아온다.
//!
//! 네이버 카페 모듈([`naver_cafe`](crate::naver_cafe)) 구조를 미러한다:
//! 순수 빌더([`request_builder`]) + HTTP 클라이언트([`client`]) 분리.

pub mod client;
pub mod cookies;
pub mod error;
pub mod getkey;
pub mod link;
pub mod request_builder;
pub mod response;
pub mod signature;
pub mod util;

use serde::Serialize;

use client::BandHttpClient;
use cookies::load_band_cookie_header;
use error::BandPostError;
use link::band_no_from_link;

/// band 게시 결과(프론트로 반환).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandPublishOutcome {
    /// 가입 시도가 성공했는지(이미 가입된 경우 false일 수 있으나 게시는 진행).
    pub joined: bool,
    /// 생성된 게시물 번호.
    pub post_no: u64,
    /// 게시물 web URL.
    pub web_url: String,
    /// 댓글까지 작성했는지.
    pub commented: bool,
    /// 실제 게시된 밴드 이름(게시 응답 `post.band.name`). 응답에 없으면 `None`.
    pub band_name: Option<String>,
}

/// 밴드 링크로 가입한 뒤 글(+선택 댓글)을 게시한다.
///
/// 흐름(사용자 시나리오): 링크 → `band_no` 추출 → 저장된 로그인 쿠키 로드 →
/// getKey로 서명키 발급 → 가입(`join_band`) → 글 게시(`create_post`) →
/// (댓글이 있으면) 댓글(`create_comment`).
///
/// 가입은 best-effort다(이미 가입된 밴드면 실패할 수 있으나 게시를 막지 않는다).
/// 게시 권한이 없으면 글 게시 단계에서 실제 오류가 표면화된다.
///
/// `title`이 비어 있지 않으면 본문 앞에 제목 줄을 붙여 하나의 `content`로 게시한다
/// (밴드 글은 제목/본문이 분리되지 않은 단일 본문 구조).
pub async fn band_publish(
    account_id: &str,
    band_link: &str,
    title: &str,
    content: &str,
    comment: Option<&str>,
) -> Result<BandPublishOutcome, BandPostError> {
    let band_no = band_no_from_link(band_link)
        .ok_or_else(|| BandPostError::InvalidLink(band_link.to_string()))?;

    let cookie_header = load_band_cookie_header(account_id)
        .map_err(|e| BandPostError::Transport(e.to_string()))?
        .ok_or(BandPostError::NoSession)?;

    let client = BandHttpClient::new();
    let key = client.fetch_secret_key(&cookie_header).await?;

    // 가입(best-effort). 이미 가입돼 있으면 오류일 수 있으나 게시를 시도한다.
    let joined = client
        .join_band(&band_no, &key, &cookie_header)
        .await
        .is_ok();

    let post_content = combine_title_and_content(title, content);
    let created = client
        .create_post(&band_no, &post_content, &key, &cookie_header)
        .await?;
    let post_no = created.post_no;

    let commented = match comment {
        Some(body) if !body.trim().is_empty() => {
            client
                .create_comment(&band_no, post_no, body, &key, &cookie_header)
                .await?;
            true
        }
        _ => false,
    };

    Ok(BandPublishOutcome {
        joined,
        post_no,
        web_url: format!("https://band.us/band/{band_no}/post/{post_no}"),
        commented,
        band_name: created.band_name,
    })
}

/// 제목과 본문을 밴드 단일 `content`로 합친다.
///
/// 제목이 비어 있으면 본문만, 아니면 `"{제목}\n{본문}"`.
fn combine_title_and_content(title: &str, content: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        content.to_string()
    } else if content.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{content}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_prepends_title_line() {
        assert_eq!(combine_title_and_content("제목", "본문"), "제목\n본문");
    }

    #[test]
    fn combine_body_only_when_no_title() {
        assert_eq!(combine_title_and_content("  ", "본문"), "본문");
    }

    #[test]
    fn combine_title_only_when_no_body() {
        assert_eq!(combine_title_and_content("제목", ""), "제목");
    }
}
