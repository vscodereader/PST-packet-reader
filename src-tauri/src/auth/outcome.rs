//! 로그인 결과(`LoginOutcome`)를 프론트엔드용 계정 상태(`AccountStatus`)와 사용자 안내
//! 메시지로 변환하는 순수 매핑. `LoginOutcome`은 auth 내부 타입이라 여기서만 IPC enum과
//! 잇는다. 단위 테스트가 커버리지에 잡히도록, ignore 대상인 `login.rs`/`login_flow.rs`와
//! 분리된 별도 모듈에 둔다.

use crate::ipc::accounts::AccountStatus;
use crate::ipc::activity::ActivityType;

use super::login_flow::{ChallengeKind, LoginOutcome};

/// 로그인 시도의 최종 해석. `worker_loop`가 큐 상태(성공/실패)와 계정 세밀 상태를 함께
/// 결정하는 데 쓴다.
pub(crate) struct LoginResolution {
    pub status: AccountStatus,
    pub message: String,
    /// 큐 상태 판정용: 로그인이 실제로 성공해 쿠키가 저장됐는가.
    pub succeeded: bool,
}

impl LoginResolution {
    /// 로그인 성공(쿠키 저장 완료).
    pub(crate) fn active() -> Self {
        Self {
            status: AccountStatus::Active,
            message: guide(&AccountStatus::Active).to_owned(),
            succeeded: true,
        }
    }

    /// 실패 계열(비번오류/인증필요/차단/오류). 큐 상태는 실패로 본다.
    pub(crate) fn failure(status: AccountStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            succeeded: false,
        }
    }
}

/// 상태별 사용자 조치 안내(정적). 프론트 `STATUS_GUIDE`와 의미를 맞춘다.
pub(crate) fn guide(status: &AccountStatus) -> &'static str {
    match status {
        AccountStatus::Active => "정상적으로 로그인되었습니다.",
        AccountStatus::BadCredentials => {
            "아이디 또는 비밀번호가 올바르지 않습니다. 계정 정보를 확인하세요."
        }
        AccountStatus::Challenge => {
            "추가 인증이 필요합니다. 열린 창에서 캡차/2차 인증을 완료한 뒤 다시 실행하세요."
        }
        AccountStatus::Blocked => {
            "계정 접근이 차단되었습니다. 잠시 후 다시 시도하거나 계정 상태를 확인하세요."
        }
        AccountStatus::Error => "로그인 중 오류가 발생했습니다. 네트워크/환경을 확인하세요.",
        AccountStatus::New => "아직 로그인하지 않은 계정입니다.",
    }
}

/// `LoginOutcome` → 계정 상태. 쿠키 저장 성공 여부(IO)는 호출부가 별도 처리하므로 여기서는
/// 신호 분류만 한다(Ok는 잠정 Active).
pub(crate) fn outcome_to_status(outcome: &LoginOutcome) -> AccountStatus {
    match outcome {
        LoginOutcome::Ok { .. } => AccountStatus::Active,
        LoginOutcome::ChallengeRequired { .. } => AccountStatus::Challenge,
        LoginOutcome::BadCredentials => AccountStatus::BadCredentials,
        LoginOutcome::Blocked => AccountStatus::Blocked,
        LoginOutcome::Error(_) => AccountStatus::Error,
    }
}

/// 인증 종류를 사람이 읽는 라벨로(챌린지 메시지용).
fn challenge_label(kind: &ChallengeKind) -> &'static str {
    match kind {
        ChallengeKind::Captcha => "캡차",
        ChallengeKind::Otp => "2차 인증(OTP)",
        ChallengeKind::Device => "새 기기 인증",
    }
}

/// 비-Ok outcome을 해석한다. 상태는 [`outcome_to_status`]를 단일 진실로 쓰고, 메시지만
/// 종류별로 다듬는다. Ok는 쿠키 저장 IO 뒤에 호출부가 `active`/`failure`로 직접 만든다
/// (여기 Ok 경로는 방어적 기본값).
pub(crate) fn resolve_non_ok(outcome: LoginOutcome) -> LoginResolution {
    let status = outcome_to_status(&outcome);
    let message = match &outcome {
        LoginOutcome::ChallengeRequired { kind } => format!(
            "{} 필요 — 열린 창에서 완료한 뒤 다시 실행하세요.",
            challenge_label(kind)
        ),
        LoginOutcome::Error(msg) => msg.clone(),
        // Ok/BadCredentials/Blocked는 정적 안내 문구를 그대로 쓴다.
        _ => guide(&status).to_owned(),
    };
    if status == AccountStatus::Active {
        LoginResolution::active()
    } else {
        LoginResolution::failure(status, message)
    }
}

