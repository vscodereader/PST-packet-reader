//! 좋아요 전용 흐름(게시 경로와 분리).
//!
//! 게시(`open_discussion_session`)는 npay 가입/프로필 로직을 그대로 두어 안정성을 지키고, 좋아요는
//! 패킷 실측(2026-07-01 캡처 `토론 좋아요 싫어요 버튼.pcapng`)에 맞춰 **여기서만** 별도 처리한다.
//! 실측 요지: `POST /api/community/discussion/posts/{id}/reactions` 는 `Nid-No` 헤더 없이 **순수
//! 로그인 쿠키(NID_AUT/NID_SES/BUC/nid_inf)만으로 인증**되고, 프로필 생성·npay 가입 호출 없이도
//! 이미 가입된 계정은 곧바로 눌린다.
//!
//! 정책:
//! 0. 좋아요 직전 `read_login_profile()`(순수 getProfile API)로 세션 생존을 확인한다 — 로그아웃이면
//!    npay·좋아요를 아예 시도하지 않고(쿠키 훼손 0) "재로그인 필요"로 넘긴다.
//! 1. 세션이 살아있으면 그대로 좋아요를 시도한다 — 이미 가입된 계정(대다수)은 npay 없이 성공한다.
//! 2. '미가입' 사유로 실패하면 그때만 npay 동의를 **쿠키 보존 모드**로 시도하고 1회 재시도한다.
//! 3. 세션 만료(`Nid-No required`/로그인 페이지 튕김)면 쿠키를 훼손하지 않고 "재로그인 필요"로 넘긴다.
//!
//! 게시 경로가 좋아요 직전 `ensure_npay_financial_join()`을 무조건 호출해 만료 세션의 NID_AUT를 빈
//! 값으로 덮어써 좋아요가 전량 400으로 실패하던 회귀(2026-07-02 야간 배치)를 이 분리로 없앤다.

use super::packet_client::{self, NpayJoinStatus};
use super::{AutomationError, AutomationResult};

/// 저장된 로그인 쿠키만으로 종목토론방 게시글에 **좋아요**를 누른다(Chrome·페이지 이동 없이
/// reactions API 전용). `post_url`은 특정 게시글 링크이며 계정별로 호출한다. 이미 좋아요면
/// 성공으로 본다(멱등). 쿠키 없음/세션 만료 등은 `AutomationError`로 올라간다.
pub fn run_naver_like(account_id: &str, post_url: &str) -> AutomationResult<()> {
    let post_url = post_url.trim();
    if post_url.is_empty() {
        return Err(AutomationError::new("좋아요를 누를 게시글 링크가 비어 있습니다."));
    }
    tracing::info!(
        account = %crate::auth::mask_id(account_id),
        "종토방 좋아요 시작(API 전용, 페이지 이동 없음)"
    );
    let storage = crate::auth::read_account_cookies(account_id)
        .map_err(|error| {
            AutomationError::new(format!("계정 '{account_id}' 쿠키 조회 실패: {error}"))
        })?
        .ok_or_else(|| {
            AutomationError::new(format!(
                "계정 '{account_id}'의 저장된 로그인 쿠키가 없습니다. 먼저 로그인하세요."
            ))
        })?;
    let mut client = packet_client::NaverPacketClient::from_storage_state(&storage)?;
    // 저장된 쿠키에서 방금 만든 좋아요 클라이언트가 들고 있는 인증 쿠키(NID_AUT/NID_SES/BUC)를
    // 원문 그대로 남긴다 — 재로그인 후 다시 좋아요를 돌리면 이 로그에 새 문자열이 찍혀, 만료 전/후
    // 쿠키를 원문으로 대조할 수 있다(사용자 요청 2026-07-03).
    client.log_auth_cookies_raw("좋아요-시작-로드된쿠키");

    // 0) 좋아요 직전 "세션 살아있나" 확인 — 페이지 이동 없이 기존 `read_login_profile()`(순수
    //    getProfile API, 실측 패킷 static.nid.naver.com/getProfile) 한 번 재사용한다. 로그아웃으로
    //    확인되면 npay·좋아요를 아예 시도하지 않고(쿠키 훼손 0) 곧장 "재로그인 필요"로 넘긴다 —
    //    죽은 계정만 걸러내고 멀쩡한 계정은 불필요한 로그인·캡차 노출을 피한다. 이 확인 자체가
    //    전송 실패(네트워크 등)면 만료로 단정하지 않고 그대로 좋아요 시도로 내려간다.
    match client.read_login_profile() {
        Ok(profile) if !profile.logged_in => {
            tracing::warn!(
                account = %crate::auth::mask_id(account_id),
                message = %profile.message,
                "좋아요 전 세션 확인 — 로그아웃 상태(getProfile) → 재로그인 필요"
            );
            return Err(relogin_needed_error(&format!(
                "좋아요 전 세션 확인 결과 로그아웃 상태입니다(getProfile: {})",
                profile.message
            )));
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                account = %crate::auth::mask_id(account_id),
                error = %error,
                "좋아요 전 세션 확인 실패(전송 계층) — 만료로 단정하지 않고 좋아요 시도로 진행"
            );
        }
    }

    // 1) 세션이 살아있으면 그대로 좋아요를 시도한다 — 실측 패킷상 이미 가입된 계정은 npay 없이 바로
    //    눌린다. (게시 경로처럼 npay를 무조건 먼저 부르면, 만료 세션에서 가입이 로그인으로 튕기며
    //    NID_AUT를 빈 값으로 덮어써 정상 계정까지 400으로 터졌다 — 2026-07-02 야간 배치 실패의 원인.)
    let first_error = match client.like_post(post_url) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    // 2) 세션 만료(인증쿠키가 서버에 도달 못 함)면 npay는 도움이 안 되고 오히려 쿠키를 훼손하므로
    //    시도하지 않고 곧장 "재로그인 필요"로 넘긴다. (step 0을 통과했어도 그 사이 만료됐을 수 있어
    //    한 번 더 방어한다.)
    if is_session_expired_error(&first_error) {
        return Err(relogin_needed_error(&format!("원본: {}", first_error.message())));
    }

    // 3) 만료가 아니면 '미가입' 추정 → npay 동의를 쿠키 보존 모드로 시도하고 1회만 재시도한다.
    //    동의가 로그인으로 튕기면(세션 만료) 쿠키를 지킨 채 재로그인으로 넘긴다.
    match client.try_npay_join_preserving_cookies() {
        NpayJoinStatus::LoginRequired => {
            Err(relogin_needed_error(&format!("원본: {}", first_error.message())))
        }
        NpayJoinStatus::Completed
        | NpayJoinStatus::TermsPending
        | NpayJoinStatus::Unknown => client.like_post(post_url),
    }
}

