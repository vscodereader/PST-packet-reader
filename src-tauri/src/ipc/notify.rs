//! 데스크톱(OS) 토스트 알림(#163). 트레이 상주(#161)로 창을 닫아둔 사용자가,
//! 백그라운드 스케줄러(#154)의 게시 완료·놓침 결과를 인앱 "알림" 피드만이
//! 아니라 OS 네이티브 토스트로도 인지하게 한다. 발송은 best-effort라 실패·미지원 환경에서도
//! 인앱 기록은 그대로 동작한다(토스트는 부가 통지).

use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::NotificationExt;

/// 게시 완료 토스트 문구(제목, 본문)를 만든다. 성공/실패 비율로 제목을 달리한다.
pub fn completion_message(title: &str, total: usize, ok: usize) -> (String, String) {
    let head = if ok == total {
        "게시 완료"
    } else if ok == 0 {
        "게시 실패"
    } else {
        "게시 일부 실패"
    };
    (
        head.to_string(),
        format!("'{title}' — {total}곳 중 {ok}곳 성공"),
    )
}

/// 계정 로그인 완료 토스트 문구(제목, 본문, #210). 성공/실패 비율로 제목을 달리한다.
/// 게시 완료 토스트(`completion_message`)와 같은 UX로, 트레이 상주 중에도 결과를 통지한다.
pub fn login_completion_message(total: usize, ok: usize) -> (String, String) {
    let head = if ok == total {
        "로그인 완료"
    } else if ok == 0 {
        "로그인 실패"
    } else {
        "로그인 일부 실패"
    };
    (
        head.to_string(),
        format!("계정 {total}개 중 {ok}개 로그인 성공"),
    )
}

/// 놓친 예약(앱 종료 중 시각 경과) 토스트 문구(제목, 본문)를 만든다.
pub fn missed_message(newly: usize) -> (String, String) {
    (
        "예약 놓침".to_string(),
        format!("예약 {newly}건이 앱 종료 중 시각이 지나 미발행됐습니다. 큐에서 재예약하거나 취소하세요."),
    )
}

/// OS 토스트를 best-effort로 띄운다. 발송 실패는 비치명적(로깅만) — 인앱 알림이 항상 우선.
pub fn notify_desktop<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(%error, "데스크톱 토스트 발송 실패 — 인앱 알림은 정상");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_title_reflects_all_success() {
        let (head, body) = completion_message("반도체 코멘트", 3, 3);
        assert_eq!(head, "게시 완료");
        assert!(body.contains("3곳 중 3곳"));
        assert!(body.contains("반도체 코멘트"));
    }

    #[test]
    fn completion_title_reflects_partial_failure() {
        let (head, _) = completion_message("x", 3, 1);
        assert_eq!(head, "게시 일부 실패");
    }

    #[test]
    fn completion_title_reflects_total_failure() {
        let (head, _) = completion_message("x", 3, 0);
        assert_eq!(head, "게시 실패");
    }

    #[test]
    fn login_completion_title_reflects_ratio() {
        assert_eq!(login_completion_message(3, 3).0, "로그인 완료");
        assert_eq!(login_completion_message(3, 1).0, "로그인 일부 실패");
        assert_eq!(login_completion_message(3, 0).0, "로그인 실패");
        assert!(login_completion_message(3, 2).1.contains("3개 중 2개"));
    }

    #[test]
    fn missed_message_summarizes_count() {
        let (head, body) = missed_message(2);
        assert_eq!(head, "예약 놓침");
        assert!(body.contains("2건"));
    }
}
