//! 밴드 로그인 결과(`BandLoginOutcome`)를 계정 상태(`AccountStatus`)/사용자 메시지로 옮기는
//! 순수 매핑. 네이버 `auth/outcome.rs`를 미러한다 — I/O가 있는 `login.rs`/`login_flow.rs`와
//! 분리해 단위 테스트가 커버리지에 잡히게 한다. 밴드는 캡차/2차 인증이 없어 `challenge`는 없다.

use crate::auth::outcome::LoginResolution;
use crate::ipc::accounts::AccountStatus;

use super::login_flow::BandLoginOutcome;

/// 실패 계열 밴드 결과를 세분 상태(badCredentials/blocked/error)로 해석한다. 성공(`Ok`)은
/// 쿠키 저장 IO 결과까지 봐야 하므로 호출부(`login::finalize`)가 `active`/`failure`로 직접
/// 만든다 — 여기 `Ok` 가지는 방어적 기본값(active)이다.
pub(crate) fn resolve_band_failure(
    outcome: &BandLoginOutcome,
    trace: Option<String>,
) -> LoginResolution {
    match outcome {
        BandLoginOutcome::BadCredentials => LoginResolution::failure(
            AccountStatus::BadCredentials,
            "BAND 이메일 또는 비밀번호가 올바르지 않습니다.",
        ),
        BandLoginOutcome::Blocked => LoginResolution::failure(
            AccountStatus::Blocked,
            "로그인 접근이 차단되었습니다(계정 상태 확인 필요).",
        ),
        // 본인확인(휴대전화) 화면이 떴는데 계정 ID가 휴대전화 형식이 아니라 자동으로 풀 수 없는
        // 보류 상태(네이버 PhoneVerify → OnHold 미러). 실패가 아니라 사용자 조치 대기다.
        BandLoginOutcome::OnHold => LoginResolution::failure(
            AccountStatus::OnHold,
            "본인확인(휴대전화) 화면이 떠 로그인이 보류되었습니다. 이 계정만 골라 다시 로그인해 직접 처리하세요.",
        ),
        // CDP 실패에서 온 trace는 "자세히 보기"에 띄우려고 보존한다(#210).
        BandLoginOutcome::Error(message) => {
            LoginResolution::failure_with_trace(AccountStatus::Error, message.clone(), trace)
        }
        BandLoginOutcome::Ok { .. } => LoginResolution::active(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_credentials_maps_to_bad_credentials_failure() {
        let r = resolve_band_failure(&BandLoginOutcome::BadCredentials, None);
        assert_eq!(r.status, AccountStatus::BadCredentials);
        assert!(!r.succeeded);
        assert_eq!(r.message, "BAND 이메일 또는 비밀번호가 올바르지 않습니다.");
    }

    #[test]
    fn blocked_maps_to_blocked_failure() {
        let r = resolve_band_failure(&BandLoginOutcome::Blocked, None);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(!r.succeeded);
        assert!(!r.message.is_empty());
    }

    #[test]
    fn error_carries_original_message() {
        let r = resolve_band_failure(&BandLoginOutcome::Error("연결 시간 초과".into()), None);
        assert_eq!(r.status, AccountStatus::Error);
        assert!(!r.succeeded);
        assert_eq!(r.message, "연결 시간 초과");
    }

    #[test]
    fn error_preserves_trace_for_detail_view() {
        // CDP 실패 백트레이스는 "자세히 보기"용으로 보존된다(#210).
        let r = resolve_band_failure(
            &BandLoginOutcome::Error("연결 실패".into()),
            Some("at band.rs:1:1\n\nframe0".to_owned()),
        );
        // 사용자 사유가 백트레이스 위에 먼저 붙는다(자세히 보기 전문 노출, 공유 LoginResolution).
        assert_eq!(
            r.trace.as_deref(),
            Some("연결 실패\n\nat band.rs:1:1\n\nframe0")
        );
    }

    #[test]
    fn on_hold_maps_to_on_hold_failure() {
        // 본인확인(휴대전화) 비-형식 ID 보류는 OnHold 상태로 매핑되고 실패(비활성)로 본다.
        let r = resolve_band_failure(&BandLoginOutcome::OnHold, None);
        assert_eq!(r.status, AccountStatus::OnHold);
        assert!(!r.succeeded);
        assert!(!r.message.is_empty());
    }

    #[test]
    fn ok_is_defensive_active() {
        let r = resolve_band_failure(&BandLoginOutcome::Ok { cookies: vec![] }, None);
        assert_eq!(r.status, AccountStatus::Active);
        assert!(r.succeeded);
    }
}