/// 좋아요 실패가 **세션 만료/인증쿠키 무효**에서 비롯됐는지 네이버 원문 응답으로 판별한다.
/// `post_with_retry`가 실패 메시지에 네이버 body를 그대로 실어주므로(그 안의 errorCode/문구), 만료
/// 신호 — 인증쿠키 미도달(`400Z01`, `[NidHeader] Nid-No is required`) 또는 로그인 페이지 리다이렉트
/// (`nidlogin.login`) — 를 문자열로 잡는다. 미가입/기타 실패는 여기 걸리지 않아 npay 재시도로 간다.
fn is_session_expired_error(error: &AutomationError) -> bool {
    let message = error.message();
    message.contains("Nid-No")
        || message.contains("400Z01")
        || message.contains("nidlogin.login")
}

/// 만료 계정용 사용자 안내 에러 — 알림 로그에 "재로그인 필요"로 뜬다. `reason`에 판정 근거(세션
/// 확인 결과/원본 실패 사유)를 붙여 진단을 남긴다.
fn relogin_needed_error(reason: &str) -> AutomationError {
    AutomationError::new(format!(
        "계정 세션이 만료되어 좋아요를 누를 수 없습니다 — 재로그인이 필요합니다. ({reason})"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nid_no_required_is_session_expired() {
        // 실측 400 본문(2026-07-02 좋아요 로그): Nid-No 미첨부 → 세션 만료로 분류.
        let error = AutomationError::new(
            "반응 생성 패킷 HTTP 실패: HTTP status 400 for url (...) body={\"errorCode\":\"400Z01\",\"message\":\"[NidHeader] Nid-No is required\"}",
        );
        assert!(is_session_expired_error(&error));
    }

    #[test]
    fn login_redirect_is_session_expired() {
        let error = AutomationError::new("가입이 nidlogin.login 으로 튕김 — 세션 무효");
        assert!(is_session_expired_error(&error));
    }

    #[test]
    fn membership_failure_is_not_session_expired() {
        // 미가입/기타 실패는 만료로 보지 않는다 → npay 동의 재시도 경로로 가야 한다.
        let error = AutomationError::new(
            "반응 생성 패킷 HTTP 실패: HTTP status 400 for url (...) body={\"errorCode\":\"400X99\",\"message\":\"not a member\"}",
        );
        assert!(!is_session_expired_error(&error));
    }

    #[test]
    fn post_not_exist_is_not_session_expired() {
        // 글 삭제(404)는 계정 문제가 아니다 — 만료로 오분류하지 않는다.
        let error = AutomationError::new(
            "반응 생성 패킷 HTTP 실패: HTTP status 404 for url (...) body={\"errorCode\":\"404A01\",\"title\":\"POST_NOT_EXIST\"}",
        );
        assert!(!is_session_expired_error(&error));
    }

    #[test]
    fn relogin_error_mentions_relogin_and_keeps_reason() {
        let error = relogin_needed_error("원본사유-XYZ");
        assert!(error.message().contains("재로그인"));
        assert!(error.message().contains("원본사유-XYZ"));
    }
}
