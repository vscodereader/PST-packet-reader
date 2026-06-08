//! band.us 이메일 로그인 CDP 시퀀스(네이버 `auth/login_flow.rs` 미러).
//!
//! 2단계: 이메일 입력 → 확인 → 비밀번호 입력 → 확인. 비밀번호는 band 페이지 JS가
//! 클라이언트측에서 암호화하므로, `Runtime.evaluate`로 값만 꽂지 않고 실제 키 이벤트
//! (`Input.dispatchKeyEvent`)로 한 글자씩 입력한다(키 후킹 암호화 대응).

use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::naver_automation::{AutomationError, CdpClient};

const EMAIL_LOGIN_URL: &str = "https://auth.band.us/email_login?keep_login=false";
const PASSWORD_URL_FRAGMENT: &str = "email_login/password";
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(40);
const HEADED_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

// 봇탐지 완화용 스텔스 스크립트(네이버와 동일). CDP 제어 흔적인 navigator.webdriver 를
// 일반 크롬과 동일하게 false 로 맞춘다.
const STEALTH_INIT_JS: &str = "Object.defineProperty(navigator,'webdriver',{get:()=>false});";

const EMAIL_SELECTOR: &str = "#input_email";
const EMAIL_SUBMIT_SELECTOR: &str = "#email_login_form button[type=submit]";
const PASSWORD_SELECTOR: &str = "#pw";
const PASSWORD_SUBMIT_SELECTOR: &str = "#email_password_login_form button[type=submit]";

/// 로그인 결과.
pub(crate) enum BandLoginOutcome {
    Ok { cookies: Vec<Value> },
    BadCredentials,
    Blocked,
    Error(String),
}

/// 페이지 판정 신호(분류 입력). 분류 로직을 브라우저에서 분리해 단위 테스트한다.
/// band 로그인은 캡차/otp/device 챌린지가 없으므로 logged_in/bad_credentials/blocked 만 둔다.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BandPageSignals {
    pub logged_in: bool,
    pub bad_credentials: bool,
    pub blocked: bool,
}

/// 진행 중/확정 신호.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BandSignal {
    Pending,
    Success,
    BadCredentials,
    Blocked,
}

/// 폴링 한 스텝의 판정 결과. 루프는 이 값을 실제 동작(반환/대기)으로 옮긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopDecision {
    Success,
    ConfirmedBad,
    ConfirmedBlocked,
    KeepWaiting(Option<BandSignal>),
}

/// 페이지 신호를 로그인 진행/결과 신호로 분류한다(순수 함수).
/// 우선순위: logged_in > blocked > bad_credentials > pending.
pub(crate) fn classify(signals: &BandPageSignals) -> BandSignal {
    if signals.logged_in {
        BandSignal::Success
    } else if signals.blocked {
        BandSignal::Blocked
    } else if signals.bad_credentials {
        BandSignal::BadCredentials
    } else {
        BandSignal::Pending
    }
}

/// 직전 음성 신호와 현재 신호로 이번 폴링의 동작을 결정한다(순수 함수).
///
/// 핵심: BadCredentials/Blocked는 **2회 연속**일 때만 확정한다(과도기 깜빡임 latch 방지).
/// Success/Pending은 음성 누적을 초기화한다. `wait_for_human`(headed)이면 Blocked는
/// 사용자가 직접 처리하도록 단정하지 않고 계속 대기한다.
fn decide_loop_step(
    last_negative: Option<BandSignal>,
    signal: BandSignal,
    wait_for_human: bool,
) -> LoopDecision {
    match signal {
        BandSignal::Success => LoopDecision::Success,
        BandSignal::BadCredentials => {
            if last_negative == Some(BandSignal::BadCredentials) {
                LoopDecision::ConfirmedBad
            } else {
                LoopDecision::KeepWaiting(Some(BandSignal::BadCredentials))
            }
        }
        BandSignal::Blocked => {
            if wait_for_human {
                LoopDecision::KeepWaiting(last_negative)
            } else if last_negative == Some(BandSignal::Blocked) {
                LoopDecision::ConfirmedBlocked
            } else {
                LoopDecision::KeepWaiting(Some(BandSignal::Blocked))
            }
        }
        BandSignal::Pending => LoopDecision::KeepWaiting(None),
    }
}

/// 자격증명이 로그인 시도에 충분한지(둘 다 비어있지 않은지) 검사한다(순수 함수).
pub(crate) fn credentials_present(id: &str, pw: &str) -> bool {
    !id.trim().is_empty() && !pw.is_empty()
}

