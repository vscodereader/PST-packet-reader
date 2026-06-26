//! 네이버 블로그 댓글 등록 오류 타입(#271). 카페/밴드와 동일하게 메인 사유(message)와
//! "자세히 보기" trace(backtrace 포함, #199)를 분리해 보존한다.
//!
//! # 쿠키 보안
//! 쿠키 헤더 값은 사용자의 인증 자격 증명이다. 이 타입은 쿠키 값을 어떤 변형/메시지에도
//! 절대 담지 않는다.

/// 네이버 블로그 댓글 등록 중 발생하는 오류. 종류별 사용자용 한국어 메시지(`message`)와,
/// 생성 지점에서 캡처한 런타임 호출 스택(`trace`)을 함께 들고 다닌다. trace는 UI의
/// "자세히 보기"에 노출되고(앵커 `at file:line` + backtrace), message는 메인 사유로 쓴다.
#[derive(Debug)]
pub struct BlogError {
    /// 사용자용 한 줄 사유. 쿠키 값은 절대 포함되지 않는다.
    message: String,
    /// 실패 지점 앵커(`at file:line:col`) + 캡처된 런타임 호출 스택(#199). 자세히 보기 전용.
    trace: String,
}

impl BlogError {
    /// 사유 메시지로부터 오류를 만들며 호출 스택(앵커 + backtrace)을 캡처한다. **실제 실패
    /// 지점**에서 호출해야 그 지점의 스택이 잡힌다(에러가 `?`로 전파된 뒤에 잡으면 사라진다).
    #[track_caller]
    pub fn new(message: impl Into<String>) -> Self {
        let loc = std::panic::Location::caller();
        let message = message.into();
        Self {
            // 사용자 사유 메시지를 백트레이스 **위**에 먼저 적는다 — 네이버가 준 거절 사유
            // (예: 도배방지 입력 제한)가 메인 한 줄에서 잘려도 "자세히 보기"에서 전문이 보이게 한다.
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

    /// "자세히 보기"용 — 실패 지점 앵커 + 캡처된 런타임 호출 스택.
    pub fn trace(&self) -> &str {
        &self.trace
    }
}

impl std::fmt::Display for BlogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BlogError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_message_and_trace() {
        let err = BlogError::new("댓글 등록 실패");
        assert_eq!(err.message(), "댓글 등록 실패");
        // trace는 앵커(at …)와 backtrace 본문을 합쳐 비어 있지 않다(자세히 보기 노출).
        assert!(err.trace().contains("at "));
        assert!(!err.trace().is_empty());
        // 사용자 사유가 백트레이스(at …) **위**에 먼저 적힌다 — 메인 한 줄이 잘려도 전문 노출.
        assert!(err.trace().starts_with("댓글 등록 실패"));
        let msg_at = err.trace().find("댓글 등록 실패").unwrap();
        let anchor_at = err.trace().find("at ").unwrap();
        assert!(msg_at < anchor_at, "메시지가 앵커보다 앞에 있어야 한다");
    }

    #[test]
    fn display_shows_message_only() {
        let err = BlogError::new("X");
        assert_eq!(format!("{err}"), "X");
    }
}
