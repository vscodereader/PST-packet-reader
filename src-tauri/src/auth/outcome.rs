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
    /// "자세히 보기"용 개발자 trace(위치 앵커 + 런타임 백트레이스, #199/#210). CDP 실패
    /// (`AutomationError`)에서만 채워지고, 비번오류/차단 등 일반 실패는 None이다. 게시 실패의
    /// `BatchItem.trace`와 동일 역할 — 로그인 실패도 알림 로그 "자세히 보기"에 백트레이스를 띄운다.
    pub trace: Option<String>,
}

impl LoginResolution {
    /// 로그인 성공(쿠키 저장 완료).
    pub(crate) fn active() -> Self {
        Self {
            status: AccountStatus::Active,
            message: guide(&AccountStatus::Active).to_owned(),
            succeeded: true,
            trace: None,
        }
    }

    /// 실패 계열(비번오류/인증필요/차단/오류). 큐 상태는 실패로 본다. 백트레이스가 없는
    /// 일반 실패(비번오류/차단 등)에 쓴다 — trace는 None.
    pub(crate) fn failure(status: AccountStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            succeeded: false,
            trace: None,
        }
    }

    /// 백트레이스를 동반한 실패(CDP/자동화 오류). "자세히 보기"에 trace를 띄우기 위해 보존한다.
    pub(crate) fn failure_with_trace(
        status: AccountStatus,
        message: impl Into<String>,
        trace: Option<String>,
    ) -> Self {
        Self {
            status,
            message: message.into(),
            succeeded: false,
            trace,
        }
    }
}

