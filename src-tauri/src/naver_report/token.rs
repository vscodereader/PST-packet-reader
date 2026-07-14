//! 신고 제출 — **브라우저 구동**(설계서 갱신 2026-07-14). 페이지가 직접 제출하게 둔다.
//!
//! `ncaptchaTokenId`는 `/v2/tokens` 응답이 아니라, 신고 페이지의 ncaptcha SDK(WASM)가 **실제 제출
//! 순간** 만드는 난독화 봇탐지 토큰이다. JS 전역/DOM 어디에도 미리 존재하지 않아 긁어올 수 없다
//! (2026-07-14 수동 vs pstmacro 패킷 대조로 확정). 그래서 우리가 `/api/report`를 쏘는 대신,
//! 로그인된 보이는 크롬으로 신고 페이지를 실제 사용자처럼 열고 사유 선택 → 제출 버튼 클릭을 CDP로
//! 수행한다. 그러면 페이지의 진짜 SDK가 토큰을 만들어 스스로 `POST /api/report`를 쏜다. 결과는 CDP
//! Network 이벤트(요청 바디·응답 바디)로 포착해 `{"success":true}` 여부로 판정한다.
//!
//! 이 경로는 오프라인 검증이 불가능하다. 그래서 프로젝트 규칙(와이어샤크식 원문 로깅)대로 **한 번의
//! 실제 실행이 완전한 진단이 되게** 계측한다: 이동한 URL, 렌더된 사유 DOM, 정확히 무엇을 클릭했는지,
//! `/api/report`의 원문 요청 바디(진짜 ncaptchaTokenId 노출)·응답 바디를 전부 남긴다.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::content_resolver::{build_report_page_url, ReportTarget};
use super::error::ReportError;
use crate::auth::{launch_debug_chrome, ChromeHandle};
use crate::naver_automation::CdpClient;

/// CDP가 붙는 로컬 DevTools 호스트(포트는 `launch_debug_chrome`가 확정).
const DEVTOOLS_HOST: &str = "127.0.0.1";
/// srp2 신고 페이지(SPA + ncaptcha SDK)가 최초 렌더될 때까지 기다리는 상한.
const PAGE_READY_TIMEOUT: Duration = Duration::from_secs(30);
/// `/api/reason` 로딩 → 사유 UI 가 DOM 에 렌더될 때까지의 폴링 상한·간격.
const REASON_RENDER_TIMEOUT: Duration = Duration::from_secs(20);
const REASON_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// 사유 선택 후 SPA 가 제출 버튼을 활성화(리렌더)할 때까지의 여유. React state 반영이 다음 틱이라
/// 같은 evaluate 안에서 선택+제출을 이어붙이면 버튼이 아직 disabled 일 수 있어 나눠서 누른다.
const AFTER_REASON_SETTLE: Duration = Duration::from_millis(900);
/// 제출 클릭 후 페이지 SDK 가 토큰을 만들어 `/api/report`를 쏘고 응답이 올 때까지의 상한.
const SUBMIT_RESULT_TIMEOUT: Duration = Duration::from_secs(30);
/// 신고 제출 요청 — 이 URL 부분일치로 CDP Network 감시자를 무장한다.
const REPORT_API_NEEDLE: &str = "/api/report";

/// 계정 1개의 신고 사이클 동안 열려 있는 보이는 크롬(설계서 §4.2: 계정당 크롬 1회). 이 계정의 링크
/// n개를 이어서 신고하고, drop되면 그 프로세스 트리째 종료된다.
pub struct ReportBrowser {
    client: CdpClient,
    // ChromeHandle Drop이 이 크롬 트리만 taskkill /T /F + wait로 완전 종료(잔존 0). 보관만 하면 된다.
    _handle: ChromeHandle,
}