/// 수거한 쿠키에 band 세션 쿠키 `band_session`이 있는지 확인한다(순수 함수).
///
/// band 로그인 성공 시 `auth.band.us/email_login/password` 응답이 `band_session`
/// 쿠키(domain `.band.us`)를 발급한다. (`BUC`는 네이버 쿠키이며 band은 발급하지 않는다 —
/// 패킷 캡처로 확인.)
pub(crate) fn has_band_session_cookies(cookies: &[Value]) -> bool {
    cookies
        .iter()
        .filter_map(|c| c.get("name").and_then(Value::as_str))
        .any(|name| name == "band_session")
}

/// 로그인을 수행하고 결과를 분류해 반환한다.
pub(crate) fn run(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
) -> BandLoginOutcome {
    match run_inner(client, id, pw, wait_for_human) {
        Ok(outcome) => outcome,
        Err(error) => BandLoginOutcome::Error(error.to_string()),
    }
}

fn run_inner(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
) -> Result<BandLoginOutcome, AutomationError> {
    // 빈/공백 자격증명이면 브라우저 폼을 건드리지 않고 즉시 입력 실패로 중단한다.
    if !credentials_present(id, pw) {
        return Ok(BandLoginOutcome::Error(
            "아이디 또는 비밀번호가 비어 있어 로그인을 시도하지 않았습니다.".to_owned(),
        ));
    }

    // navigate 전에 스텔스 스크립트를 등록한다(best-effort).
    let _ = client.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_INIT_JS }),
    );

    // --- 1단계: 이메일 페이지 ---
    client.navigate(EMAIL_LOGIN_URL)?;
    if !wait_for_visible(client, EMAIL_SELECTOR) {
        return Ok(BandLoginOutcome::Error(
            "band 이메일 입력 폼(#input_email)을 찾지 못했습니다.".to_owned(),
        ));
    }
    if !type_into(client, EMAIL_SELECTOR, id)? {
        return Ok(BandLoginOutcome::Error(
            "이메일 자동 입력에 실패했습니다(이메일 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(1));
    click_button(client, EMAIL_SUBMIT_SELECTOR)?;

    // --- 2단계: 비밀번호 페이지 ---
    if !wait_for_password_page(client) {
        return Ok(BandLoginOutcome::Error(
            "band 비밀번호 입력 폼(#pw)을 찾지 못했습니다. 이메일이 올바른지 확인하세요."
                .to_owned(),
        ));
    }
    if !type_into(client, PASSWORD_SELECTOR, pw)? {
        return Ok(BandLoginOutcome::Error(
            "비밀번호 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(2));
    click_button(client, PASSWORD_SUBMIT_SELECTOR)?;

    // --- 결과 폴링 ---
    let timeout = if wait_for_human {
        HEADED_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    sleep(POLL_INTERVAL);

    let mut last_negative: Option<BandSignal> = None;
    let deadline = Instant::now() + timeout;
    loop {
        let signals = read_signals(client)?;
        match decide_loop_step(last_negative, classify(&signals), wait_for_human) {
            LoopDecision::Success => {
                let cookies = collect_band_cookies(client)?;
                return Ok(BandLoginOutcome::Ok { cookies });
            }
            LoopDecision::ConfirmedBad => return Ok(BandLoginOutcome::BadCredentials),
            LoopDecision::ConfirmedBlocked => return Ok(BandLoginOutcome::Blocked),
            LoopDecision::KeepWaiting(next) => last_negative = next,
        }

        if Instant::now() >= deadline {
            let url = client.current_url().unwrap_or_default();
            return Ok(BandLoginOutcome::Error(format!(
                "로그인 시간이 초과되었습니다. 마지막 페이지: {url}"
            )));
        }
        sleep(POLL_INTERVAL);
    }
}

// 셀렉터 요소가 화면에 보이고 입력 가능(disabled 아님)할 때까지 기다린다.
fn wait_for_visible(client: &mut CdpClient, selector: &str) -> bool {
    tracing::info!("[BAND] 입력 폼 로딩 대기 중... ({selector})");
    let deadline = Instant::now() + Duration::from_secs(15);
    let ready_expr = format!(
        "(()=>{{if(document.readyState!=='complete')return false;\
         const el=document.querySelector('{selector}');\
         return !!(el&&el.offsetParent!==null&&!el.disabled);}})()"
    );
    loop {
        if client.evaluate_bool(&ready_expr).unwrap_or(false) {
            tracing::info!("[BAND] ✓ 입력 폼 로딩 확인 ({selector})");
            return true;
        }
        if Instant::now() >= deadline {
            tracing::info!("[BAND] ✗ 입력 폼 로딩 시간 초과(15초) ({selector})");
            return false;
        }
        sleep(Duration::from_millis(250));
    }
}

// 비밀번호 페이지가 로드될 때까지 기다린다: URL이 password 페이지로 바뀌고 #pw가 입력 가능.
fn wait_for_password_page(client: &mut CdpClient) -> bool {
    tracing::info!("[BAND] 비밀번호 페이지 대기 중...");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let url = client.current_url().unwrap_or_default();
        if url.contains(PASSWORD_URL_FRAGMENT)
            && client
                .evaluate_bool(
                    "(()=>{const el=document.querySelector('#pw');\
                     return !!(el&&el.offsetParent!==null&&!el.disabled);})()",
                )
                .unwrap_or(false)
        {
            tracing::info!("[BAND] ✓ 비밀번호 페이지 로딩 확인");
            return true;
        }
        if Instant::now() >= deadline {
            tracing::info!("[BAND] ✗ 비밀번호 페이지 로딩 시간 초과(15초)");
            return false;
        }
        sleep(Duration::from_millis(250));
    }
}

// 한 글자에 대응하는 US 키보드 물리키 정보(네이버 미러).
struct KeyInfo {
    code: String,
    vk: u32,
    shift: bool,
}

// 문자를 US 키보드 배열의 (code, windowsVirtualKeyCode, Shift 필요 여부)로 매핑한다(순수 함수).
fn key_info(ch: char) -> KeyInfo {
    if ch.is_ascii_alphabetic() {
        let upper = ch.to_ascii_uppercase();
        return KeyInfo {
            code: format!("Key{upper}"),
            vk: upper as u32,
            shift: ch.is_ascii_uppercase(),
        };
    }
    if ch.is_ascii_digit() {
        return KeyInfo {
            code: format!("Digit{ch}"),
            vk: ch as u32,
            shift: false,
        };
    }
    let (code, vk, shift): (&str, u32, bool) = match ch {
        ')' => ("Digit0", 0x30, true),
        '!' => ("Digit1", 0x31, true),
        '@' => ("Digit2", 0x32, true),
        '#' => ("Digit3", 0x33, true),
        '$' => ("Digit4", 0x34, true),
        '%' => ("Digit5", 0x35, true),
        '^' => ("Digit6", 0x36, true),
        '&' => ("Digit7", 0x37, true),
        '*' => ("Digit8", 0x38, true),
        '(' => ("Digit9", 0x39, true),
        ' ' => ("Space", 0x20, false),
        '-' => ("Minus", 0xBD, false),
        '_' => ("Minus", 0xBD, true),
        '=' => ("Equal", 0xBB, false),
        '+' => ("Equal", 0xBB, true),
        '[' => ("BracketLeft", 0xDB, false),
        '{' => ("BracketLeft", 0xDB, true),
        ']' => ("BracketRight", 0xDD, false),
        '}' => ("BracketRight", 0xDD, true),
        '\\' => ("Backslash", 0xDC, false),
        '|' => ("Backslash", 0xDC, true),
        ';' => ("Semicolon", 0xBA, false),
        ':' => ("Semicolon", 0xBA, true),
        '\'' => ("Quote", 0xDE, false),
        '"' => ("Quote", 0xDE, true),
        ',' => ("Comma", 0xBC, false),
        '<' => ("Comma", 0xBC, true),
        '.' => ("Period", 0xBE, false),
        '>' => ("Period", 0xBE, true),
        '/' => ("Slash", 0xBF, false),
        '?' => ("Slash", 0xBF, true),
        '`' => ("Backquote", 0xC0, false),
        '~' => ("Backquote", 0xC0, true),
        _ => ("", 0, false),
    };
    KeyInfo {
        code: code.to_owned(),
        vk,
        shift,
    }
}

// 글자 사이 사람 같은 타이핑 지연 범위(ms).
const TYPE_DELAY_MIN_MS: u64 = 60;
const TYPE_DELAY_MAX_MS: u64 = 180;

// 시드+인덱스로 [MIN, MAX] 범위의 타이핑 지연(ms)을 정하는 순수 함수(splitmix64 혼합).
fn type_delay_ms(seed: u64, index: usize) -> u64 {
    let mut x = seed ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    TYPE_DELAY_MIN_MS + (x % (TYPE_DELAY_MAX_MS - TYPE_DELAY_MIN_MS + 1))
}

// 타이핑 지연 시드(타이핑 호출마다 한 번 — 실행마다 패턴이 달라지게).
fn jitter_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

// evaluate가 돌려준 `[x, y]`(returnByValue) 배열을 좌표로 파싱한다(없으면 None).
fn parse_xy(value: &Value) -> Option<(f64, f64)> {
    let arr = value.as_array()?;
    Some((arr.first()?.as_f64()?, arr.get(1)?.as_f64()?))
}

// 셀렉터 요소의 뷰포트 중심 좌표(CSS px)를 구한다. 없거나 크기 0이면 None.
fn element_center(
    client: &mut CdpClient,
    selector: &str,
) -> Result<Option<(f64, f64)>, AutomationError> {
    let expr = format!(
        "(()=>{{const e=document.querySelector('{selector}');\
         if(!e)return null;const r=e.getBoundingClientRect();\
         if(r.width<=0||r.height<=0)return null;\
         return [r.left+r.width/2, r.top+r.height/2];}})()"
    );
    Ok(parse_xy(&client.evaluate(&expr)?))
}

// (x,y)로 마우스를 옮겨 좌클릭한다 — 진짜 mouse 이벤트로 행동 기반 봇탐지를 완화한다.
fn mouse_click(client: &mut CdpClient, x: f64, y: f64) -> Result<(), AutomationError> {
    client.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseMoved", "x": x, "y": y, "buttons": 0 }),
    )?;
    client.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1 }),
    )?;
    client.call(
        "Input.dispatchMouseEvent",
        json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1 }),
    )?;
    Ok(())
}

