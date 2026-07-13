//! band.us "네이버로 로그인"(OAuth2) CDP 시퀀스(네이버 `auth/login_flow.rs` 미러).
//!
//! band 이메일 로그인 대신 **네이버 OAuth** 로 로그인한다(패킷 캡처로 검증). 진입 URL
//! `redirect_external_account_login?type=naver` 로 이동하면 band가 **표준 네이버 로그인 폼**
//! (`#id`/`#pw`, default_ecc.js — 일반 네이버 로그인과 동일)으로 리다이렉트한다. 그래서 네이버
//! 로그인과 똑같이 실제 키 이벤트(`Input.dispatchKeyEvent`)로 아이디/비밀번호를 입력한다
//! (페이지 JS가 keydown 을 후킹해 ECC 암호화하므로 값만 꽂으면 암호화가 깨진다). 네이버 인증
//! 뒤에는 새 기기 확인/추가 페이지(네이버 로그인과 동일)와 OAuth 동의(`allow_oauth`) 페이지가
//! 나올 수 있고, band가 세션을 세우면 `.band.us` 에 `band_session` 쿠키가 발급된다. band 측
//! reCAPTCHA(`/b/validation/recaptcha`) 검증이 낄 수 있는데, 자동해결하지 않고 headed 면 사람이
//! 풀도록 계속 기다린다.

use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::naver_automation::{AutomationError, CdpClient};

// band "네이버로 로그인" 진입점. 이 URL로 이동하면 band가 네이버 OAuth authorize 로
// 리다이렉트해 표준 네이버 로그인 폼(#id/#pw)을 띄운다.
const OAUTH_ENTRY_URL: &str =
    "https://auth.band.us/redirect_external_account_login?type=naver&keep_login=false&rcv=none";
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(40);
const HEADED_TIMEOUT: Duration = Duration::from_secs(180);
const POLL_INTERVAL: Duration = Duration::from_secs(2);

// 봇탐지 완화용 스텔스 스크립트(네이버와 동일). CDP 제어 흔적인 navigator.webdriver 를
// 일반 크롬과 동일하게 false 로 맞춘다.
const STEALTH_INIT_JS: &str = "Object.defineProperty(navigator,'webdriver',{get:()=>false});";

// band-OAuth 페이지에 뜨는 표준 네이버 로그인 폼 셀렉터(일반 네이버 로그인과 동일).
const NAVER_ID_SELECTOR: &str = "#id";
const NAVER_PW_SELECTOR: &str = "#pw";

/// 로그인 결과.
pub(crate) enum BandLoginOutcome {
    Ok { cookies: Vec<Value> },
    BadCredentials,
    Blocked,
    Error(String),
}

/// 페이지 판정 신호(분류 입력). 분류 로직을 브라우저에서 분리해 단위 테스트한다.
/// 네이버 OAuth 로그인 폼을 거치지만, band 큐 관점의 결과는 logged_in/bad_credentials/blocked
/// 세 가지로 충분하다. band 측 reCAPTCHA(`/b/validation/recaptcha`)와 계정 상태 페이지는
/// `blocked`로 잡아, headed면 사람이 풀도록 계속 대기하고 headless면 실패로 확정한다.
/// 네이버 캡차(보안문자)는 별도 신호 없이 pending 으로 두어 headed 사람 대기(180초) 안에
/// 풀리면 성공 쿠키로 확정된다(자동해결 안 함).
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

/// band-OAuth 진입 URL(순수 함수). `redirect_external_account_login?type=naver` 로 이동하면
/// band가 네이버 OAuth authorize 로 리다이렉트해 표준 네이버 로그인 폼을 띄운다.
pub(crate) fn oauth_entry_url() -> &'static str {
    OAUTH_ENTRY_URL
}

/// 현재 URL이 네이버 OAuth 동의 페이지(`allow_oauth ... step=agree_term`)인지(순수 함수).
/// 이 화면은 종료 신호가 아니라 "동의" 클릭이 필요한 액션 화면이라 루프에서 자동 클릭한다.
pub(crate) fn is_oauth_consent_url(url: &str) -> bool {
    url.contains("allow_oauth")
}

/// 현재 URL이 band 측 reCAPTCHA/검증 페이지인지(순수 함수). 자동해결하지 않고 `blocked`로 잡아
/// headed면 사람 대기, headless면 실패로 확정한다.
pub(crate) fn is_recaptcha_challenge_url(url: &str) -> bool {
    url.contains("recaptcha") || url.contains("/validation/")
}

