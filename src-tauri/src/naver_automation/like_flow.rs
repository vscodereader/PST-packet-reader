//! 좋아요 전용 흐름(게시 경로와 분리).
//!
//! 게시(`open_discussion_session`)는 npay 가입/프로필 로직을 그대로 두어 안정성을 지키고, 좋아요는
//! 패킷 실측(2026-07-01 캡처)에 맞춰 **여기서만** 별도 처리한다. 좋아요 POST는 `Nid-No` 헤더 없이
//! **순수 로그인 쿠키(NID_AUT/NID_SES/BUC/nid_inf)만으로 인증**되고, 프로필 생성·npay 없이도 이미
//! 가입된 계정은 곧바로 눌린다.
//!
//! 판정([`LikeVerdict`]) — 세션 만료(재로그인)/차단(비활성)을 **추측이 아니라 네이버 원문으로** 가른다
//! (2026-07-03 사용자 지적). 좋아요가 막히면 `probe_restriction_raw`가 `/profile/users/form` 응답
//! **원문 전체를 로그에 남기고** 그 원문으로 판정한다: UMON 밴/아이디 잠금/보호조치/이용제한/글쓰기
//! 금지/비정상적인 활동 → 차단, 로그인 안 됨 → 재로그인.

use super::packet_client::{self, NpayJoinStatus, RestrictionVerdict};
use super::AutomationError;

/// 좋아요 1건의 판정. 호출부(`lib.rs`)가 이 값으로 알림 메시지·**계정 상태 전환**(재로그인/비활성)·쿠키
/// 삭제를 결정한다.
pub enum LikeVerdict {
    /// 좋아요 성공(또는 이미 좋아요).
    Liked,
    /// 세션 만료 — 재로그인 필요(상태 `Relogin`, 쿠키 삭제).
    Relogin(String),
    /// 계정 차단 — 비활성(상태 `Blocked`, 쿠키 삭제).
    Blocked(String),
    /// 기타 실패(글 삭제 404 등 계정 문제 아님) — 상태는 바꾸지 않는다.
    Failed(String),
}

/// 저장된 로그인 쿠키만으로 종목토론방 게시글에 **좋아요**를 누른다. [`run_naver_reaction`] 래퍼.
pub fn run_naver_like(account_id: &str, post_url: &str) -> LikeVerdict {
    run_naver_reaction(account_id, post_url, "good")
}

/// 저장된 로그인 쿠키만으로 종목토론방 게시글에 **싫어요**를 누른다. 좋아요와 동일 경로이고
/// reactions API의 reactionType만 `"bad"`다(패킷 실측). [`run_naver_reaction`] 래퍼.
pub fn run_naver_dislike(account_id: &str, post_url: &str) -> LikeVerdict {
    run_naver_reaction(account_id, post_url, "bad")
}

/// 좋아요·싫어요 **공용** 흐름(Chrome·페이지 이동 없이 reactions API 전용). `reaction_type`:
/// `"good"`=좋아요 / `"bad"`=싫어요. 이미 같은 반응이면 성공(멱등). 결과를 [`LikeVerdict`]로 돌려준다.
/// 쿠키 로드·세션 확인(getProfile)·npay 재시도·차단/만료 원문 판정은 좋아요와 100% 동일하며,
/// 마지막에 호출하는 reactions API의 reactionType만 다르다.
fn run_naver_reaction(account_id: &str, post_url: &str, reaction_type: &str) -> LikeVerdict {
    let label = if reaction_type == "bad" { "싫어요" } else { "좋아요" };
    let post_url = post_url.trim();
    if post_url.is_empty() {
        return LikeVerdict::Failed(format!("{label}를 누를 게시글 링크가 비어 있습니다."));
    }
    tracing::info!(
        account = %crate::auth::mask_id(account_id),
        "종토방 {label} 시작(API 전용, 페이지 이동 없음)"
    );

    let storage = match crate::auth::read_account_cookies(account_id) {
        Ok(Some(storage)) => storage,
        Ok(None) => {
            return LikeVerdict::Relogin(
                "저장된 로그인 쿠키가 없거나 만료됨 — 재로그인이 필요합니다.".to_owned(),
            );
        }
        Err(error) => {
            return LikeVerdict::Failed(format!("계정 '{account_id}' 쿠키 조회 실패: {error}"));
        }
    };
    let mut client = match packet_client::NaverPacketClient::from_storage_state(&storage) {
        Ok(client) => client,
        Err(error) => return LikeVerdict::Failed(format!("{label} 클라이언트 생성 실패: {error}")),
    };
    // 로드된 인증 쿠키 원문을 남긴다(재로그인 후 새 문자열과 대조 — 사용자 요청 2026-07-03).
    client.log_auth_cookies_raw(&format!("{label}-시작-로드된쿠키"));

    // 0) 세션 생존 확인(getProfile). 로그아웃이면 원문 프로브로 차단/만료를 확정한다(추측 금지).
    let logged_out = matches!(client.read_login_profile(), Ok(profile) if !profile.logged_in);
    if logged_out {
        tracing::warn!(
            account = %crate::auth::mask_id(account_id),
            "{label} 전 세션 확인 — 로그아웃 상태(getProfile) → 원문(form)으로 차단/만료 확정"
        );
        return verdict_from_probe(&client, "getProfile 로그아웃");
    }

    // 1) 세션이 살아있으면 그대로 반응을 시도한다 — 이미 가입된 계정은 npay 없이 바로 눌린다.
    let first_error = match client.react_post(post_url, reaction_type) {
        Ok(()) => return LikeVerdict::Liked,
        Err(error) => error,
    };

    // 2) 반응 응답에 이미 차단 신호(403 UMON)가 담겨 있으면 즉시 비활성.
    if is_blocked_error(&first_error) {
        return LikeVerdict::Blocked(format!("계정 차단(비활성) — {}", first_error.message()));
    }

    // 3) 세션 만료 신호가 아니면 '미가입' 추정 → 쿠키 보존 npay 후 1회 재시도.
    if !is_session_expired_error(&first_error) {
        match client.try_npay_join_preserving_cookies() {
            NpayJoinStatus::Completed | NpayJoinStatus::TermsPending | NpayJoinStatus::Unknown => {
                match client.react_post(post_url, reaction_type) {
                    Ok(()) => return LikeVerdict::Liked,
                    Err(error) if is_blocked_error(&error) => {
                        return LikeVerdict::Blocked(format!("계정 차단(비활성) — {}", error.message()));
                    }
                    Err(_) => {}
                }
            }
            NpayJoinStatus::LoginRequired => {}
        }
    }

    // 4) 여기까지 막혔으면 **원문(form)으로 차단/만료를 확정**한다(자르지 않은 응답을 로그에 남기고 판정).
    verdict_from_probe(&client, &format!("{label} 실패({})", first_error.message()))
}