// 셀렉터를 마우스로 클릭(=포커스). 좌표를 못 구하면 false(호출부가 JS focus로 폴백).
fn mouse_click_selector(client: &mut CdpClient, selector: &str) -> Result<bool, AutomationError> {
    if let Some((x, y)) = element_center(client, selector)? {
        mouse_click(client, x, y)?;
        return Ok(true);
    }
    Ok(false)
}

// 제출 버튼을 사람처럼 좌표 클릭한다. 좌표를 못 구하면 .click()으로 폴백.
fn click_button(client: &mut CdpClient, selector: &str) -> Result<(), AutomationError> {
    if !mouse_click_selector(client, selector)? {
        let click = format!(
            "(()=>{{const b=document.querySelector('{selector}');\
             if(b){{b.click();return true;}}return false;}})()"
        );
        client.evaluate(&click)?;
    }
    Ok(())
}

// 선택자를 마우스로 클릭해 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(키 후킹 암호화
// 대응). 입력 후 필드 값 길이를 확인해 비어 있으면 최대 3회 재시도한다.
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<bool, AutomationError> {
    let expected = text.chars().count();
    let seed = jitter_seed();

    for _ in 0..3 {
        let clear = format!(
            "(()=>{{const el=document.querySelector('{selector}');\
             if(el){{el.value='';return true;}}return false;}})()"
        );
        client.evaluate(&clear)?;
        if !mouse_click_selector(client, selector)? {
            let focus = format!(
                "(()=>{{const el=document.querySelector('{selector}');\
                 if(el){{el.focus();return true;}}return false;}})()"
            );
            client.evaluate(&focus)?;
        }

        for (i, ch) in text.chars().enumerate() {
            let s = ch.to_string();
            let k = key_info(ch);
            let modifiers = if k.shift { 8 } else { 0 };
            client.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyDown",
                    "text": s,
                    "key": s,
                    "code": k.code,
                    "windowsVirtualKeyCode": k.vk,
                    "nativeVirtualKeyCode": k.vk,
                    "modifiers": modifiers,
                }),
            )?;
            client.call(
                "Input.dispatchKeyEvent",
                json!({
                    "type": "keyUp",
                    "key": s,
                    "code": k.code,
                    "windowsVirtualKeyCode": k.vk,
                    "nativeVirtualKeyCode": k.vk,
                    "modifiers": modifiers,
                }),
            )?;
            sleep(Duration::from_millis(type_delay_ms(seed, i)));
        }

        let got = client
            .evaluate(&format!(
                "(()=>{{const el=document.querySelector('{selector}');\
                 return el&&el.value?el.value.length:0;}})()"
            ))?
            .as_u64()
            .unwrap_or(0) as usize;
        if got >= expected {
            return Ok(true);
        }
        sleep(Duration::from_millis(500));
    }
    Ok(false)
}

