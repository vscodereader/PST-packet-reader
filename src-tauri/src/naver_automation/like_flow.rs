//! 좋아요 전용 흐름(게시 경로와 분리).
//!
//! 게시(`open_discussion_session`)는 npay 가입/프로필 로직을 그대로 두어 안정성을 지키고, 좋아요는
//! 패킷 실측(2026-07-01 캡처 `토론 좋아요 싫어요 버튼.pcapng`)에 맞춰 **여기서만** 별도 처리한다.
//! 좋아요 POST는 `Nid-No` 헤더 없이 **순수 로그인 쿠키(NID_AUT/NID_SES/BUC/nid_inf)만으로 인증**되고,
//! 프로필 생성·npay 가입 호출 없이도 이미 가입된 계정은 곧바로 눌린다.
//!
//! 판정([`LikeVerdict`]) — 세션 만료(재로그인)와 차단(비활성)을 구분한다(2026-07-03 사용자 지적:
//! 차단 계정이 "재로그인"으로 뭉뚱그려짐). 실측(07-02 게시로그) 근거:
//! - **차단(Blocked/비활성)**: 세션은 살아있으나(getProfile `rtn_cd:0`) 행동이 막힘 —
//!   npay가 로그인으로 튕기거나(`nidlogin.login`), 응답이 `403 UMON_*_BANNED`(403F01/403F02).
//! - **재로그인(Relogin)**: getProfile가 로그아웃(`rtn_cd:1`)으로 응답 = 세션이 죽음. 재로그인해야
//!   회복(재로그인 시 로그인 플로우가 실제 차단이면 `Blocked`로 확정).
//!
//! 게시 경로가 좋아요 직전 `ensure_npay_financial_join()`을 무조건 호출해 만료 세션의 NID_AUT를 빈
//! 값으로 덮어써 좋아요가 전량 400으로 실패하던 회귀(2026-07-02)를 이 분리로 없앤다.

use super::packet_client::{self, NpayJoinStatus};
use super::AutomationError;

/// 좋아요 1건의 판정. 호출부(`lib.rs`)가 이 값으로 알림 메시지와 **계정 상태 전환**(재로그인/비활성)
/// 및 쿠키 삭제를 결정한다.
pub enum LikeVerdict {
    /// 좋아요 성공(또는 이미 좋아요).
    Liked,
    /// 세션 만료 — 재로그인 필요(상태 `Relogin`으로, 쿠키 삭제).
    Relogin(String),
    /// 계정 차단 — 비활성(상태 `Blocked`로, 쿠키 삭제).
    Blocked(String),
    /// 기타 실패(글 삭제 404 등 계정 문제 아님) — 상태는 바꾸지 않는다.
    Failed(String),
}