/// 수거한 쿠키에 band 세션 쿠키 `band_session`이 있는지 확인한다(순수 함수).
///
/// 네이버 OAuth 로그인이 끝나 band가 세션을 세우면(`external_account_login` 응답) `band_session`
/// 쿠키(domain `.band.us`)가 발급된다 — 이게 로그인 성공의 최종 신호다. (`BUC`는 네이버 쿠키이며
/// band은 발급하지 않는다 — 패킷 캡처로 확인.)
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
) -> (BandLoginOutcome, Option<String>) {
    match run_inner(client, id, pw, wait_for_human) {
        // graceful 실패(Ok(BandLoginOutcome::Error): 폼 못 찾음/타이핑 실패/루프 오류 등)도
        // 알림 "자세히 보기"용 백트레이스를 갖게 한다(네이버 login_flow와 동일). 예전엔 Ok 가지
        // 전부 trace=None으로 흘려 밴드 로그인 실패가 추적 불가였다.
        Ok(outcome) => {
            let trace = match &outcome {
                BandLoginOutcome::Error(_) => Some(crate::util::backtrace_string()),
                _ => None,
            };
            (outcome, trace)
        }
        // CDP/자동화 실패 — 메시지는 사용자용, trace(위치+백트레이스)는 "자세히 보기"용(#210).
        Err(error) => (
            BandLoginOutcome::Error(error.message().to_owned()),
            Some(error.trace()),
        ),
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

    // navigate 전에 스텔스 스크립트를 등록한다(best-effort, 네이버 로그인과 동일).
    let _ = client.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_INIT_JS }),
    );

    // --- 1단계: band-OAuth 진입 → 표준 네이버 로그인 폼 ---
    // redirect_external_account_login?type=naver 로 이동하면 band가 네이버 OAuth authorize 로
    // 리다이렉트해 #id/#pw 로그인 폼(default_ecc.js — 일반 네이버 로그인과 동일)을 띄운다.
    client.navigate(oauth_entry_url())?;
    if !wait_for_visible(client, NAVER_ID_SELECTOR) {
        return Ok(BandLoginOutcome::Error(
            "네이버 로그인 폼(#id)을 찾지 못했습니다(band 네이버 OAuth 리다이렉트 실패 가능)."
                .to_owned(),
        ));
    }

    // --- 2단계: 네이버 아이디/비밀번호 입력 → 로그인 ---
    // 값만 꽂으면 keydown 후킹 ECC 암호화가 깨지므로 실제 키 이벤트로 한 글자씩 입력한다.
    if !type_into(client, NAVER_ID_SELECTOR, id)? {
        return Ok(BandLoginOutcome::Error(
            "아이디 자동 입력에 실패했습니다(아이디 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(1));
    if !type_into(client, NAVER_PW_SELECTOR, pw)? {
        return Ok(BandLoginOutcome::Error(
            "비밀번호 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(Duration::from_secs(2));
    click_login_button(client)?;

    // --- 3단계: 결과 폴링 ---
    // 매 폴링마다 (a) 새 기기 확인/추가 페이지("등록 안함"·네이버 로그인과 동일)와 (b) OAuth 동의
    // 페이지(allow_oauth)를 best-effort 로 처리하고, band_session 쿠키가 뜨는지 확인한다. band 측
    // reCAPTCHA/계정상태 페이지는 blocked 로 잡혀, headed면 사람이 풀도록 계속 대기한다.
    let timeout = if wait_for_human {
        HEADED_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    sleep(POLL_INTERVAL);

    let mut last_negative: Option<BandSignal> = None;
    let deadline = Instant::now() + timeout;
    loop {
        // (a) 인증 성공 직후 뜨는 "새 기기 등록" 페이지면 "등록 안함"을 눌러 마무리한다
        // (네이버 로그인과 동일한 CdpClient 헬퍼 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));
        // (b) OAuth 동의 페이지(allow_oauth ... step=agree_term)면 동의/허용 버튼을 눌러 진행한다.
        let url = client.current_url().unwrap_or_default();
        if is_oauth_consent_url(&url) {
            let _ = click_oauth_consent(client);
        }

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

// 네이버 로그인 버튼을 사람처럼 좌표 마우스 클릭한다(네이버 login_flow 미러). id 값에 점이 있어
// CSS 이스케이프(#log\.login)가 필요하며, 좌표를 못 구하면 .click()으로, 그마저 없으면
// button[type=submit]로 폴백한다.
fn click_login_button(client: &mut CdpClient) -> Result<(), AutomationError> {
    let center = client.evaluate(
        "(()=>{const b=document.querySelector('#log\\\\.login')||\
         document.querySelector('button[type=submit]');if(!b)return null;\
         const r=b.getBoundingClientRect();if(r.width<=0||r.height<=0)return null;\
         return [r.left+r.width/2, r.top+r.height/2];})()",
    )?;
    if let Some((x, y)) = parse_xy(&center) {
        mouse_click(client, x, y)?;
    } else {
        client.evaluate(
            "(()=>{const b=document.querySelector('#log\\\\.login')||\
             document.querySelector('button[type=submit]');if(b){b.click();return true;}return false;})()",
        )?;
    }
    Ok(())
}

// OAuth 동의 페이지(allow_oauth ... step=agree_term)에서 "동의/허용/계속" 버튼을 클릭한다
// (best-effort). id 후보(#agree_btn/#agree/#btnAgree) 우선, 없으면 보이는 버튼/링크/submit 중
// 텍스트에 동의·허용·계속이 든 첫 요소를 누른다. 자동 리다이렉트라 눌 게 없으면 no-op(false).
fn click_oauth_consent(client: &mut CdpClient) -> Result<bool, AutomationError> {
    const JS: &str = "(()=>{\
        const vis=el=>{if(!el)return false;const r=el.getBoundingClientRect();\
            return r.width>0&&r.height>0&&el.offsetParent!==null&&!el.disabled;};\
        let el=document.querySelector('#agree_btn')||document.querySelector('#agree')\
            ||document.querySelector('#btnAgree');\
        if(!vis(el)){el=null;\
            const cs=Array.prototype.slice.call(\
                document.querySelectorAll('button, a, input[type=submit], input[type=button]'));\
            for(const c of cs){\
                const t=String(c.innerText||c.textContent||c.value||'').replace(/\\s+/g,' ').trim();\
                if(vis(c)&&(t.includes('동의')||t.includes('허용')||t.includes('계속'))){el=c;break;}}}\
        if(vis(el)){el.click();return true;}return false;})()";
    Ok(client.evaluate_bool(JS).unwrap_or(false))
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

    // 성공: band가 세션을 세워 band_session 쿠키가 발급됨(fresh Chrome 이라 stale 쿠키 없음).
    let logged_in = has_band_session_cookies(&cookies);
    // 차단: band 측 reCAPTCHA/검증 페이지 또는 계정 상태 페이지. 자동해결하지 않고 blocked 로 둔다
    // (headed면 decide_loop_step 이 사람 대기, headless면 실패로 확정).
    let blocked = is_recaptcha_challenge_url(&current_url) || current_url.contains("account_status");
    // 네이버 캡차(보안문자)가 떠 있으면 비번오류로 오판하지 않는다 — headed 사람 대기 중에 풀도록
    // pending 으로 둔다(자동해결 안 함).
    let naver_captcha = client
        .evaluate_bool(
            "(()=>{const q=s=>{const e=document.querySelector(s);\
             return !!(e&&e.offsetParent!==null);};\
             return q('#captchaDiv')||q('#captcha')||q('img#captchaimg');})()",
        )
        .unwrap_or(false);
    // 비번 오류: 네이버 로그인 폼에 오류 박스(#err_common)가 보이는 상태로 텍스트를 가짐(네이버
    // login_flow 미러). 캡차가 떠 있거나 이미 로그인됐으면 오류로 보지 않는다.
    let bad_credentials = !logged_in
        && !naver_captcha
        && client
            .evaluate_bool(
                "(()=>{const e=document.querySelector('#err_common');\
                 return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()",
            )
            .unwrap_or(false);

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

    // --- OAuth 진입 URL / 페이지 판별(순수 함수) ---

    #[test]
    fn oauth_entry_url_targets_naver_external_login() {
        let url = oauth_entry_url();
        assert!(url.contains("auth.band.us/redirect_external_account_login"));
        assert!(url.contains("type=naver"));
    }

    #[test]
    fn detects_oauth_consent_page() {
        assert!(is_oauth_consent_url(
            "https://nid.naver.com/login/noauth/allow_oauth?oauth_token=x&step=agree_term"
        ));
        assert!(!is_oauth_consent_url(
            "https://nid.naver.com/nidlogin.login?mode=form"
        ));
        assert!(!is_oauth_consent_url("https://www.band.us"));
    }

    #[test]
    fn detects_recaptcha_challenge_page() {
        assert!(is_recaptcha_challenge_url(
            "https://auth.band.us/b/validation/recaptcha"
        ));
        assert!(is_recaptcha_challenge_url(
            "https://auth.band.us/b/confirm_recaptcha"
        ));
        assert!(!is_recaptcha_challenge_url("https://www.band.us"));
        assert!(!is_recaptcha_challenge_url(
            "https://nid.naver.com/nidlogin.login"
        ));
    }
}