// 현재 페이지에서 로그인 성공/실패 신호를 읽는다.
fn read_signals(client: &mut CdpClient) -> Result<BandPageSignals, AutomationError> {
    let cookies = collect_band_cookies(client)?;
    let current_url = client.current_url().unwrap_or_default();

    // 성공: band_session 쿠키 존재 + auth를 벗어나 www.band.us 로 이동.
    let logged_in = has_band_session_cookies(&cookies) && current_url.contains("www.band.us");
    // 차단: 계정 상태 페이지로 리다이렉트.
    let blocked = current_url.contains("account_status");
    // 비번 오류: 비밀번호 제출 후에도 비밀번호 페이지에 머무름.
    let bad_credentials = current_url.contains(PASSWORD_URL_FRAGMENT) && !logged_in;

    Ok(BandPageSignals {
        logged_in,
        bad_credentials,
        blocked,
    })
}

// Network.getCookies로 band.us 쿠키를 수거한다.
fn collect_band_cookies(client: &mut CdpClient) -> Result<Vec<Value>, AutomationError> {
    // getAllCookies는 경로(Path) 제한과 무관하게 브라우저의 모든 쿠키를 돌려준다.
    // getCookies(urls)는 URL 경로('/')에 매칭되는 쿠키만 줘서, 로그인이 발급하는
    // `secretKey` 쿠키(Path=/s/login/getKey, HttpOnly)가 누락된다 — 이게 없으면
    // 게시용 getKey가 'temp'만 돌려줘 서명키 발급에 실패한다(패킷 캡처로 확인).
    let result = client.call("Network.getAllCookies", json!({}))?;
    let cookies = result
        .get("cookies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| {
            c.get("domain")
                .and_then(Value::as_str)
                .is_some_and(|d| d.contains("band.us"))
        })
        .collect();
    Ok(cookies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_prioritizes_success() {
        let s = BandPageSignals {
            logged_in: true,
            blocked: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::Success);
    }

    #[test]
    fn classify_detects_blocked_and_bad_credentials() {
        assert_eq!(
            classify(&BandPageSignals {
                blocked: true,
                ..Default::default()
            }),
            BandSignal::Blocked
        );
        assert_eq!(
            classify(&BandPageSignals {
                bad_credentials: true,
                ..Default::default()
            }),
            BandSignal::BadCredentials
        );
        assert_eq!(classify(&BandPageSignals::default()), BandSignal::Pending);
    }

    #[test]
    fn classify_blocked_beats_bad_credentials() {
        assert_eq!(
            classify(&BandPageSignals {
                blocked: true,
                bad_credentials: true,
                ..Default::default()
            }),
            BandSignal::Blocked
        );
    }

    #[test]
    fn credentials_present_rejects_empty_or_whitespace() {
        assert!(credentials_present("user", "pw"));
        assert!(!credentials_present("", "pw"));
        assert!(!credentials_present("   ", "pw"));
        assert!(!credentials_present("user", ""));
    }

    // --- decide_loop_step: 과도기 신호를 영구 실패로 latch하지 않는지(2회 확정) ---

    #[test]
    fn loop_success_returns_even_after_negative() {
        assert_eq!(
            decide_loop_step(None, BandSignal::Success, false),
            LoopDecision::Success
        );
        assert_eq!(
            decide_loop_step(Some(BandSignal::BadCredentials), BandSignal::Success, false),
            LoopDecision::Success
        );
    }

    #[test]
    fn loop_bad_credentials_needs_two_consecutive_polls() {
        assert_eq!(
            decide_loop_step(None, BandSignal::BadCredentials, false),
            LoopDecision::KeepWaiting(Some(BandSignal::BadCredentials))
        );
        assert_eq!(
            decide_loop_step(
                Some(BandSignal::BadCredentials),
                BandSignal::BadCredentials,
                false
            ),
            LoopDecision::ConfirmedBad
        );
    }

    #[test]
    fn loop_pending_resets_negative_so_transient_does_not_latch() {
        assert_eq!(
            decide_loop_step(Some(BandSignal::BadCredentials), BandSignal::Pending, false),
            LoopDecision::KeepWaiting(None)
        );
    }

    #[test]
    fn loop_blocked_confirms_only_in_headless_over_two_polls() {
        assert_eq!(
            decide_loop_step(None, BandSignal::Blocked, false),
            LoopDecision::KeepWaiting(Some(BandSignal::Blocked))
        );
        assert_eq!(
            decide_loop_step(Some(BandSignal::Blocked), BandSignal::Blocked, false),
            LoopDecision::ConfirmedBlocked
        );
    }

    #[test]
    fn loop_blocked_in_headed_keeps_waiting_for_user() {
        assert_eq!(
            decide_loop_step(None, BandSignal::Blocked, true),
            LoopDecision::KeepWaiting(None)
        );
    }

    // --- key_info ---

    #[test]
    fn key_info_lowercase_letter_has_keycode_without_shift() {
        let k = key_info('a');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41);
        assert!(!k.shift);
    }

    #[test]
    fn key_info_uppercase_letter_sends_shift() {
        let k = key_info('A');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41);
        assert!(k.shift);
    }

    #[test]
    fn key_info_digit_has_keycode() {
        let k = key_info('7');
        assert_eq!(k.code, "Digit7");
        assert_eq!(k.vk, 0x37);
        assert!(!k.shift);
    }

    #[test]
    fn key_info_shifted_symbol_maps_to_base_digit_with_shift() {
        let bang = key_info('!');
        assert_eq!(bang.code, "Digit1");
        assert_eq!(bang.vk, 0x31);
        assert!(bang.shift);
        let at = key_info('@');
        assert_eq!(at.code, "Digit2");
        assert!(at.shift);
    }

    #[test]
    fn key_info_oem_punctuation_has_nonzero_keycode() {
        for ch in ['-', '_', '.', '/', ';', '\'', '=', '+'] {
            assert_ne!(key_info(ch).vk, 0, "{ch} 의 vk 가 0이면 안 된다");
        }
        assert!(key_info('_').shift);
        assert!(!key_info('-').shift);
    }

    #[test]
    fn key_info_unknown_char_is_best_effort_zero() {
        let k = key_info('가');
        assert_eq!(k.vk, 0);
        assert_eq!(k.code, "");
        assert!(!k.shift);
    }

    // --- type_delay_ms ---

    #[test]
    fn type_delay_always_within_human_range() {
        for seed in [0u64, 1, 42, 9_999, u64::MAX, 0x1234_5678_9ABC_DEF0] {
            for index in 0..64 {
                let d = type_delay_ms(seed, index);
                assert!(
                    (TYPE_DELAY_MIN_MS..=TYPE_DELAY_MAX_MS).contains(&d),
                    "seed={seed} index={index} d={d} 범위 밖"
                );
            }
        }
    }

    #[test]
    fn type_delay_varies_by_index_and_seed() {
        let by_index: Vec<u64> = (0..16).map(|i| type_delay_ms(7, i)).collect();
        assert!(
            by_index
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1,
            "인덱스에 따라 지연이 전혀 안 변함"
        );
        assert_ne!(
            (0..8).map(|i| type_delay_ms(1, i)).collect::<Vec<_>>(),
            (0..8).map(|i| type_delay_ms(2, i)).collect::<Vec<_>>(),
        );
    }

    #[test]
    fn type_delay_is_deterministic_for_same_input() {
        assert_eq!(type_delay_ms(123, 4), type_delay_ms(123, 4));
    }

    #[test]
    fn parse_xy_reads_coordinate_array() {
        assert_eq!(parse_xy(&json!([10.0, 20.5])), Some((10.0, 20.5)));
        assert_eq!(parse_xy(&json!([3, 4])), Some((3.0, 4.0)));
        assert_eq!(parse_xy(&Value::Null), None);
        assert_eq!(parse_xy(&json!([1.0])), None);
        assert_eq!(parse_xy(&json!("nope")), None);
    }

    #[test]
    fn band_session_cookie_detection() {
        let cookies = vec![json!({ "name": "band_session", "value": "a" })];
        assert!(has_band_session_cookies(&cookies));
        // BUC는 네이버 쿠키이므로 band 세션으로 인정하지 않는다.
        assert!(!has_band_session_cookies(&[json!({ "name": "BUC" })]));
        assert!(!has_band_session_cookies(&[json!({ "name": "OTHER" })]));
        assert!(!has_band_session_cookies(&[]));
    }
}