/// `probe_restriction_raw`(폼 원문 판정)를 [`LikeVerdict`]로 옮긴다. Healthy(정상·비차단)면 계정 문제가
/// 아니므로 상태를 바꾸지 않는 `Failed`로 둔다.
fn verdict_from_probe(client: &packet_client::NaverPacketClient, context: &str) -> LikeVerdict {
    match client.probe_restriction_raw() {
        RestrictionVerdict::Blocked(raw) => {
            LikeVerdict::Blocked(format!("계정 차단(비활성) [{context}] — 원문: {raw}"))
        }
        RestrictionVerdict::Expired(raw) => {
            LikeVerdict::Relogin(format!("세션 만료 — 재로그인 필요 [{context}] — 원문: {raw}"))
        }
        RestrictionVerdict::Healthy => LikeVerdict::Failed(format!(
            "좋아요 실패({context}) — 계정은 정상(비차단·로그인됨). 대상 글 삭제 등 계정 외 문제."
        )),
        RestrictionVerdict::Unknown(reason) => {
            LikeVerdict::Relogin(format!("판정 불가 → 재로그인 권장 [{context}] ({reason})"))
        }
    }
}

/// 실패 원문에 **차단/제재** 신호(403 UMON/`403F0x`/`penaltyDays`/보호조치)가 있는지.
fn is_blocked_error(error: &AutomationError) -> bool {
    let message = error.message();
    message.contains("UMON_TEMP_BANNED")
        || message.contains("UMON_PERMANENT_BANNED")
        || message.contains("403F01")
        || message.contains("403F02")
        || message.contains("penaltyDays")
        || message.contains("보호조치")
}

/// 실패 원문에 **세션 만료/인증쿠키 무효** 신호(`400Z01`/`Nid-No`/`nidlogin.login`)가 있는지.
fn is_session_expired_error(error: &AutomationError) -> bool {
    let message = error.message();
    message.contains("Nid-No")
        || message.contains("400Z01")
        || message.contains("nidlogin.login")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> AutomationError {
        AutomationError::new(text)
    }

    #[test]
    fn umon_ban_is_blocked() {
        let e = err("HTTP status 403 body={\"title\":\"UMON_TEMP_BANNED\",\"penaltyDays\":31}");
        assert!(is_blocked_error(&e));
    }

    #[test]
    fn nid_no_required_is_session_expired() {
        let e = err("HTTP status 400 body={\"errorCode\":\"400Z01\",\"message\":\"[NidHeader] Nid-No is required\"}");
        assert!(is_session_expired_error(&e));
        assert!(!is_blocked_error(&e));
    }

    #[test]
    fn post_not_exist_is_neither() {
        let e = err("HTTP status 404 body={\"title\":\"POST_NOT_EXIST\"}");
        assert!(!is_blocked_error(&e));
        assert!(!is_session_expired_error(&e));
    }
}