/// 상태에 맞는 활동 피드 타입.
pub(crate) fn status_activity_type(status: &AccountStatus) -> ActivityType {
    match status {
        AccountStatus::Active => ActivityType::Success,
        AccountStatus::Challenge | AccountStatus::New => ActivityType::Info,
        AccountStatus::BadCredentials | AccountStatus::Blocked | AccountStatus::Error => {
            ActivityType::Error
        }
    }
}

/// 활동 피드 한 줄 메시지(loginId + 상태 라벨 + 사유).
pub(crate) fn activity_message(login_id: &str, status: &AccountStatus, detail: &str) -> String {
    let label = match status {
        AccountStatus::Active => "로그인 성공",
        AccountStatus::BadCredentials => "로그인 실패(비밀번호 오류)",
        AccountStatus::Challenge => "추가 인증 필요",
        AccountStatus::Blocked => "접근 차단",
        AccountStatus::Error => "로그인 오류",
        AccountStatus::New => "미로그인",
    };
    format!("계정 {login_id} — {label}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_maps_to_expected_status() {
        assert_eq!(
            outcome_to_status(&LoginOutcome::Ok { cookies: vec![] }),
            AccountStatus::Active
        );
        assert_eq!(
            outcome_to_status(&LoginOutcome::ChallengeRequired {
                kind: ChallengeKind::Captcha
            }),
            AccountStatus::Challenge
        );
        assert_eq!(
            outcome_to_status(&LoginOutcome::BadCredentials),
            AccountStatus::BadCredentials
        );
        assert_eq!(
            outcome_to_status(&LoginOutcome::Blocked),
            AccountStatus::Blocked
        );
        assert_eq!(
            outcome_to_status(&LoginOutcome::Error("x".into())),
            AccountStatus::Error
        );
    }

    #[test]
    fn resolve_non_ok_preserves_error_text_and_marks_failure() {
        let r = resolve_non_ok(LoginOutcome::Error("타임아웃".into()));
        assert_eq!(r.status, AccountStatus::Error);
        assert_eq!(r.message, "타임아웃");
        assert!(!r.succeeded);
    }

    #[test]
    fn resolve_non_ok_blocked_uses_guide() {
        let r = resolve_non_ok(LoginOutcome::Blocked);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(r.message.contains("차단"));
        assert!(!r.succeeded);
    }

    #[test]
    fn resolve_non_ok_challenge_names_the_kind() {
        let r = resolve_non_ok(LoginOutcome::ChallengeRequired {
            kind: ChallengeKind::Otp,
        });
        assert_eq!(r.status, AccountStatus::Challenge);
        assert!(r.message.contains("OTP"));
    }

    #[test]
    fn active_resolution_is_success() {
        let r = LoginResolution::active();
        assert_eq!(r.status, AccountStatus::Active);
        assert!(r.succeeded);
    }

    #[test]
    fn activity_type_groups_failures_as_error() {
        assert_eq!(
            status_activity_type(&AccountStatus::Active),
            ActivityType::Success
        );
        assert_eq!(
            status_activity_type(&AccountStatus::Challenge),
            ActivityType::Info
        );
        assert_eq!(
            status_activity_type(&AccountStatus::BadCredentials),
            ActivityType::Error
        );
        assert_eq!(
            status_activity_type(&AccountStatus::Blocked),
            ActivityType::Error
        );
    }

    #[test]
    fn activity_message_includes_id_label_and_detail() {
        let m = activity_message("user01", &AccountStatus::Blocked, "잠시 후 다시");
        assert!(m.contains("user01"));
        assert!(m.contains("접근 차단"));
        assert!(m.contains("잠시 후 다시"));
    }
}