impl ReportBrowser {
    /// 이 계정용 보이는 크롬을 열고 CDP를 붙인다. 붙인 직후 신고 페이지로 이동하기 **전에** 이 계정의
    /// 네이버 세션 쿠키(`cookies`: name/value 쌍)를 CDP로 주입해 로그인 상태를 만든다 — 비로그인이면
    /// srp2 신고 페이지가 nid 로그인으로 리다이렉트돼 ncaptcha SDK가 뜨지 않는다. `set_naver_cookies`가
    /// Network 도메인도 켜므로(쿠키 주입 겸용) 이후 `/api/report` 요청/응답을 CDP 이벤트로 포착할 수
    /// 있다. 쿠키 값은 자격증명이라 로그에 남기지 않는다. 실패하면 이 계정은 신고할 수 없다.
    pub fn open(cookies: &[(String, String)]) -> Result<Self, ReportError> {
        let handle = launch_debug_chrome(false)
            .map_err(|error| ReportError::Token(format!("신고용 크롬 실행 실패: {error}")))?;
        let mut client = match CdpClient::connect_to_existing_chrome(DEVTOOLS_HOST, handle.port) {
            Ok(client) => client,
            Err(error) => {
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
        // navigate 전에 세션 쿠키 주입(Network 도메인 활성화 + Network.setCookies). 로그인 상태로
        // 신고 페이지가 열려야 SDK/사유 화면이 뜨고, Network.enable 로 /api/report 포착이 가능해진다.
        if let Err(error) = client.set_naver_cookies(cookies) {
            drop(handle);
            return Err(ReportError::Token(format!(
                "신고용 세션 쿠키 주입 실패: {error}"
            )));
        }
        Ok(Self {
            client,
            _handle: handle,
        })
    }

    /// 링크 한 건을 실제 페이지 구동으로 신고한다. 전체 신고 페이지 URL 로 이동 → 사유 UI 렌더 대기 →
    /// 사유 선택 → 제출 클릭 → 페이지 SDK 가 쏜 `POST /api/report`를 CDP Network 이벤트로 포착해
    /// `{"success":true}` 여부로 판정한다. 성공이면 Ok, 아니면 진단 원문을 담은 [`ReportError`].
    pub fn submit_report(
        &mut self,
        content_id: &str,
        target: &ReportTarget,
        reason_code: &str,
        reason_label: &str,
    ) -> Result<(), ReportError> {
        let url = build_report_page_url(content_id, target);
        // 이동한 URL 을 원문으로 남긴다(어느 페이지에서 신고하려 했는지).
        tracing::info!(target: "report", url = %url, "[REPORT-SUBMIT] 신고 페이지 navigate 시작");
        self.client.navigate(&url).map_err(|error| {
            tracing::warn!(target: "report", url = %url, %error, "[REPORT-SUBMIT] navigate 실패");
            ReportError::Submit(format!("신고 페이지 이동 실패: {error}"))
        })?;
        match self.client.wait_for_ready_state(PAGE_READY_TIMEOUT) {
            Ok(()) => tracing::info!(target: "report", "[REPORT-SUBMIT] 페이지 ready 도달"),
            Err(error) => {
                tracing::warn!(target: "report", %error, "[REPORT-SUBMIT] 페이지 ready 대기 실패");
                return Err(ReportError::Submit(format!(
                    "신고 페이지 로딩 대기 실패: {error}"
                )));
            }
        }

        // 사유 UI(SPA + /api/reason)가 렌더될 때까지 폴링한다. 렌더된 사유 컨테이너 원문을 남긴다.
        self.wait_reasons_rendered()?;

        // 제출 클릭 전에 /api/report 감시자를 무장한다(SDK 가 쏘는 요청을 놓치지 않게).
        self.client.arm_network_capture(REPORT_API_NEEDLE);

        // 1) 사유 선택 — 무엇을 찾고 무엇을 클릭했는지 원문 로그.
        let select_js = build_select_reason_js(reason_code, reason_label);
        match self.client.evaluate_string(&select_js) {
            Ok(log) => tracing::info!(target: "report", reason = %reason_code, result = %log, "[REPORT-SUBMIT] 사유 선택 시도 — DOM 원문"),
            Err(error) => {
                tracing::warn!(target: "report", %error, "[REPORT-SUBMIT] 사유 선택 evaluate 실패");
                return Err(ReportError::Submit(format!("사유 선택 실패: {error}")));
            }
        }

        // React state 반영 대기 후 제출 버튼을 누른다(선택 즉시 disabled 일 수 있어 나눠서).
        sleep(AFTER_REASON_SETTLE);
        match self.client.evaluate_string(SUBMIT_CLICK_JS) {
            Ok(log) => tracing::info!(target: "report", result = %log, "[REPORT-SUBMIT] 제출 버튼 클릭 시도 — DOM 원문"),
            Err(error) => {
                tracing::warn!(target: "report", %error, "[REPORT-SUBMIT] 제출 클릭 evaluate 실패");
                return Err(ReportError::Submit(format!("제출 버튼 클릭 실패: {error}")));
            }
        }

        // 페이지가 쏜 /api/report 요청/응답을 CDP Network 이벤트로 기다린다.
        self.await_report_result()
    }

    /// 사유 UI 가 DOM 에 렌더될 때까지 폴링한다(SPA + `/api/reason` 응답 후 렌더). 후보 사유 요소가
    /// 하나 이상 보이면 통과하고, 그 사유 컨테이너 원문(outerHTML, 상한 절삭)을 진단용으로 남긴다.
    /// 상한까지 안 뜨면 실패(원문 로그 포함) — 실기기에서 셀렉터를 확정할 근거가 된다.
    fn wait_reasons_rendered(&mut self) -> Result<(), ReportError> {
        let deadline = Instant::now() + REASON_RENDER_TIMEOUT;
        let mut polls = 0u32;
        loop {
            polls += 1;
            let probe = self
                .client
                .evaluate_string(REASON_PROBE_JS)
                .unwrap_or_default();
            // probe 는 "count|containerOuterHTML(절삭)" 형태. count>0 이면 렌더된 것으로 본다.
            let count = probe
                .split('|')
                .next()
                .and_then(|c| c.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if count > 0 {
                tracing::info!(target: "report", polls, candidate_count = count, dom = %probe, "[REPORT-SUBMIT] 사유 UI 렌더 감지 — DOM 원문");
                return Ok(());
            }
            if Instant::now() >= deadline {
                tracing::warn!(target: "report", polls, dom = %probe, "[REPORT-SUBMIT] 사유 UI 렌더 대기 초과 — 마지막 DOM 원문");
                return Err(ReportError::Submit(
                    "신고 사유 화면이 렌더되지 않았습니다(로그인 만료/캡차/셀렉터 확인 필요 — token.rs)"
                        .to_owned(),
                ));
            }
            sleep(REASON_POLL_INTERVAL);
        }
    }

    /// 무장된 `/api/report` 감시가 끝날 때까지 기다려 결과를 판정한다. 요청 바디 원문(진짜
    /// ncaptchaTokenId 포함)과 응답 바디 원문을 트레이스 OFF 여도 항상 남긴다. `{"success":true}`면
    /// Ok, 아니면 원문을 담은 실패. 요청 자체를 못 잡았으면(SDK 가 안 쐈거나 캡차) 그 사실을 남긴다.
    fn await_report_result(&mut self) -> Result<(), ReportError> {
        let capture = self.client.wait_for_network_capture(SUBMIT_RESULT_TIMEOUT);

        // 요청 바디 원문(있으면) — 여기에 진짜 ncaptchaTokenId 가 담긴다. 항상 남긴다.
        if let Some(body) = capture.request_body.as_deref() {
            tracing::info!(target: "report", body = %body, "[REPORT-SUBMIT] /api/report 요청 바디 원문(진짜 ncaptchaTokenId 포함)");
        }
        let Some(request_id) = capture.request_id.as_deref() else {
            tracing::warn!(
                target: "report",
                "[REPORT-SUBMIT] /api/report 요청을 포착하지 못함 — 페이지가 제출을 쏘지 않았습니다(캡차/선택·버튼 미클릭 의심)"
            );
            return Err(ReportError::Submit(
                "신고 요청이 전송되지 않았습니다(사유/제출 클릭이 먹지 않았거나 캡차 — 위 DOM 로그 확인)"
                    .to_owned(),
            ));
        };
        if let Some(failed) = capture.failed.as_deref() {
            tracing::warn!(target: "report", error = %failed, "[REPORT-SUBMIT] /api/report 로딩 실패");
            return Err(ReportError::Submit(format!("신고 요청 실패: {failed}")));
        }

        // 응답 바디 원문을 읽어 성공을 판정한다(loadingFinished 이후라 읽을 수 있다).
        let text = match self.client.network_get_response_body(request_id) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(target: "report", status = ?capture.status, %error, "[REPORT-SUBMIT] 응답 바디 읽기 실패");
                return Err(ReportError::Submit(format!(
                    "신고 응답 바디를 읽지 못했습니다(status={:?}): {error}",
                    capture.status
                )));
            }
        };
        // 응답 원문은 트레이스 OFF 여도 항상 남긴다(성공/거부 사유가 여기 담긴다).
        tracing::info!(target: "report", status = ?capture.status, body = %text, "[REPORT-SUBMIT] /api/report 응답 바디 원문");
        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if parsed.get("success").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        Err(ReportError::Submit(format!(
            "신고 실패(status={:?}): {text}",
            capture.status
        )))
    }
}

/// 사유 선택 JS 를 만든다. `code`(예: `AA29`)와 `label`(예: `스팸홍보/도배입니다`)를 안전하게 JSON
/// 임베드해, 동적 SPA 의 라이브 DOM 을 **일반적으로** 훑어 사유 요소를 찾아 클릭한다(정확한 셀렉터를
/// 하드코딩할 수 없다). 찾은/클릭한 것을 전부 JSON 문자열로 돌려준다(원문 로깅용).
fn build_select_reason_js(code: &str, label: &str) -> String {
    // serde_json 직렬화는 &str 에 대해 실패하지 않는다(따옴표/유니코드 안전 이스케이프).
    let code_js = serde_json::to_string(code).unwrap_or_else(|_| "\"\"".to_owned());
    let label_js = serde_json::to_string(label).unwrap_or_else(|_| "\"\"".to_owned());
    format!(
        r#"
(() => {{
  const out = {{ code: {code_js}, label: {label_js}, strategy: null, clicked: false,
                 target: null, candidates: [] }};
  try {{
    const code = {code_js};
    // 라벨 정규화(뒤 마침표/공백 제거) — /api/reason text 는 "...입니다." 처럼 마침표가 붙는다.
    const norm = s => (s || '').replace(/\s+/g, ' ').replace(/[.\s]+$/, '').trim();
    const wantLabel = norm({label_js});

    const clickEl = el => {{
      try {{
        el.scrollIntoView({{ block: 'center' }});
        el.click();
        if (el.tagName === 'INPUT') {{
          el.checked = true;
          el.dispatchEvent(new Event('input', {{ bubbles: true }}));
          el.dispatchEvent(new Event('change', {{ bubbles: true }}));
        }}
        return true;
      }} catch (e) {{ return false; }}
    }};
    const desc = el => {{
      if (!el) return null;
      const h = (el.outerHTML || '').slice(0, 300);
      return {{ tag: el.tagName, text: (el.textContent || '').trim().slice(0, 80), html: h }};
    }};

    // 1) code 로 직접 매칭(value / 각종 data-* 속성).
    let el = document.querySelector(
      'input[value="' + code + '"], [data-code="' + code + '"], [data-value="' + code + '"], ' +
      '[data-reason-code="' + code + '"], [data-reason="' + code + '"], [value="' + code + '"]'
    );
    if (el) out.strategy = 'code-attr';

    // 2) 라벨 텍스트 매칭 — 라디오/라벨/리스트/버튼 등 클릭 가능한 후보를 넓게 훑는다.
    const clickable = Array.from(document.querySelectorAll(
      'label, li, button, [role="radio"], [role="option"], [role="button"], a, div, span'
    ));
    for (const c of clickable) {{
      const t = norm(c.textContent);
      if (t && wantLabel && (t === wantLabel || t.indexOf(wantLabel) !== -1) && t.length < 120) {{
        out.candidates.push(desc(c));
      }}
    }}
    if (!el && wantLabel) {{
      // 라벨 텍스트가 정확히 일치하거나 포함하는 가장 작은(가장 구체적인) 요소를 고른다.
      let best = null;
      for (const c of clickable) {{
        const t = norm(c.textContent);
        if (!t) continue;
        if (t === wantLabel || (t.indexOf(wantLabel) !== -1 && t.length <= wantLabel.length + 40)) {{
          if (!best || (c.textContent || '').length < (best.textContent || '').length) best = c;
        }}
      }}
      if (best) {{ el = best; out.strategy = 'label-text'; }}
    }}

    if (el) {{
      // input 이면 연결된 label(for=id) 도 함께 눌러 확실히 선택되게 한다.
      let clickTarget = el;
      if (el.tagName === 'INPUT' && el.id) {{
        const lab = document.querySelector('label[for="' + el.id + '"]');
        if (lab) clickTarget = lab;
      }}
      out.target = desc(el);
      out.clicked = clickEl(clickTarget) || clickEl(el);
    }}
  }} catch (e) {{ out.error = String(e); }}
  return JSON.stringify(out);
}})()
"#
    )
}

/// 제출/확인 버튼을 찾아 클릭하는 JS. 동적 SPA 라 정확한 셀렉터를 하드코딩할 수 없어, 한국어 텍스트
/// (신고/신고하기/확인/제출/완료)나 `type=submit` 을 일반적으로 훑어 1차 액션 버튼을 누르고, 취소/닫기
/// 류는 배제한다. 찾은/클릭한 것을 JSON 으로 돌려준다(원문 로깅용).
const SUBMIT_CLICK_JS: &str = r#"
(() => {
  const out = { clicked: false, target: null, candidates: [] };
  try {
    const norm = s => (s || '').replace(/\s+/g, ' ').trim();
    const desc = el => el ? { tag: el.tagName, text: norm(el.textContent).slice(0, 40),
                              type: el.getAttribute('type'), disabled: el.disabled === true,
                              html: (el.outerHTML || '').slice(0, 200) } : null;
    const POS = /(신고하기|신고|제출|확인|완료|접수)/;
    const NEG = /(취소|닫기|이전|뒤로|cancel|close)/i;

    const buttons = Array.from(document.querySelectorAll(
      'button, [role="button"], input[type="submit"], input[type="button"], a'
    ));
    let pick = null;
    for (const b of buttons) {
      const t = norm(b.textContent) || norm(b.value) || '';
      const isSubmit = (b.getAttribute('type') === 'submit');
      const match = isSubmit || (POS.test(t) && !NEG.test(t));
      if (match) {
        out.candidates.push(desc(b));
        if (!pick && b.disabled !== true) pick = b;
      }
    }
    if (!pick) {
      // 활성 후보가 없으면 첫 후보라도(대개 disabled) — 로그로 "왜 못 눌렀나"가 보이게.
      const c = buttons.find(b => { const t = norm(b.textContent) || norm(b.value) || '';
        return (b.getAttribute('type') === 'submit') || (POS.test(t) && !NEG.test(t)); });
      if (c) pick = c;
    }
    if (pick) {
      out.target = desc(pick);
      try { pick.scrollIntoView({ block: 'center' }); pick.click(); out.clicked = true; }
      catch (e) { out.error = String(e); }
    }
  } catch (e) { out.error = String(e); }
  return JSON.stringify(out);
})()
"#;

/// 사유 UI 렌더 감지 프로브 JS. 사유처럼 보이는 후보(라디오/사유코드 data-* / label·li 텍스트)의 개수와
/// 사유 컨테이너 outerHTML(절삭)을 `"count|html"` 로 돌려준다. count>0 이면 렌더된 것으로 본다.
const REASON_PROBE_JS: &str = r#"
(() => {
  try {
    const radios = document.querySelectorAll(
      'input[type="radio"], [role="radio"], [data-code], [data-reason-code], [data-reason]'
    );
    let count = radios.length;
    if (count === 0) {
      const labels = Array.from(document.querySelectorAll('label, li'))
        .filter(e => /입니다|표현|스팸|음란|불법|개인정보|청소년/.test(e.textContent || ''));
      count = labels.length;
    }
    let container = document.querySelector('#app') || document.body;
    const firstRadio = document.querySelector('input[type="radio"], [role="radio"]');
    if (firstRadio) {
      let p = firstRadio;
      for (let i = 0; i < 3 && p.parentElement; i++) p = p.parentElement;
      container = p;
    }
    const html = (container.outerHTML || '').slice(0, 1500);
    return count + '|' + html;
  } catch (e) { return '0|' + String(e); }
})()
"#;