/// 저장된 로그인 쿠키만으로 종목토론방 게시글에 **좋아요**를 누른다(Chrome·페이지 이동 없이
/// reactions API 전용). 이미 좋아요면 성공으로 본다(멱등). 결과를 [`LikeVerdict`]로 돌려준다.
pub fn run_naver_like(account_id: &str, post_url: &str) -> LikeVerdict {
    let post_url = post_url.trim();
    if post_url.is_empty() {
        return LikeVerdict::Failed("좋아요를 누를 게시글 링크가 비어 있습니다.".to_owned());
    }
    tracing::info!(
        account = %crate::auth::mask_id(account_id),
        "종토방 좋아요 시작(API 전용, 페이지 이동 없음)"
    );

    // 쿠키 없음/만료(유효 세션 쿠키 아님)면 곧장 재로그인 대상.
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
        Err(error) => return LikeVerdict::Failed(format!("좋아요 클라이언트 생성 실패: {error}")),
    };
    // 로드된 인증 쿠키 원문을 남긴다(재로그인 후 새 문자열과 대조 — 사용자 요청 2026-07-03).
    client.log_auth_cookies_raw("좋아요-시작-로드된쿠키");

    // 0) 세션 생존 확인 — 페이지 이동 없이 getProfile API 한 번. 로그아웃(rtn_cd:1)이면 세션이 죽은
    //    것이라 재로그인 대상. 전송 실패(네트워크)면 만료로 단정하지 않고 좋아요 시도로 내려간다.
    let session_alive = match client.read_login_profile() {
        Ok(profile) if !profile.logged_in => {
            tracing::warn!(
                account = %crate::auth::mask_id(account_id),
                message = %profile.message,
                "좋아요 전 세션 확인 — 로그아웃 상태(getProfile) → 재로그인 필요"
            );
            return LikeVerdict::Relogin(format!(
                "세션 만료(getProfile 로그아웃: {}) — 재로그인이 필요합니다.",
                profile.message
            ));
        }
        Ok(_) => true,
        Err(error) => {
            tracing::warn!(
                account = %crate::auth::mask_id(account_id),
                error = %error,
                "좋아요 전 세션 확인 실패(전송 계층) — 만료로 단정하지 않고 좋아요 시도로 진행"
            );
            false
        }
    };

    // 1) 세션이 살아있으면 그대로 좋아요를 시도한다 — 이미 가입된 계정은 npay 없이 바로 눌린다.
    let first_error = match client.like_post(post_url) {
        Ok(()) => return LikeVerdict::Liked,
        Err(error) => error,
    };

    // 2) 실패 분류. 차단 신호(403 UMON/보호조치)면 세션 생존과 무관하게 비활성.
    if is_blocked_error(&first_error) {
        return LikeVerdict::Blocked(format!("계정 차단(비활성) — {}", first_error.message()));
    }
    // 세션 만료 신호(Nid-No/400Z01/로그인 페이지)면 재로그인.
    if is_session_expired_error(&first_error) {
        return LikeVerdict::Relogin(format!("세션 만료 — 재로그인 필요 ({})", first_error.message()));
    }

    // 3) 위 신호가 없으면 '미가입' 추정 → npay 동의를 쿠키 보존 모드로 시도하고 1회 재시도한다.
    match client.try_npay_join_preserving_cookies() {
        NpayJoinStatus::LoginRequired => {
            // npay가 로그인 페이지로 튕김. getProfile가 로그인됨(session_alive)이었다면 = 세션은
            // 살아있는데 계정이 제한 = **차단(비활성)**. 세션 상태를 몰랐다면(getProfile 전송 실패)
            // 재로그인으로 둔다.
            if session_alive {
                LikeVerdict::Blocked(format!(
                    "세션은 살아있으나 계정 제한(npay 로그인 튕김) → 비활성 — {}",
                    first_error.message()
                ))
            } else {
                LikeVerdict::Relogin(format!(
                    "세션 만료(npay 로그인 튕김) — 재로그인 필요 ({})",
                    first_error.message()
                ))
            }
        }
        NpayJoinStatus::Completed | NpayJoinStatus::TermsPending | NpayJoinStatus::Unknown => {
            match client.like_post(post_url) {
                Ok(()) => LikeVerdict::Liked,
                Err(error) if is_blocked_error(&error) => {
                    LikeVerdict::Blocked(format!("계정 차단(비활성) — {}", error.message()))
                }
                Err(error) if is_session_expired_error(&error) => {
                    LikeVerdict::Relogin(format!("세션 만료 — 재로그인 필요 ({})", error.message()))
                }
                Err(error) => LikeVerdict::Failed(error.message().to_owned()),
            }
        }
    }
}

/// 실패가 **계정 차단/제재**에서 비롯됐는지 네이버 원문으로 판별한다(실측 07-02 게시로그: 차단 계정은
/// `403 UMON_*_BANNED`/`403F01`/`403F02`/`penaltyDays`, 로그인 단계 `보호조치`). `post_with_retry`가
/// 실패 메시지에 네이버 body를 그대로 실어주므로 문자열로 잡는다.
fn is_blocked_error(error: &AutomationError) -> bool {
    let message = error.message();
    message.contains("UMON_TEMP_BANNED")
        || message.contains("UMON_PERMANENT_BANNED")
        || message.contains("403F01")
        || message.contains("403F02")
        || message.contains("penaltyDays")
        || message.contains("보호조치")
}

/// 실패가 **세션 만료/인증쿠키 무효**에서 비롯됐는지 판별한다. 만료 신호 — 인증쿠키 미도달(`400Z01`,
/// `[NidHeader] Nid-No is required`) 또는 로그인 페이지 리다이렉트(`nidlogin.login`). 차단 신호가 아닌
/// 것만 여기서 만료로 본다(차단은 [`is_blocked_error`]가 먼저 잡는다).
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
    fn umon_ban_is_blocked_not_expired() {
        let e = err(
            "반응 생성 패킷 HTTP 실패: HTTP status 403 body={\"errorCode\":\"403F02\",\"title\":\"UMON_TEMP_BANNED\",\"penaltyDays\":31}",
        );
        assert!(is_blocked_error(&e));
    }

    #[test]
    fn nid_no_required_is_session_expired() {
        let e = err(
            "반응 생성 패킷 HTTP 실패: HTTP status 400 body={\"errorCode\":\"400Z01\",\"message\":\"[NidHeader] Nid-No is required\"}",
        );
        assert!(is_session_expired_error(&e));
        assert!(!is_blocked_error(&e));
    }

    #[test]
    fn login_redirect_is_session_expired() {
        let e = err("가입이 nidlogin.login 으로 튕김 — 세션 무효");
        assert!(is_session_expired_error(&e));
    }

    #[test]
    fn post_not_exist_is_neither() {
        // 글 삭제(404)는 계정 문제가 아니다 — 차단도 만료도 아님(상태 안 바꿈).
        let e = err(
            "반응 생성 패킷 HTTP 실패: HTTP status 404 body={\"errorCode\":\"404A01\",\"title\":\"POST_NOT_EXIST\"}",
        );
        assert!(!is_blocked_error(&e));
        assert!(!is_session_expired_error(&e));
    }
}
