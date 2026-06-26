//! 네이버 클립 댓글 게시 오류 타입(#클립). 카페/블로그/밴드와 동일하게 메인 사유(message)와
//! "자세히 보기" trace(backtrace 포함, #199)를 분리해 보존한다. 메시지는 백트레이스 **위**에
//! 먼저 적어, 메인 한 줄에서 잘려도 자세히 보기에서 전문이 보이게 한다(블로그와 동일 철학).
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 타입은 쿠키 값을 어떤 변형/메시지에도
//! 절대 담지 않는다.

/// 네이버 클립 댓글 게시 중 발생하는 오류. 사용자용 한국어 메시지(`message`)와, 생성 지점에서
/// 캡처한 런타임 호출 스택(`trace`)을 함께 들고 다닌다.
#[derive(Debug)]
pub struct ClipError {
    message: String,
    trace: String,
}

impl ClipError {
    /// 사유 메시지로부터 오류를 만들며 호출 스택(앵커 + backtrace)을 캡처한다. **실제 실패
    /// 지점**에서 호출해야 그 지점의 스택이 잡힌다.
    #[track_caller]
    pub fn new(message: impl Into<String>) -> Self {
        let loc = std::panic::Location::caller();
        let message = message.into();
        Self {
            // 사용자 사유를 백트레이스 위에 먼저 적는다(블로그/로그인과 동일).
            trace: format!(
                "{}\n\nat {}:{}:{}\n\n{}",
                message,
                loc.file(),
                loc.line(),
                loc.column(),
                crate::util::backtrace_string(),
            ),
            message,
        }
    }

    /// 사용자용 한 줄 오류 메시지(쿠키 미포함).
    pub fn message(&self) -> &str {
        &self.message
    }

    /// "자세히 보기"용 — 사유 + 실패 지점 앵커 + 캡처된 런타임 호출 스택.
    pub fn trace(&self) -> &str {
        &self.trace
    }
}

impl std::fmt::Display for ClipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClipError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_message_and_trace_with_message_first() {
        let err = ClipError::new("댓글 등록 실패");
        assert_eq!(err.message(), "댓글 등록 실패");
        assert!(err.trace().contains("at "));
        // 사용자 사유가 백트레이스(at …) 위에 먼저 적힌다.
        assert!(err.trace().starts_with("댓글 등록 실패"));
        let msg_at = err.trace().find("댓글 등록 실패").unwrap();
        let anchor_at = err.trace().find("at ").unwrap();
        assert!(msg_at < anchor_at);
    }

    #[test]
    fn display_shows_message_only() {
        let err = ClipError::new("X");
        assert_eq!(format!("{err}"), "X");
    }
}
