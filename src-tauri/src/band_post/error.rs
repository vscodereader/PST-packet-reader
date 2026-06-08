//! band_post 오류 타입. 쿠키/secretKey 등 자격 증명은 어떤 변형에도 담지 않는다.

use super::response::BandApiError;

/// band 가입·글쓰기·댓글 수행 중 발생하는 오류.
#[derive(Debug)]
pub enum BandPostError {
    /// 입력 링크에서 band_no를 추출하지 못함.
    InvalidLink(String),
    /// 저장된 band 쿠키가 없거나 만료됨(재로그인 필요).
    NoSession,
    /// getKey 응답에서 secretKey를 얻지 못함.
    NoSecretKey,
    /// HTTP 전송 계층 오류(연결/타임아웃 등). 쿠키 값 미포함.
    Transport(String),
    /// non-2xx HTTP 응답.
    Http { status: u16, body: String },
    /// band api가 `result_code != 1` 반환.
    Api(BandApiError),
}

impl std::fmt::Display for BandPostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BandPostError::InvalidLink(link) => {
                write!(f, "밴드 링크에서 밴드 번호를 찾지 못했습니다: {link}")
            }
            BandPostError::NoSession => {
                write!(f, "밴드 로그인 세션이 없습니다. 먼저 밴드 로그인을 해주세요.")
            }
            BandPostError::NoSecretKey => {
                write!(f, "밴드 서명 키 발급에 실패했습니다(getKey).")
            }
            BandPostError::Transport(msg) => write!(f, "HTTP 전송 오류: {msg}"),
            BandPostError::Http { status, body } => {
                write!(f, "HTTP {status} 응답: {}", truncate(body, 300))
            }
            BandPostError::Api(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for BandPostError {}

impl From<BandApiError> for BandPostError {
    fn from(err: BandApiError) -> Self {
        BandPostError::Api(err)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
