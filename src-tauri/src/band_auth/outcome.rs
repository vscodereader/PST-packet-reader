//! 밴드 로그인 결과(`BandLoginOutcome`)를 계정 상태(`AccountStatus`)/사용자 메시지로 옮기는
//! 순수 매핑. 네이버 `auth/outcome.rs`를 미러한다 — I/O가 있는 `login.rs`/`login_flow.rs`와
//! 분리해 단위 테스트가 커버리지에 잡히게 한다. 밴드는 캡차/2차 인증이 없어 `challenge`는 없다.

use crate::auth::outcome::LoginResolution;
use crate::ipc::accounts::AccountStatus;

use super::login_flow::BandLoginOutcome;

/// 실패 계열 밴드 결과를 세분 상태(badCredentials/blocked/error)로 해석한다. 성공(`Ok`)은
/// 쿠키 저장 IO 결과까지 봐야 하므로 호출부(`login::finalize`)가 `active`/`failure`로 직접
/// 만든다 — 여기 `Ok` 가지는 방어적 기본값(active)이다.
pub(crate) fn resolve_band_failure(outcome: &BandLoginOutcome) -> LoginResolution {
    match outcome {
        BandLoginOutcome::BadCredentials => LoginResolution::failure(
            AccountStatus::BadCredentials,
            "이메일 또는 비밀번호가 올바르지 않습니다.",
        ),
        BandLoginOutcome::Blocked => LoginResolution::failure(
            AccountStatus::Blocked,
            "로그인 접근이 차단되었습니다(계정 상태 확인 필요).",
        ),
        BandLoginOutcome::Error(message) => {
            LoginResolution::failure(AccountStatus::Error, message.clone())
        }
        BandLoginOutcome::Ok { .. } => LoginResolution::active(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_credentials_maps_to_bad_credentials_failure() {
        let r = resolve_band_failure(&BandLoginOutcome::BadCredentials);
        assert_eq!(r.status, AccountStatus::BadCredentials);
        assert!(!r.succeeded);
        assert!(!r.message.is_empty());
    }

    #[test]
    fn blocked_maps_to_blocked_failure() {
        let r = resolve_band_failure(&BandLoginOutcome::Blocked);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(!r.succeeded);
        assert!(!r.message.is_empty());
    }

    #[test]
    fn error_carries_original_message() {
        let r = resolve_band_failure(&BandLoginOutcome::Error("연결 시간 초과".into()));
        assert_eq!(r.status, AccountStatus::Error);
        assert!(!r.succeeded);
        assert_eq!(r.message, "연결 시간 초과");
    }

    #[test]
    fn ok_is_defensive_active() {
        let r = resolve_band_failure(&BandLoginOutcome::Ok { cookies: vec![] });
        assert_eq!(r.status, AccountStatus::Active);
        assert!(r.succeeded);
    }
}
