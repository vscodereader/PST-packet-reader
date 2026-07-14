//! ncaptcha 토큰(`ncaptchaTokenId`) 획득 — **유일하게 브라우저가 필요한 단계**(설계서 §3·§4).
//!
//! `cipherText`는 네이버 봇탐지 WASM이 브라우저 안에서 만들어 Rust로 복제 불가하다. 그래서 신고센터
//! srp2 report 페이지를 **보이는 크롬**으로 열어(로그인과 동일 인프라 재사용) SDK가 만든 토큰을
//! CDP `Runtime.evaluate`로 읽어낸다. 크롬은 토큰이 필요한 순간에만 켜고 [`TokenBrowser`]가 drop될
//! 때 프로세스 트리째 종료된다(`ChromeHandle` Drop = `taskkill /T /F`, 잔존 0).
//!
//! ⚠️ 이 추출부는 **실기기 튜닝**이 필요하다: SDK가 토큰을 어느 JS 변수/콜백/네트워크 응답에 두는지는
//! 실제로 한 번 붙여야 확정된다. 아래는 컴파일되고 나머지 파이프라인이 동작하는 best-effort 골격이며,
//! 토큰을 못 얻으면 그 건은 [`ReportError::Token`] 실패로 보고한다(패닉 금지).

use std::thread::sleep;
use std::time::{Duration, Instant};

use super::error::ReportError;
use crate::auth::{launch_debug_chrome, ChromeHandle};
use crate::naver_automation::CdpClient;

/// CDP가 붙는 로컬 DevTools 호스트(포트는 `launch_debug_chrome`가 확정).
const DEVTOOLS_HOST: &str = "127.0.0.1";
/// srp2 신고 페이지가 ncaptcha SDK를 로드·초기화할 때까지 기다리는 상한.
const PAGE_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// 페이지 로드 후 ncaptcha 토큰이 준비될 때까지의 폴링 상한·간격.
const TOKEN_POLL_TIMEOUT: Duration = Duration::from_secs(20);
const TOKEN_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 신고센터 report 페이지 — ncaptcha SDK가 여기서 토큰을 만든다(설계서 §2.1).
const SRP2_REPORT_PAGE: &str = "https://srp2.naver.com/report";

/// srp2 report 페이지에서 ncaptcha 토큰을 뽑는 JS. **실기기 튜닝 지점** — SDK가 토큰을 두는 위치를
/// 확정하면 이 접근 경로만 고치면 된다. 지금은 흔한 후보(전역 변수·hidden input·SDK API)를 관대하게
/// 훑어 문자열을 돌려주고, 없으면 빈 문자열이다.
///
/// TODO(실기기 튜닝): ncaptchaTokenId 추출 셀렉터 — 실제 SDK가 토큰을 노출하는 정확한
/// 변수/콜백/DOM 위치로 교체한다(설계서 §10).
const TOKEN_EXTRACT_JS: &str = r#"
(() => {
  try {
    const g = window;
    const globals = [
      g.ncaptchaTokenId,
      g.__ncaptchaToken,
      g.ncpt && g.ncpt.tokenId,
      g.nid_ncaptcha && g.nid_ncaptcha.token
    ];
    for (const v of globals) {
      if (typeof v === 'string' && v.length > 0) return v;
    }
    const el = document.querySelector(
      'input[name="ncaptchaTokenId"], [data-ncaptcha-token], #ncaptchaTokenId'
    );
    if (el) {
      const v = el.value || el.getAttribute('data-ncaptcha-token') || el.textContent;
      if (v && v.trim()) return v.trim();
    }
  } catch (e) {}
  return '';
})()
"#;

/// 계정 1개의 신고 사이클 동안 열려 있는 보이는 크롬(설계서 §4.2: 계정당 크롬 1회). 이 계정의 링크
/// n개 토큰을 이어서 만들고, drop되면 그 프로세스 트리째 종료된다.
pub struct TokenBrowser {
    client: CdpClient,
    // ChromeHandle Drop이 이 크롬 트리만 taskkill /T /F + wait로 완전 종료(잔존 0). 보관만 하면 된다.
    _handle: ChromeHandle,
}

impl TokenBrowser {
    /// 이 계정용 보이는 크롬을 열고 CDP를 붙인다(Page 도메인만 활성화). 실패하면 토큰을 만들 수 없다.
    pub fn open() -> Result<Self, ReportError> {
        let handle = launch_debug_chrome(false)
            .map_err(|error| ReportError::Token(format!("신고용 크롬 실행 실패: {error}")))?;
        let mut client = match CdpClient::connect_to_existing_chrome(DEVTOOLS_HOST, handle.port) {
            Ok(client) => client,
            Err(error) => {
                // 연결 실패면 방금 띄운 크롬을 즉시 종료한다(잔존 방지).
                drop(handle);
                return Err(ReportError::Token(format!(
                    "신고용 크롬 CDP 연결 실패: {error}"
                )));
            }
        };
        if let Err(error) = client.enable_page_only() {
            drop(handle);
            return Err(ReportError::Token(format!(
                "Page 도메인 활성화 실패: {error}"
            )));
        }
        Ok(Self {
            client,
            _handle: handle,
        })
    }

    /// srp2 report 페이지를 열어 이 링크(글)의 ncaptcha 토큰을 만들어 읽는다. best-effort —
    /// 튜닝 전이라 토큰을 못 얻으면 [`ReportError::Token`]으로 실패를 돌려준다(패닉 없음).
    pub fn acquire_token(&mut self, _post_id: &str) -> Result<String, ReportError> {
        self.client
            .navigate(SRP2_REPORT_PAGE)
            .map_err(|error| ReportError::Token(format!("신고 페이지 이동 실패: {error}")))?;
        self.client
            .wait_for_ready_state(PAGE_READY_TIMEOUT)
            .map_err(|error| ReportError::Token(format!("신고 페이지 로딩 대기 실패: {error}")))?;

        // 토큰이 비동기로 준비되므로 상한까지 폴링한다.
        let deadline = Instant::now() + TOKEN_POLL_TIMEOUT;
        loop {
            match self.client.evaluate_string(TOKEN_EXTRACT_JS) {
                Ok(token) if !token.trim().is_empty() => return Ok(token.trim().to_owned()),
                Ok(_) => {}
                Err(error) => {
                    return Err(ReportError::Token(format!(
                        "토큰 추출 evaluate 실패: {error}"
                    )))
                }
            }
            if Instant::now() >= deadline {
                return Err(ReportError::Token(
                    "ncaptcha 토큰을 얻지 못했습니다(실기기 튜닝 필요 — token.rs TODO)".to_owned(),
                ));
            }
            sleep(TOKEN_POLL_INTERVAL);
        }
    }
}
