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

use std::time::Duration;

use serde::Serialize;
use tokio::time::sleep;

use client::BandHttpClient;
use cookies::load_band_cookie_header;
use error::BandPostError;
use link::band_no_from_link;

/// 같은 글에 댓글을 연속으로 달 때 band.us의 연속요청/도배 차단으로 두 번째 이후가
/// 거부되는 것을 피하려고 댓글 사이에 두는 기본 간격(카페 `COMMENT_JOB_DELAY`와 동일).
const BAND_COMMENT_DELAY: Duration = Duration::from_millis(2000);

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
    /// 같은 글에 단 댓글 중 **성공한 개수**(0이면 미작성). best-effort라 일부 댓글이
    /// 실패해도 글 게시는 성공으로 남고 성공분만 센다.
    pub commented_count: usize,
    /// 시도한 댓글 수(비어있지 않은 댓글 풀의 크기). 프론트가 `commented_count`와
    /// 비교해 "N/M건"을 표시하고 부분 실패를 성공으로 묻지 않도록 한다(카페와 동일).
    pub comment_total: usize,
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
///
/// `comments`는 게시한 글에 다는 댓글 풀이다. 비어있지 않은 항목을 **모두 같은 글에**
/// 순서대로 단다(카페 both와 동일). 댓글 게시는 best-effort라 일부 실패해도 글 게시는
/// 성공으로 남는다.
pub async fn band_publish(
    account_id: &str,
    band_link: &str,
    title: &str,
    content: &str,
    comments: &[String],
) -> Result<BandPublishOutcome, BandPostError> {
    // 최종 게시 결과를 사람이 읽는 한 줄로 남긴다(로그인의 ✅/❌ 결과 로그와 동일
    // 형식). 내부 흐름과 기존 단계별 로그(게시 시작·getKey 등)는 그대로 두고,
    // 성공/실패 결과만 덧붙인다.
    match band_publish_inner(account_id, band_link, title, content, comments).await {
        Ok(outcome) => {
            tracing::info!(
                "{}",
                publish_success_log(
                    account_id,
                    outcome.band_name.as_deref(),
                    outcome.post_no,
                    outcome.commented_count,
                    outcome.comment_total,
                )
            );
            Ok(outcome)
        }
        Err(e) => {
            tracing::warn!("[BAND] ❌ 게시 실패 — 계정 {account_id} ({e})");
            Err(e)
        }
    }
}

/// 게시 성공 로그 문구를 만든다(순수 함수, 테스트 가능). 댓글 성공/시도 개수와 밴드명을
/// 반영한다(밴드명이 없으면 "밴드"로 대체). 시도한 댓글이 있으면 "글+댓글 N/M개"로
/// 부분 실패까지 드러낸다.
fn publish_success_log(
    account_id: &str,
    band_name: Option<&str>,
    post_no: u64,
    comment_count: usize,
    comment_total: usize,
) -> String {
    let what = if comment_total > 0 {
        format!("글+댓글 {comment_count}/{comment_total}개")
    } else {
        "글".to_owned()
    };
    let band = band_name.unwrap_or("밴드");
    format!("[BAND] ✅ \"{band}\" {what} 게시 성공 — 계정 {account_id}, post_no {post_no}")
}

async fn band_publish_inner(
    account_id: &str,
    band_link: &str,
    title: &str,
    content: &str,
    comments: &[String],
) -> Result<BandPublishOutcome, BandPostError> {
    let band_no = band_no_from_link(band_link)
        .ok_or_else(|| BandPostError::InvalidLink(band_link.to_string()))?;

    let cookie_header = load_band_cookie_header(account_id)
        .map_err(|e| BandPostError::Transport(e.to_string()))?
        .ok_or(BandPostError::NoSession)?;

    let client = BandHttpClient::new();
    tracing::info!("[BAND] 게시 시작 — 계정 {account_id}, band_no {band_no}");
    let key = client.fetch_secret_key(&cookie_header).await?;
    tracing::info!("[BAND] getKey 서명키 발급 성공 — 게시 진행");

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

    // 비어있지 않은 댓글을 모두 같은 글(post_no)에 순서대로 단다(카페 both와 동일).
    // best-effort: 한 댓글 실패가 글 게시나 다른 댓글을 막지 않고, 성공한 개수만 센다.
    // 같은 글 연속 댓글은 도배 차단을 부르므로 첫 댓글 이후 간격을 둔다(카페와 동일).
    let targets: Vec<&String> = comments.iter().filter(|c| !c.trim().is_empty()).collect();
    let comment_total = targets.len();
    let mut commented_count = 0usize;
    for (index, body) in targets.iter().enumerate() {
        if index > 0 {
            sleep(BAND_COMMENT_DELAY).await;
        }
        match client
            .create_comment(&band_no, post_no, body, &key, &cookie_header)
            .await
        {
            Ok(()) => commented_count += 1,
            Err(error) => tracing::warn!(
                "[BAND] 댓글 게시 실패 — 계정 {account_id}, post_no {post_no} ({error})"
            ),
        }
    }

    Ok(BandPublishOutcome {
        joined,
        post_no,
        web_url: format!("https://band.us/band/{band_no}/post/{post_no}"),
        commented_count,
        comment_total,
        band_name: created.band_name,
    })
}

/// 링크(band_no)로 밴드 이름을 조회한다(게시 전 저장 시점에 실제 밴드명 확인용).
///
/// 저장된 band 로그인 쿠키 → getKey → `get_band_information`. 응답에 이름이 없으면
/// `band_no`를 그대로 돌려준다(프론트가 항상 무언가 표시하도록).
pub async fn resolve_band_name(account_id: &str, band_link: &str) -> Result<String, BandPostError> {
    let band_no = band_no_from_link(band_link)
        .ok_or_else(|| BandPostError::InvalidLink(band_link.to_string()))?;

    let cookie_header = load_band_cookie_header(account_id)
        .map_err(|e| BandPostError::Transport(e.to_string()))?
        .ok_or(BandPostError::NoSession)?;

    let client = BandHttpClient::new();
    let key = client.fetch_secret_key(&cookie_header).await?;
    let name = client.get_band_name(&band_no, &key, &cookie_header).await?;
    Ok(name.unwrap_or(band_no))
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

    #[test]
    fn success_log_mentions_band_post_no_and_comment_count() {
        let s = publish_success_log("cho****", Some("데일밴드"), 42, 3, 3);
        assert!(s.contains("[BAND] ✅"), "{s}");
        assert!(s.contains("데일밴드"), "{s}");
        // 댓글 3개 모두 성공 → "글+댓글 3/3개".
        assert!(s.contains("글+댓글 3/3개"), "{s}");
        assert!(s.contains("post_no 42"), "{s}");
        assert!(s.contains("cho****"), "{s}");
    }

    #[test]
    fn success_log_shows_partial_comment_failure_as_n_over_m() {
        // 댓글 3개 중 1개만 성공 → "글+댓글 1/3개"로 부분 실패를 드러낸다.
        let s = publish_success_log("acc", Some("데일밴드"), 9, 1, 3);
        assert!(s.contains("글+댓글 1/3개"), "{s}");
    }

    #[test]
    fn success_log_without_comment_falls_back_to_band_label() {
        let s = publish_success_log("acc", None, 7, 0, 0);
        // 시도 댓글 0개 → "글"만(글+댓글 아님), 밴드명 없음 → "밴드" 대체.
        assert!(s.contains("\"밴드\" 글 게시 성공"), "{s}");
        assert!(!s.contains("글+댓글"), "{s}");
        assert!(s.contains("post_no 7"), "{s}");
    }
}