/// 상태별 사용자 조치 안내(정적). 프론트 `STATUS_GUIDE`와 의미를 맞춘다.
pub(crate) fn guide(status: &AccountStatus) -> &'static str {
    match status {
        AccountStatus::Active => "정상적으로 로그인되었습니다.",
        // 글 게시 성공 후의 대기 상태(#267-3). 로그인 경로에서는 거의 나오지 않지만 exhaustive.
        AccountStatus::Waiting => {
            "글 게시 완료 후 대기 중입니다. 상태를 눌러 다시 활성으로 바꿀 수 있습니다."
        }
        // 캡차 보류(#267 후속). 보류된 계정만 골라 다시 선택 로그인하면 캡차 10초 대기 뒤
        // 자동/수동으로 풀어 활성으로 되돌릴 수 있다.
        AccountStatus::OnHold => {
            "보안문자(캡차)가 떠 로그인이 보류되었습니다. 이 계정만 골라 다시 로그인하면 캡차를 직접 풀 수 있습니다."
        }
        // 대기초과(#286 후속): 페이지 대기시간 초과·네이버 서버 오류(HTTP 500) 등 일시적 문제로
        // 게시가 실패한 상태. 로그인 경로에서는 나오지 않지만 exhaustive 매치를 위해 둔다.
        AccountStatus::TimedOut => {
            "페이지 대기시간 초과 또는 네이버 서버 오류로 게시가 실패했습니다. 잠시 후 다시 시도하세요."
        }
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
        // 보호조치/잠금도 접근 차단 계열로 본다(별도 배지/바인딩 추가 없이 Blocked 재사용). 메시지만
        // resolve_non_ok에서 보호조치/잠금용으로 구분한다(#228/#243).
        LoginOutcome::Blocked | LoginOutcome::Protected | LoginOutcome::Locked => {
            AccountStatus::Blocked
        }
        // 캡차 미해결은 사람이 직접 풀면 회복 가능하므로 별도 "보류"(OnHold)로 둔다(#267 후속).
        LoginOutcome::CaptchaUnsolved => AccountStatus::OnHold,
        // 본인확인(휴대전화) 화면도 캡차와 동일하게 "보류"(OnHold)로 둔다(전화번호 패킷분석).
        LoginOutcome::PhoneVerify => AccountStatus::OnHold,
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
pub(crate) fn resolve_non_ok(outcome: LoginOutcome, trace: Option<String>) -> LoginResolution {
    let status = outcome_to_status(&outcome);
    let message = match &outcome {
        LoginOutcome::ChallengeRequired { kind } => format!(
            "{} 필요 — 열린 창에서 완료한 뒤 다시 실행하세요.",
            challenge_label(kind)
        ),
        LoginOutcome::Error(msg) => msg.clone(),
        // 보호조치는 Blocked 상태로 매핑되지만 안내는 "잠시 후 다시"가 아니라 해제 절차를
        // 알려야 한다(#228) — 일시적 차단이 아니라 사용자가 네이버에서 직접 풀어야 하는 상태다.
        LoginOutcome::Protected => {
            "계정에 보호조치가 적용되어 로그인이 차단되었습니다. 네이버에서 보호조치를 해제한 뒤 다시 시도하세요."
                .to_owned()
        }
        // 잠금도 Blocked 상태로 매핑되지만 안내는 "잠시 후 다시"가 아니라 해제 절차를 알려야
        // 한다(#243) — 사용자가 네이버에서 본인 확인으로 직접 풀어야 하는 종료 상태다.
        LoginOutcome::Locked => {
            "계정이 잠겨 로그인할 수 없습니다. 네이버에서 본인 확인으로 잠금을 해제한 뒤 다시 시도하세요."
                .to_owned()
        }
        // 본인확인(휴대전화) 화면은 OnHold로 매핑되지만 안내는 캡차용이 아니라 전화 본인확인용으로
        // 둔다(전화번호 패킷분석). ID가 010+8자리가 아니거나 번호 확인이 바로 통과 못 하면 보류된다.
        LoginOutcome::PhoneVerify => {
            "본인확인(휴대전화 번호) 화면이 떠 로그인이 보류되었습니다. 계정 ID가 휴대전화 형식이면 자동 입력을 시도하며, 통과하지 못하면 보류로 남습니다."
                .to_owned()
        }
        // Ok/BadCredentials/Blocked/CaptchaUnsolved는 정적 안내 문구를 그대로 쓴다.
        _ => guide(&status).to_owned(),
    };
    if status == AccountStatus::Active {
        LoginResolution::active()
    } else {
        // trace(CDP 실패 백트레이스)는 있으면 보존해 "자세히 보기"에 띄운다(#210).
        LoginResolution::failure_with_trace(status, message, trace)
    }
}

/// 상태에 맞는 활동 피드 타입.
pub(crate) fn status_activity_type(status: &AccountStatus) -> ActivityType {
    match status {
        AccountStatus::Active => ActivityType::Success,
        AccountStatus::Challenge | AccountStatus::New | AccountStatus::Waiting => {
            ActivityType::Info
        }
        // 보류(캡차 미해결)는 실패가 아니라 사용자 조치 대기 — Info로 둔다(#267 후속).
        AccountStatus::OnHold => ActivityType::Info,
        // 대기초과(서버/타이밍 일시 문제, #286 후속)도 재시도 대상이라 Info로 둔다.
        AccountStatus::TimedOut => ActivityType::Info,
        AccountStatus::BadCredentials | AccountStatus::Blocked | AccountStatus::Error => {
            ActivityType::Error
        }
    }
}

/// 활동 피드 한 줄 메시지(loginId + 상태 라벨 + 사유).
pub(crate) fn activity_message(login_id: &str, status: &AccountStatus, detail: &str) -> String {
    let label = match status {
        AccountStatus::Active => "로그인 성공",
        AccountStatus::Waiting => "게시 완료(대기)",
        AccountStatus::OnHold => "캡차 보류",
        AccountStatus::TimedOut => "대기초과",
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
        let r = resolve_non_ok(LoginOutcome::Error("타임아웃".into()), None);
        assert_eq!(r.status, AccountStatus::Error);
        assert_eq!(r.message, "타임아웃");
        assert!(!r.succeeded);
        assert_eq!(r.trace, None);
    }

    #[test]
    fn resolve_non_ok_preserves_trace_for_detail_view() {
        // CDP 실패에서 넘어온 백트레이스는 "자세히 보기"용으로 보존된다(#210).
        let r = resolve_non_ok(
            LoginOutcome::Error("연결 실패".into()),
            Some("at file.rs:1:1\n\nframe0".to_owned()),
        );
        assert_eq!(r.status, AccountStatus::Error);
        assert_eq!(r.trace.as_deref(), Some("at file.rs:1:1\n\nframe0"));
    }

    #[test]
    fn resolve_non_ok_blocked_uses_guide() {
        let r = resolve_non_ok(LoginOutcome::Blocked, None);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(r.message.contains("차단"));
        assert!(!r.succeeded);
    }

    #[test]
    fn protected_maps_to_blocked_status() {
        // 보호조치는 별도 배지 없이 Blocked 상태로 매핑한다(#228).
        assert_eq!(
            outcome_to_status(&LoginOutcome::Protected),
            AccountStatus::Blocked
        );
    }

    #[test]
    fn resolve_non_ok_protected_uses_release_guidance() {
        // 보호조치 메시지는 "잠시 후 다시"가 아니라 해제 절차를 안내해야 한다(#228).
        let r = resolve_non_ok(LoginOutcome::Protected, None);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(r.message.contains("보호조치"));
        assert!(!r.message.contains("잠시 후 다시"));
        assert!(!r.succeeded);
    }

    #[test]
    fn locked_maps_to_blocked_status() {
        // 잠금은 별도 배지 없이 Blocked 상태로 매핑한다(#243).
        assert_eq!(
            outcome_to_status(&LoginOutcome::Locked),
            AccountStatus::Blocked
        );
    }

    #[test]
    fn resolve_non_ok_locked_uses_release_guidance() {
        // 잠금 메시지는 "잠시 후 다시"가 아니라 해제 절차(본인 확인)를 안내해야 한다(#243).
        let r = resolve_non_ok(LoginOutcome::Locked, None);
        assert_eq!(r.status, AccountStatus::Blocked);
        assert!(r.message.contains("잠겨"));
        assert!(!r.message.contains("잠시 후 다시"));
        assert!(!r.succeeded);
    }

    #[test]
    fn resolve_non_ok_challenge_names_the_kind() {
        let r = resolve_non_ok(
            LoginOutcome::ChallengeRequired {
                kind: ChallengeKind::Otp,
            },
            None,
        );
        assert_eq!(r.status, AccountStatus::Challenge);
        assert!(r.message.contains("OTP"));
    }

    #[test]
    fn captcha_unsolved_maps_to_on_hold() {
        // 캡차 미해결은 일반 Error가 아니라 보류(OnHold)로 매핑된다(#267 후속).
        assert_eq!(
            outcome_to_status(&LoginOutcome::CaptchaUnsolved),
            AccountStatus::OnHold
        );
    }

    #[test]
    fn resolve_non_ok_captcha_unsolved_is_on_hold_and_recoverable_guidance() {
        let r = resolve_non_ok(LoginOutcome::CaptchaUnsolved, None);
        assert_eq!(r.status, AccountStatus::OnHold);
        assert!(r.message.contains("캡차") || r.message.contains("보안문자"));
        assert!(!r.succeeded);
    }

    #[test]
    fn on_hold_activity_type_is_info_not_error() {
        // 보류는 실패가 아니라 사용자 조치 대기이므로 Info로 분류한다.
        assert_eq!(
            status_activity_type(&AccountStatus::OnHold),
            ActivityType::Info
        );
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
