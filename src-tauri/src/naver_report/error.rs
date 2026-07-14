//! 신고(report) 모듈의 오류 타입. HTTP 조회·토큰 획득·제출 각 단계의 실패를 사람이 읽는
//! 메시지로 감싸 오케스트레이션이 계정×링크별 결과에 담게 한다(패닉 없이 실패로 보고).

use std::fmt;

/// 신고 파이프라인 한 단계의 실패. 표시용 메시지를 그대로 담는다(계정×링크 결과의 message로 쓰임).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportError {
    /// 링크에서 itemCode/postId를 뽑지 못함(형식 오류).
    InvalidLink(String),
    /// 저장된 로그인 쿠키가 없거나 만료 — 재로그인 필요.
    NoCookies(String),
    /// 조회(by-item/profile) HTTP 실패 또는 encryptedUserId 미해석.
    Resolve(String),
    /// ncaptcha 토큰 획득 실패(브라우저/CDP) — 이 건은 실패로 보고, 패닉 금지.
    Token(String),
    /// `POST /api/report` 전송 실패 또는 success!=true.
    Submit(String),
}

impl ReportError {
    /// 표시용 메시지 원문(결과 패널·알림에 그대로 노출).
    pub fn message(&self) -> &str {
        match self {
            ReportError::InvalidLink(m)
            | ReportError::NoCookies(m)
            | ReportError::Resolve(m)
            | ReportError::Token(m)
            | ReportError::Submit(m) => m,
        }
    }
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, msg) = match self {
            ReportError::InvalidLink(m) => ("링크 오류", m),
            ReportError::NoCookies(m) => ("쿠키 없음", m),
            ReportError::Resolve(m) => ("대상 조회 실패", m),
            ReportError::Token(m) => ("토큰 획득 실패", m),
            ReportError::Submit(m) => ("신고 제출 실패", m),
        };
        write!(f, "{kind}: {msg}")
    }
}

impl std::error::Error for ReportError {}
