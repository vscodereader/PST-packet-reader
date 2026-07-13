//! band_post 오류 타입. 쿠키/secretKey 등 자격 증명은 어떤 변형에도 담지 않는다.

use super::response::BandApiError;

/// band 가입·글쓰기·댓글 수행 중 발생하는 오류의 종류.
#[derive(Debug)]
pub enum BandPostErrorKind {
    /// 입력 링크에서 band_no를 추출하지 못함.
    InvalidLink(String),
    /// 저장된 band 쿠키가 없거나 만료됨(재로그인 필요).
    NoSession,
    /// getKey 응답에서 secretKey를 얻지 못함. 진단용 상세(HTTP 상태/응답 앞부분)를 담는다.
    NoSecretKey(String),
    /// HTTP 전송 계층 오류(연결/타임아웃 등). 쿠키 값 미포함.
    Transport(String),
    /// non-2xx HTTP 응답.
    Http { status: u16, body: String },
    /// band api가 `result_code != 1` 반환.
    Api(BandApiError),
}

/// band 오류 = 종류([`BandPostErrorKind`]) + 생성 지점에서 캡처한 런타임 호출 스택(#199).
///
/// 카페·종토방과 동일하게 메인 사유(`band_failure_reason`)와 "자세히 보기" trace를 나누되,
/// trace에 이 `backtrace`를 실어 실패 지점의 호출 스택을 보여준다. 밴드 실패는 panic이 아니라
/// 에러 값으로 흐르므로, [`BandPostError::new`]를 **실제 실패 지점**에서 호출해 스택을 잡는다
/// (에러가 `?`로 전파된 뒤에 잡으면 그 지점 스택이 사라진다).
#[derive(Debug)]
pub struct BandPostError {
    pub kind: BandPostErrorKind,
    pub backtrace: String,
}

impl BandPostError {
    /// 종류로부터 오류를 만들며 호출 스택을 캡처한다.
    pub fn new(kind: BandPostErrorKind) -> Self {
        Self {
            backtrace: crate::util::backtrace_string(),
            kind,
        }
    }

    pub fn invalid_link(link: impl Into<String>) -> Self {
        Self::new(BandPostErrorKind::InvalidLink(link.into()))
    }

    pub fn no_session() -> Self {
        Self::new(BandPostErrorKind::NoSession)
    }

    pub fn no_secret_key(detail: impl Into<String>) -> Self {
        Self::new(BandPostErrorKind::NoSecretKey(detail.into()))
    }

    pub fn transport(detail: impl Into<String>) -> Self {
        Self::new(BandPostErrorKind::Transport(detail.into()))
    }

    pub fn http(status: u16, body: impl Into<String>) -> Self {
        Self::new(BandPostErrorKind::Http {
            status,
            body: body.into(),
        })
    }
}

impl std::fmt::Display for BandPostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            BandPostErrorKind::InvalidLink(link) => {
                write!(f, "밴드 링크에서 밴드 번호를 찾지 못했습니다: {link}")
            }
            BandPostErrorKind::NoSession => {
                write!(
                    f,
                    "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요."
                )
            }
            BandPostErrorKind::NoSecretKey(detail) => {
                write!(f, "밴드 서명 키 발급에 실패했습니다(getKey). {detail}")
            }
            BandPostErrorKind::Transport(msg) => write!(f, "HTTP 전송 오류: {msg}"),
            BandPostErrorKind::Http { status, body } => {
                write!(f, "HTTP {status} 응답: {}", truncate(body, 300))
            }
            BandPostErrorKind::Api(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BandPostError {}

impl From<BandApiError> for BandPostError {
    fn from(err: BandApiError) -> Self {
        Self::new(BandPostErrorKind::Api(err))
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_expected_kind() {
        assert!(
            matches!(BandPostError::invalid_link("L").kind, BandPostErrorKind::InvalidLink(ref s) if s == "L")
        );
        assert!(matches!(
            BandPostError::no_session().kind,
            BandPostErrorKind::NoSession
        ));
        assert!(
            matches!(BandPostError::no_secret_key("d").kind, BandPostErrorKind::NoSecretKey(ref s) if s == "d")
        );
        assert!(
            matches!(BandPostError::transport("t").kind, BandPostErrorKind::Transport(ref s) if s == "t")
        );
        assert!(
            matches!(BandPostError::http(404, "b").kind, BandPostErrorKind::Http { status: 404, ref body } if body == "b")
        );
    }

    #[test]
    fn display_messages_include_context() {
        assert!(BandPostError::invalid_link("https://x")
            .to_string()
            .contains("https://x"));
        assert!(BandPostError::no_session()
            .to_string()
            .contains("밴드 로그인"));
        assert!(BandPostError::no_secret_key("getKey 500")
            .to_string()
            .contains("getKey 500"));
        assert!(BandPostError::transport("timeout")
            .to_string()
            .contains("timeout"));
        let http = BandPostError::http(503, "boom").to_string();
        assert!(http.contains("503") && http.contains("boom"));
    }

    #[test]
    fn http_display_truncates_long_body() {
        let shown = BandPostError::http(500, "x".repeat(500)).to_string();
        // 본문은 300자에서 '…'로 잘린다: 원문 500개보다 짧고 301연속은 남지 않는다.
        assert!(shown.contains('…'));
        assert!(!shown.contains(&"x".repeat(301)));
    }

    #[test]
    fn from_band_api_error_wraps_as_api_kind() {
        let api = BandApiError {
            result_code: Some(3),
            message: "nope".into(),
        };
        let err: BandPostError = api.into();
        assert!(matches!(err.kind, BandPostErrorKind::Api(_)));
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn truncate_keeps_short_and_cuts_long() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abc", 3), "abc"); // 경계값(==max): 자르지 않음
        assert_eq!(truncate("abcdef", 3), "abc…");
    }
}
