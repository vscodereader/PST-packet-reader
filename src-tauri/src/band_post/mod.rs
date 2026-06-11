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
// 카페 comment-only와 동일한 댓글 분배(셔플 후 1개씩 라운드로빈)를 재사용한다.
use crate::naver_cafe::distribute::{distribute_comments, mulberry32, seed_from_clock};

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

/// 밴드 댓글 전용 게시 결과(프론트로 반환). 기존 글(최신글/인기글)에 댓글을 단 결과.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandCommentOutcome {
    /// 댓글 대상으로 조회된 글 수(상위 N개).
    pub target_count: usize,
    /// 실제로 게시에 성공한 댓글 개수(best-effort, 글마다 1개씩 분배).
    pub commented_count: usize,
    /// 실제 밴드 이름(`get_band_information`). 없으면 `None`.
    pub band_name: Option<String>,
}

/// 밴드 댓글 전용 모드의 대상 글 정렬 기준.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandFeedSort {
    /// 최신글(`get_posts_and_announcements`, created_at_desc).
    Latest,
    /// 인기글(`get_popular_posts`, 공감·댓글 기반).
    Popular,
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
    // 최종 게시 결과를 사람이 읽는 한 줄로 남긴다(로그인의 ✅/❌ 결과 로그와 동일
    // 형식). 내부 흐름과 기존 단계별 로그(게시 시작·getKey 등)는 그대로 두고,
    // 성공/실패 결과만 덧붙인다.
    match band_publish_inner(account_id, band_link, title, content, comment).await {
        Ok(outcome) => {
            tracing::info!(
                "{}",
                publish_success_log(
                    account_id,
                    outcome.band_name.as_deref(),
                    outcome.post_no,
                    outcome.commented,
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

/// 게시 성공 로그 문구를 만든다(순수 함수, 테스트 가능). 댓글 작성 여부와 밴드명을
/// 반영한다(밴드명이 없으면 "밴드"로 대체).
fn publish_success_log(
    account_id: &str,
    band_name: Option<&str>,
    post_no: u64,
    commented: bool,
) -> String {
    let what = if commented { "글+댓글" } else { "글" };
    let band = band_name.unwrap_or("밴드");
    format!("[BAND] ✅ \"{band}\" {what} 게시 성공 — 계정 {account_id}, post_no {post_no}")
}

async fn band_publish_inner(
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

/// 댓글 전용 모드에서 글 사이에 두는 기본 간격. 같은 계정이 여러 글에 연속으로 댓글을
/// 달 때 밴드의 연속요청/도배 차단을 피하려는 것으로, 카페 `COMMENT_JOB_DELAY`와 같다.
const BAND_COMMENT_DELAY: Duration = Duration::from_millis(2000);

/// 밴드의 기존 글(최신글/인기글) 상위 N개를 조회해 댓글을 단다(댓글 전용 모드).
///
/// 흐름: 링크 → `band_no` → 쿠키/서명키 → 대상 글(post_no) 상위 N개 조회(sort) →
/// 댓글 풀을 글 수만큼 분배(글마다 1개) → 각 글에 `create_comment`(best-effort). 카페
/// comment-only(latest/popular)를 밴드에 미러한 것으로, 새 글은 만들지 않는다.
pub async fn band_comment(
    account_id: &str,
    band_link: &str,
    sort: BandFeedSort,
    count: u32,
    comments: &[String],
) -> Result<BandCommentOutcome, BandPostError> {
    match band_comment_inner(account_id, band_link, sort, count, comments).await {
        Ok(outcome) => {
            // 한 건도 못 달았으면(대상 글 없음/전부 실패) 성공으로 보고하지 않고 경고로 남긴다.
            if outcome.commented_count > 0 {
                tracing::info!(
                    "{}",
                    comment_success_log(
                        account_id,
                        outcome.band_name.as_deref(),
                        outcome.target_count,
                        outcome.commented_count,
                    )
                );
            } else {
                tracing::warn!(
                    "{}",
                    comment_failure_log(
                        account_id,
                        outcome.band_name.as_deref(),
                        outcome.target_count,
                    )
                );
            }
            Ok(outcome)
        }
        Err(e) => {
            tracing::warn!("[BAND] ❌ 댓글 전용 게시 실패 — 계정 {account_id} ({e})");
            Err(e)
        }
    }
}

/// 댓글 전용 게시 성공 로그 문구(순수 함수, 테스트 가능).
fn comment_success_log(
    account_id: &str,
    band_name: Option<&str>,
    target_count: usize,
    commented_count: usize,
) -> String {
    let band = band_name.unwrap_or("밴드");
    format!(
        "[BAND] ✅ \"{band}\" 댓글 게시 성공 — 계정 {account_id}, 대상 {target_count}글 중 댓글 {commented_count}개"
    )
}

/// 댓글 전용 게시에서 한 건도 성공하지 못했을 때의 경고 문구(순수 함수, 테스트 가능).
/// 대상 글 자체가 없었는지(`target_count == 0`) 대상은 있었으나 전부 실패했는지를
/// 구분해, 로그만 보고도 원인(빈 피드 vs 차단/오류)을 좁힐 수 있게 한다.
fn comment_failure_log(account_id: &str, band_name: Option<&str>, target_count: usize) -> String {
    let band = band_name.unwrap_or("밴드");
    if target_count == 0 {
        format!("[BAND] ⚠️ \"{band}\" 댓글 대상 글 없음 — 계정 {account_id}")
    } else {
        format!(
            "[BAND] ⚠️ \"{band}\" 댓글 전부 실패 — 계정 {account_id}, 대상 {target_count}글 중 0개"
        )
    }
}

async fn band_comment_inner(
    account_id: &str,
    band_link: &str,
    sort: BandFeedSort,
    count: u32,
    comments: &[String],
) -> Result<BandCommentOutcome, BandPostError> {
    let band_no = band_no_from_link(band_link)
        .ok_or_else(|| BandPostError::InvalidLink(band_link.to_string()))?;

    let cookie_header = load_band_cookie_header(account_id)
        .map_err(|e| BandPostError::Transport(e.to_string()))?
        .ok_or(BandPostError::NoSession)?;

    let client = BandHttpClient::new();
    tracing::info!("[BAND] 댓글 전용 시작 — 계정 {account_id}, band_no {band_no}, sort {sort:?}");
    let key = client.fetch_secret_key(&cookie_header).await?;

    // 대상 글(post_no) 상위 N개 조회. 최신글은 limit=N 단일 호출, 인기글은 offset 누적.
    let n = count.max(1);
    let post_nos = match sort {
        BandFeedSort::Latest => {
            client
                .get_latest_posts(&band_no, n, &key, &cookie_header)
                .await?
        }
        BandFeedSort::Popular => {
            client
                .get_popular_posts(&band_no, n, &key, &cookie_header)
                .await?
        }
    };

    // 댓글 풀을 대상 글 수만큼 분배(글마다 1개, 카페 comment-only와 동일 규칙). 풀이 비면
    // 빈 목록 → 댓글 0건.
    let mut rng = mulberry32(seed_from_clock());
    let contents = distribute_comments(post_nos.len(), comments, &mut rng);

    // best-effort: 한 글 댓글 실패가 다른 글을 막지 않고 성공 개수만 센다. 같은 계정이
    // 여러 글에 연속으로 댓글을 달면 밴드의 연속요청/도배 차단으로 두 번째 이후가 거부될
    // 수 있어, 첫 댓글 이후에는 글 사이에 간격을 둔다(카페 COMMENT_JOB_DELAY 미러).
    let mut commented_count = 0usize;
    let mut attempted = 0usize;
    for (post_no, content) in post_nos.iter().zip(contents.iter()) {
        if content.trim().is_empty() {
            continue;
        }
        if attempted > 0 {
            sleep(BAND_COMMENT_DELAY).await;
        }
        attempted += 1;
        match client
            .create_comment(&band_no, *post_no, content, &key, &cookie_header)
            .await
        {
            Ok(()) => commented_count += 1,
            Err(error) => tracing::warn!(
                "[BAND] 댓글 게시 실패 — 계정 {account_id}, post_no {post_no} ({error})"
            ),
        }
    }

    // 결과 라벨용 실제 밴드명(조회 실패해도 게시는 성공이므로 best-effort).
    let band_name = client
        .get_band_name(&band_no, &key, &cookie_header)
        .await
        .ok()
        .flatten();

    Ok(BandCommentOutcome {
        target_count: post_nos.len(),
        commented_count,
        band_name,
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
    fn success_log_mentions_band_post_no_and_comment() {
        let s = publish_success_log("cho****", Some("데일밴드"), 42, true);
        assert!(s.contains("[BAND] ✅"), "{s}");
        assert!(s.contains("데일밴드"), "{s}");
        assert!(s.contains("글+댓글"), "{s}");
        assert!(s.contains("post_no 42"), "{s}");
        assert!(s.contains("cho****"), "{s}");
    }

    #[test]
    fn success_log_without_comment_falls_back_to_band_label() {
        let s = publish_success_log("acc", None, 7, false);
        // 댓글 없음 → "글"만(글+댓글 아님), 밴드명 없음 → "밴드" 대체.
        assert!(s.contains("\"밴드\" 글 게시 성공"), "{s}");
        assert!(!s.contains("글+댓글"), "{s}");
        assert!(s.contains("post_no 7"), "{s}");
    }

    #[test]
    fn comment_success_log_mentions_targets_and_count() {
        let s = comment_success_log("cho****", Some("데일밴드"), 3, 3);
        assert!(s.contains("[BAND] ✅"), "{s}");
        assert!(s.contains("데일밴드"), "{s}");
        assert!(s.contains("댓글 게시 성공"), "{s}");
        assert!(s.contains("대상 3글"), "{s}");
        assert!(s.contains("댓글 3개"), "{s}");
        assert!(s.contains("cho****"), "{s}");
    }

    #[test]
    fn comment_success_log_falls_back_to_band_label() {
        let s = comment_success_log("acc", None, 0, 0);
        assert!(s.contains("\"밴드\" 댓글 게시 성공"), "{s}");
    }

    #[test]
    fn comment_failure_log_distinguishes_empty_feed_from_all_failed() {
        // 대상 글 자체가 없을 때: "대상 글 없음".
        let none = comment_failure_log("cho****", Some("데일밴드"), 0);
        assert!(none.contains("[BAND] ⚠️"), "{none}");
        assert!(none.contains("데일밴드"), "{none}");
        assert!(none.contains("대상 글 없음"), "{none}");
        assert!(!none.contains("✅"), "{none}");

        // 대상은 있었으나 전부 실패: "전부 실패" + 대상 글 수.
        let all_failed = comment_failure_log("acc", None, 3);
        assert!(
            all_failed.contains("\"밴드\" 댓글 전부 실패"),
            "{all_failed}"
        );
        assert!(all_failed.contains("대상 3글"), "{all_failed}");
        assert!(!all_failed.contains("✅"), "{all_failed}");
    }
}
