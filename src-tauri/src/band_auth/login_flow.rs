//! band.us "네이버로 로그인/가입"(OAuth2) CDP 시퀀스. 네이버 `auth/login_flow.rs` 의 메커니즘을
//! **그대로 미러**한다: 페이지에서 API(fetch)를 호출하지 않고(봇탐지 표면↑) DOM 을 몰아 —
//! 아이디/비밀번호를 실제 키 이벤트(`Input.dispatchKeyEvent`)로 입력하고, 버튼은 좌표 마우스
//! 클릭(진짜 mouse 이벤트, JS `.click()` 아님)으로 누르며, `document.readyState==='complete'` 를
//! 기다린 뒤 쿠키 + 현재 URL + DOM 텍스트로 페이지 상태를 분류한다.
//!
//! 진입 URL `redirect_external_account_login?type=naver` 로 이동하면 band 가 **표준 네이버 로그인
//! 폼**(`#id`/`#pw`, default_ecc.js — 일반 네이버 로그인과 동일)으로 리다이렉트한다. 네이버 인증
//! 뒤에는 (a) 새 기기 등록 페이지("등록 안함"), (b) OAuth 동의(`allow_oauth`) 페이지가 나올 수 있고,
//! band 가 세션을 세우면 `.band.us` 에 `band_session` 쿠키가 발급된다(로그인 성공의 최종 신호).
//! 미가입 계정이면 `auth.band.us/login?...&_ns=false` 로 떨어져 "네이버로 가입하기"를 눌러야 하고,
//! 그러면 네이버 로그인 폼이 **다시** 떠 재로그인 → `continue_external_account_sign_up` → 가입 완료
//! → `band_session` 순으로 진행된다. 각 화면은 아래 반응형 루프가 매 폴링마다 재판정해 처리한다.

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

// 봇탐지(ncaptcha/wtm) 완화용 스텔스 스크립트(네이버 login_flow 와 동일). navigator.webdriver 를
// **인스턴스가 아닌 Navigator.prototype** 에 정의해(일반 크롬과 위치까지 동일) own-property 흔적을
// 남기지 않고 false 로 맞추고, languages 도 일반 크롬(`ko-KR,ko,en-US,en`)과 동일하게 정렬한다.
const STEALTH_INIT_JS: &str = "(()=>{try{\
    Object.defineProperty(Navigator.prototype,'webdriver',\
        {get:()=>false,configurable:true,enumerable:true});}catch(e){}\
    try{Object.defineProperty(Navigator.prototype,'languages',\
        {get:()=>['ko-KR','ko','en-US','en'],configurable:true,enumerable:true});}catch(e){}})();";

// band-OAuth 페이지에 뜨는 표준 네이버 로그인 폼 셀렉터(일반 네이버 로그인과 동일).
const NAVER_ID_SELECTOR: &str = "#id";
const NAVER_PW_SELECTOR: &str = "#pw";

// 폼 준비/결과 DOM 이 흔들리지 않고 자리잡았다고 볼 연속 확인 횟수(네이버 미러). 상위 문서가
// complete 된 뒤에도 캡차/안티봇 iframe·스크립트가 뒤늦게 로드되며 DOM 이 잠깐 출렁이므로,
// 그 과도기에 타이핑/클릭하지 않도록 연속 N회 안정될 때만 진행한다.
const FORM_READY_STABLE_POLLS: u32 = 3;

// 안티봇/keydown 암호화 스크립트가 실제 로드(주입)됐는지 본다(네이버 미러). performance resource
// 타이밍은 리소스가 "다운로드 완료"됐을 때만 엔트리가 생기므로, 이 패턴이 잡히면 스크립트가
// 실제로 붙은 것이다(wtm=봇탐지, default_ecc=keydown 암호화, ncaptcha=캡차).
const ANTIBOT_READY_JS: &str = "(()=>{try{\
    const r=performance.getEntriesByType('resource');\
    return r.some(e=>/wtm\\.pstatic\\.net|default_ecc|ncaptcha|nclk\\.naver/i.test(e.name));\
}catch(e){return false;}})()";

// 페이지가 받은 "완료된 리소스 수"(네이버 미러). 폴링 간에 이 수가 더 안 늘면 = 그 사이 새로
// 끝난(=로딩 중이던) 리소스가 없다 = 로딩이 정착했다는 뜻.
const RESOURCE_COUNT_JS: &str =
    "(()=>{try{return performance.getEntriesByType('resource').length;}catch(e){return -1;}})()";

// 상위 문서 + 모든 iframe 이 complete 인지 본다(네이버 ALL_DOCS_COMPLETE_JS 미러). 교차 출처
// iframe 은 contentDocument 를 읽을 수 없어 통과(true)로 둔다(보안상 검사 불가).
const ALL_DOCS_COMPLETE_JS: &str = "(()=>{\
    if(document.readyState!=='complete')return false;\
    const frames=Array.prototype.slice.call(document.querySelectorAll('iframe'));\
    return frames.every(f=>{\
        try{const d=f.contentDocument;return !d||d.readyState==='complete';}\
        catch(e){return true;}\
    });\
})()";

// 페이지 본문 텍스트(앞부분)를 읽는다 — 장기 미로그인/잠금 안내 문구 판정용.
const BODY_TEXT_JS: &str = "(()=>{try{\
    return (document.body?document.body.innerText:'').replace(/\\s+/g,' ').slice(0,600);\
}catch(e){return '';}})()";

// "네이버로 가입하기" 버튼/링크가 화면에 보이는지(미가입 계정 화면 판정용).
const SIGNUP_BUTTON_JS: &str = "(()=>{\
    const els=Array.prototype.slice.call(\
        document.querySelectorAll('a, button, input[type=submit], input[type=button]'));\
    return els.some(el=>{\
        const t=String(el.innerText||el.textContent||el.value||'').replace(/\\s+/g,' ');\
        return el.offsetParent!==null&&t.includes('네이버로 가입');});})()";

// OAuth 동의 화면이 DOM 으로 떠 있는지(URL 판정 폴백·주력). 실제 동의 화면 URL 은
// `nid.naver.com/oauth2.0/authorize`(band redirect_uri 동봉)라 `allow_oauth`(동의 제출 시 POST 되는
// 순간 URL)만으로는 못 잡는다 — 패킷/로그로 확인(2026-07-13). 그래서 화면 고유 구조로 감지한다:
// 본문에 "개인정보"+"제3자/제 3자"(밴드 OAuth [필수] 제3자 제공 동의) 가 있고, 화면에 보이는
// "동의하기" 버튼이 있을 때. 전이 리다이렉트(authorize) 순간엔 이 DOM 이 없어 오탐이 없다.
const CONSENT_DOM_JS: &str = "(()=>{\
    const body=String(document.body?document.body.innerText:'').replace(/\\s+/g,'');\
    if(body.indexOf('개인정보')<0||(body.indexOf('제3자')<0&&body.indexOf('제3자제공')<0))return false;\
    const els=Array.prototype.slice.call(\
        document.querySelectorAll('a, button, input[type=submit], input[type=button]'));\
    return els.some(el=>{\
        const t=String(el.innerText||el.textContent||el.value||'').replace(/\\s+/g,'');\
        return el.offsetParent!==null&&(t==='동의하기'||t==='동의'||t==='허용하기'||t==='허용');});})()";

// 밴드 2단계 인증/추가 검증 게이트 화면인지 본문 텍스트로 판정한다(순수 함수). band_session 쿠키가
// 이 화면(`auth.band.us/b/validation_welcome`)에서 이미 발급되지만, 이는 인증 미완료 반쪽 세션이라
// 게시가 거부된다("session expired"/"not authorized") — 성공으로 보면 안 된다(2026-07-13 로그 확인).
pub(crate) fn is_two_factor_text(text: &str) -> bool {
    text.contains("2단계 인증") || text.contains("2차 인증") || text.contains("2단계 인증이 필요")
}

/// 로그인 결과.
pub(crate) enum BandLoginOutcome {
    Ok { cookies: Vec<Value> },
    BadCredentials,
    Blocked,
    /// 본인확인(휴대전화) 화면이 떴는데 계정 ID가 휴대전화 형식이 아니라 자동으로 풀 수 없어
    /// 로그인을 보류한 상태(네이버 `PhoneVerify` 미러). 계정 상태는 `OnHold`로 매핑해 사용자가
    /// 보류 계정만 골라 직접 처리하게 한다.
    OnHold,
    Error(String),
}

/// 페이지 판정 신호(분류 입력). 분류 로직을 브라우저에서 분리해 단위 테스트한다.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct BandPageSignals {
    /// band 세션 쿠키(`band_session`) 발급 = 로그인 성공.
    pub logged_in: bool,
    /// 표준 네이버 로그인 폼(#id/#pw)이 이번 폴링에서 **입력해야 하는** 상태(보이고, 아직 이번
    /// 폼에 제출하지 않았고, 캡차/오류 표시가 없음). 첫 로그인·가입 재로그인 모두 여기로 잡힌다.
    pub naver_form: bool,
    /// OAuth 동의 페이지(`allow_oauth`/`agree_term`).
    pub consent: bool,
    /// 미가입 계정 화면(`/login?...&_ns=false` 또는 "네이버로 가입하기" 버튼).
    pub signup_needed: bool,
    /// 새 기기 등록/확인 페이지(`deviceConfirm`/`deviceCheck`) — 실패 아님, "등록 안함" 후 진행.
    pub device: bool,
    /// 네이버 캡차(보안문자) 표시.
    pub captcha: bool,
    /// 본인확인(휴대전화 번호) 화면(`#phone_value`).
    pub phone_verify: bool,
    /// 2차 인증/추가 인증(OTP) 화면.
    pub otp: bool,
    /// 장기 미로그인 안내 화면(본문 텍스트로 판정).
    pub long_dormant: bool,
    /// 계정 보호조치(`idSafetyRelease`) 착지.
    pub protected: bool,
    /// 계정 잠금 안내 본문 감지.
    pub locked: bool,
    /// 비밀번호 오류(#err_common 가 보이고 텍스트를 가짐, 캡차 아님).
    pub bad_credentials: bool,
    /// band 측 reCAPTCHA/검증 페이지 또는 계정 상태 페이지(휴리스틱 차단).
    pub blocked: bool,
}

/// 진행 중/확정 신호(순수 분류 결과).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BandSignal {
    Pending,
    Success,
    NaverForm,
    Consent,
    SignupNeeded,
    Device,
    Captcha,
    PhoneVerify,
    Otp,
    LongDormant,
    Protected,
    Locked,
    BadCredentials,
    Blocked,
}

/// 폴링 한 스텝의 판정 결과. 루프는 이 값을 실제 동작(입력/클릭/반환/대기)으로 옮긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopAction {
    Success,
    TypeLogin,
    ClickConsent,
    ClickSignup,
    HandleDevice,
    HandlePhoneVerify,
    ConfirmedBad,
    ConfirmedBlocked,
    KeepWaiting(Option<BandSignal>),
}

/// 페이지 신호를 로그인 진행/결과 신호로 분류한다(순수 함수). 우선순위(사용자 지시):
/// 성공 > 네이버폼 > 동의 > 가입 > 새기기 > 캡차 > 본인확인 > OTP > 장기미로그인 > 보호조치 >
/// 잠금 > 비번오류 > 차단 > 대기. 네이버 폼은 `naver_form`(제출 전·캡차/오류 없음)일 때만 잡혀,
/// 오류가 뜬 폼(비번오류)이나 캡차가 뜬 폼에는 재입력하지 않는다.
pub(crate) fn classify(s: &BandPageSignals) -> BandSignal {
    if s.logged_in {
        BandSignal::Success
    } else if s.naver_form {
        BandSignal::NaverForm
    } else if s.consent {
        BandSignal::Consent
    } else if s.signup_needed {
        BandSignal::SignupNeeded
    } else if s.device {
        BandSignal::Device
    } else if s.captcha {
        BandSignal::Captcha
    } else if s.phone_verify {
        BandSignal::PhoneVerify
    } else if s.otp {
        BandSignal::Otp
    } else if s.long_dormant {
        BandSignal::LongDormant
    } else if s.protected {
        BandSignal::Protected
    } else if s.locked {
        BandSignal::Locked
    } else if s.bad_credentials {
        BandSignal::BadCredentials
    } else if s.blocked {
        BandSignal::Blocked
    } else {
        BandSignal::Pending
    }
}

/// 직전 음성 신호와 현재 신호로 이번 폴링의 동작을 결정한다(순수 함수).
///
/// - 액션 신호(네이버폼/동의/가입/새기기/본인확인)는 즉시 해당 동작으로 옮긴다.
/// - 종료 실패(OTP/장기미로그인/보호조치/잠금)는 headed 여도 즉시 확정한다(사용자가 즉석에서
///   풀 수 없는 상태 — 네이버 Protected/Locked/미지원 인증 즉시 실패 미러).
/// - 캡차/차단(reCAPTCHA)은 headed 면 사람이 풀도록 계속 대기하고, headless 면 **2회 연속**일
///   때만 확정한다(과도기 깜빡임 latch 방지).
/// - 비번오류도 **2회 연속**일 때만 확정한다.
fn decide_loop_step(
    last_negative: Option<BandSignal>,
    signal: BandSignal,
    wait_for_human: bool,
) -> LoopAction {
    match signal {
        BandSignal::Success => LoopAction::Success,
        BandSignal::NaverForm => LoopAction::TypeLogin,
        BandSignal::Consent => LoopAction::ClickConsent,
        BandSignal::SignupNeeded => LoopAction::ClickSignup,
        BandSignal::Device => LoopAction::HandleDevice,
        BandSignal::PhoneVerify => LoopAction::HandlePhoneVerify,
        // 사람이 즉석에서 풀 수 없는 종료 상태 — headed 여도 즉시 실패(Blocked 매핑).
        BandSignal::Otp
        | BandSignal::LongDormant
        | BandSignal::Protected
        | BandSignal::Locked => LoopAction::ConfirmedBlocked,
        // 캡차/차단: headed 는 사람 대기, headless 는 2회 연속 확정.
        BandSignal::Captcha | BandSignal::Blocked => {
            if wait_for_human {
                LoopAction::KeepWaiting(last_negative)
            } else if last_negative == Some(signal) {
                LoopAction::ConfirmedBlocked
            } else {
                LoopAction::KeepWaiting(Some(signal))
            }
        }
        BandSignal::BadCredentials => {
            if last_negative == Some(BandSignal::BadCredentials) {
                LoopAction::ConfirmedBad
            } else {
                LoopAction::KeepWaiting(Some(BandSignal::BadCredentials))
            }
        }
        BandSignal::Pending => LoopAction::KeepWaiting(None),
    }
}

/// 자격증명이 로그인 시도에 충분한지(둘 다 비어있지 않은지) 검사한다(순수 함수).
pub(crate) fn credentials_present(id: &str, pw: &str) -> bool {
    !id.trim().is_empty() && !pw.is_empty()
}

/// band-OAuth 진입 URL(순수 함수).
pub(crate) fn oauth_entry_url() -> &'static str {
    OAUTH_ENTRY_URL
}

/// 현재 URL이 네이버 OAuth 동의 페이지(`allow_oauth`/`agree_term`)인지(순수 함수).
pub(crate) fn is_oauth_consent_url(url: &str) -> bool {
    url.contains("allow_oauth") || url.contains("agree_term")
}

/// 현재 URL이 "로그인 완료" band 홈인지(순수 함수). band_session 쿠키만으론 성공을 못 가른다
/// — auth.band.us 인터스티셜(2단계 인증/캡차/가입중)에서도 band_session 이 발급되기 때문이다.
/// 실제 완료는 auth 도메인·네이버 도메인을 벗어나 band.us 홈(`www.band.us` 등)에 착지한 상태다
/// (패킷 diff 2026-07-13: 성공 계정만 `www.band.us` JSESSIONID 발급). 인터스티셜(`auth.band.us`)과
/// 네이버 로그인(`nid.naver.com`)은 제외한다.
pub(crate) fn is_logged_in_band_url(url: &str) -> bool {
    url.contains("band.us") && !url.contains("auth.band.us") && !url.contains("nid.naver.com")
}

/// 현재 URL이 미가입 계정 화면(`_ns=false`)인지(순수 함수). band 가 미가입 네이버 계정을
/// `auth.band.us/login?...&_ns=false` 로 떨궈 "네이버로 가입하기"를 눌러야 한다.
pub(crate) fn is_signup_needed_url(url: &str) -> bool {
    url.contains("_ns=false")
}

/// 현재 URL이 band 측 reCAPTCHA/검증 페이지인지(순수 함수).
pub(crate) fn is_recaptcha_challenge_url(url: &str) -> bool {
    url.contains("recaptcha") || url.contains("/validation/")
}

/// 현재 URL이 밴드 "장기 미로그인" 화면인지(순수 함수). 패킷 캡처로 확인한 정확한 착지 URL
/// (`auth.band.us/b/inactive_user`)로 잡는다 — 텍스트 매칭보다 오탐 없이 정확하다.
pub(crate) fn is_inactive_user_url(url: &str) -> bool {
    url.contains("/b/inactive_user")
}

/// 본문 텍스트가 장기 미로그인 안내인지(순수 함수). URL(`/b/inactive_user`) 감지의 폴백 —
/// 화면 문구로도 잡는다("장기"/"오랫동안 로그인"/"미로그인").
pub(crate) fn is_long_dormant_text(text: &str) -> bool {
    text.contains("장기") || text.contains("오랫동안 로그인") || text.contains("미로그인")
}

/// 본문 텍스트가 계정 잠금 안내인지(순수 함수, 네이버 미러). 잠금 페이지 고유어로 좁혀 오탐을 막는다.
pub(crate) fn is_locked_text(text: &str) -> bool {
    text.contains("아이디 잠금조치")
        || text.contains("보호(잠금)")
        || text.contains("비정상적인 활동이 감지")
}

/// 로그인 ID가 "010 + 숫자 8자리"(총 11자리) 휴대전화 형식인지(순수 함수, 네이버 미러). 본인확인
/// 화면에서 이 형식이면 그 번호를 입력·확인까지 시도하고, 아니면 즉시 보류로 떨어뜨린다.
pub(crate) fn id_is_phone_format(id: &str) -> bool {
    let t = id.trim();
    t.len() == 11 && t.starts_with("010") && t.bytes().all(|b| b.is_ascii_digit())
}

/// 수거한 쿠키에 band 세션 쿠키 `band_session`이 있는지 확인한다(순수 함수). band 가 세션을
/// 세우면(`external_account_login` 응답) `band_session`(domain `.band.us`)이 발급된다 — 로그인
/// 성공의 최종 신호. (`BUC`는 네이버 쿠키이며 band 은 발급하지 않는다 — 패킷 캡처로 확인.)
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
        // graceful 실패(Ok(Error))도 알림 "자세히 보기"용 백트레이스를 갖게 한다(네이버 미러).
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

    // band-OAuth 진입 → 표준 네이버 로그인 폼으로 리다이렉트. 이후 판정/동작은 반응형 루프가 맡는다.
    client.navigate(oauth_entry_url())?;

    let timeout = if wait_for_human {
        HEADED_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    let deadline = Instant::now() + timeout;
    let mut last_negative: Option<BandSignal> = None;
    // 현재 표시된 네이버 폼에 이미 입력·제출했는지. 첫 로그인 후 다시 폼이 뜨는 경우는 오직
    // "가입하기" 클릭 뒤이므로(재로그인), 그 arm 에서만 false 로 되돌려 재입력을 허용한다. 이 플래그로
    // 제출 직후 같은 폼에 이중 제출하는 것을 막는다.
    let mut submitted_for_form = false;
    // 본인확인(휴대전화) 번호 입력·확인을 이미 1회 시도했는지(중복 제출 방지).
    let mut phone_attempted = false;
    // 첫 타이핑 직전 1회만 웹 컨텐츠로 창 포커스를 옮긴다(주소창 선택 해제 → 캡차 완화, 네이버 미러).
    let mut focused_once = false;

    loop {
        // (a) 인증 성공 직후/새 기기 확인 페이지의 "등록 안함" 다이얼로그를 best-effort 로 눌러 마무리한다
        // (네이버와 동일한 CdpClient 헬퍼 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        // 새로 이동한 페이지를 읽거나 동작하기 전에 DOM 로딩 완료(readyState=complete + 모든 iframe)
        // 를 기다린다(사용자 보고: 페이지가 다 로드되기 전에 동작하던 문제 — 매 화면에서 방지).
        wait_for_dom_ready(client);

        let signals = read_signals(client, submitted_for_form)?;
        match decide_loop_step(last_negative, classify(&signals), wait_for_human) {
            LoopAction::Success => {
                let cookies = collect_band_cookies(client)?;
                return Ok(BandLoginOutcome::Ok { cookies });
            }
            LoopAction::TypeLogin => {
                // 타이핑 전에 폼이 "완전히" 준비될 때까지 기다린다(readyState + #id/#pw 입력가능 +
                // 로그인 버튼 + 모든 iframe complete + 안티봇/keydown 후킹 설치, 연속 안정). 준비 전
                // 타이핑은 keydown 암호화가 깨져 캡차를 유발한다(네이버 wait_for_login_form 미러).
                if !wait_for_login_form(client) {
                    return Ok(BandLoginOutcome::Error(
                        "네이버 로그인 폼(#id/#pw)이 준비되지 않았습니다(band 네이버 OAuth 리다이렉트 실패 가능)."
                            .to_owned(),
                    ));
                }
                if !focused_once {
                    focus_web_contents(client);
                    focused_once = true;
                }
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
                sleep(Duration::from_secs(1));
                click_login_button(client)?;
                submitted_for_form = true;
                last_negative = None;
            }
            LoopAction::ClickConsent => {
                let _ = click_oauth_consent(client);
                last_negative = None;
            }
            LoopAction::ClickSignup => {
                // "네이버로 가입하기"를 눌러 redirect_external_account_sign_up 으로 이동시킨다. 그러면
                // 네이버 로그인 폼이 다시 뜨므로 재입력을 허용하도록 제출 플래그를 되돌린다.
                let _ = click_signup_button(client);
                submitted_for_form = false;
                last_negative = None;
            }
            LoopAction::HandleDevice => {
                // 위에서 이미 "등록 안함"을 눌렀다 — 다음 폴링에서 진행 상태를 재판정한다.
                last_negative = None;
            }
            LoopAction::HandlePhoneVerify => {
                // ID가 010+8자리면 그 번호를 #phone_value 에 넣고 확인을 눌러 1회 시도한다. 형식이
                // 아니면 즉시 보류(OnHold)로 떨어뜨린다(네이버 PhoneVerify 미러).
                if !id_is_phone_format(id) {
                    return Ok(BandLoginOutcome::OnHold);
                }
                if !phone_attempted {
                    let _ = type_into(client, "#phone_value", id)?;
                    let _ = client.evaluate(
                        "(()=>{const b=document.querySelector('#oab\\\\.submit')\
                         ||document.querySelector('#frmNIDLogin input[type=submit]');\
                         if(b){b.click();return true;}return false;})()",
                    )?;
                    phone_attempted = true;
                }
                last_negative = None;
            }
            LoopAction::ConfirmedBad => return Ok(BandLoginOutcome::BadCredentials),
            LoopAction::ConfirmedBlocked => return Ok(BandLoginOutcome::Blocked),
            LoopAction::KeepWaiting(next) => last_negative = next,
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

// 현재 페이지에서 로그인 진행/결과 신호를 읽는다.
fn read_signals(
    client: &mut CdpClient,
    submitted_for_form: bool,
) -> Result<BandPageSignals, AutomationError> {
    let cookies = collect_band_cookies(client)?;
    // band_session 쿠키 존재는 "성공"의 필요조건일 뿐 충분조건이 아니다 — 2단계 인증 게이트
    // (validation_welcome)에서도 이미 발급된다. 아래에서 인증 게이트가 아닐 때만 성공으로 확정한다.
    let has_session = has_band_session_cookies(&cookies);
    let url = client.current_url().unwrap_or_default();
    let body_text = client.evaluate_string(BODY_TEXT_JS).unwrap_or_default();

    let captcha = visible_exists(client, "#captchaDiv, #captcha, img#captchaimg");
    let phone_verify = visible_exists(client, "#phone_value");
    // 밴드 2단계 인증은 화면 문구("2단계 인증")로만 잡는다 — 사람이 즉석에서 풀 수 없는 종료 실패.
    // URL(validation_welcome)로는 잡지 않는다: 가입 흐름도 그 화면을 잠깐 지나가므로 성공 가능한
    // 계정을 죽이면 안 된다(패킷 diff: 성공한 mango 도 validation_welcome 를 거쳐 band.us 로 진행).
    let two_factor = is_two_factor_text(&body_text);
    let otp = two_factor || visible_exists(client, "#otp, input[name=otp], #cellphoneCertify");

    // 네이버 로그인 폼(#id/#pw)이 보이고 입력 가능한지.
    let form_visible = client
        .evaluate_bool(
            "(()=>{const ok=el=>!!(el&&el.offsetParent!==null&&!el.disabled);\
             return ok(document.querySelector('#id'))&&ok(document.querySelector('#pw'));})()",
        )
        .unwrap_or(false);
    // 비번 오류: #err_common 이 보이고 텍스트를 가짐. 캡차가 떠 있거나 이미 로그인됐으면 오류로
    // 보지 않는다(네이버 미러).
    let bad_credentials = !has_session
        && !captcha
        && client
            .evaluate_bool(
                "(()=>{const e=document.querySelector('#err_common');\
                 return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()",
            )
            .unwrap_or(false);
    // 폼이 보이고, 아직 이번 폼에 제출하지 않았고, 캡차/오류 표시가 없을 때만 "입력해야 하는" 폼으로
    // 본다 — 오류가 뜬 폼(비번오류)이나 캡차가 뜬 폼에 재입력하지 않는다.
    let naver_form = form_visible && !submitted_for_form && !captcha && !bad_credentials && !has_session;

    // 동의 화면 감지: URL(allow_oauth/agree_term) 또는 화면 DOM(동의하기 버튼 + 개인정보 제3자 제공).
    // 실제 동의 화면 URL 은 oauth2.0/authorize 라 URL 만으론 못 잡아 DOM 감지가 주력이다(2026-07-13).
    let consent = is_oauth_consent_url(&url) || client.evaluate_bool(CONSENT_DOM_JS).unwrap_or(false);
    let signup_needed =
        is_signup_needed_url(&url) || client.evaluate_bool(SIGNUP_BUTTON_JS).unwrap_or(false);
    let device = url.contains("deviceConfirm") || url.contains("deviceCheck");

    let on_naver = url.contains("nid.naver.com");
    let long_dormant = is_inactive_user_url(&url) || is_long_dormant_text(&body_text);
    let protected = url.contains("idSafetyRelease");
    let locked = on_naver && is_locked_text(&body_text);
    let blocked = is_recaptcha_challenge_url(&url) || url.contains("account_status");

    // 성공 확정(패킷 diff 근거, 2026-07-13): band_session 만으론 부족하다 — auth.band.us 인터스티셜
    // (2단계 인증/캡차/recaptcha/가입중)에서도 발급되며 그 반쪽 세션은 게시가 거부된다("session
    // expired 4 hours"). 실제 로그인 완료 = auth 도메인을 벗어나 band.us 홈(www.band.us 등)에 착지
    // (성공한 계정만 www.band.us JSESSIONID 발급). band_session 이 있고 band.us 홈일 때만 성공.
    let logged_in = has_session && is_logged_in_band_url(&url);

    Ok(BandPageSignals {
        logged_in,
        naver_form,
        consent,
        signup_needed,
        device,
        captcha,
        phone_verify,
        otp,
        long_dormant,
        protected,
        locked,
        bad_credentials,
        blocked,
    })
}

// 셀렉터에 해당하는 "화면에 보이는" 요소가 있는지(네이버 미러). offsetParent 가 null 이면 숨겨진
// 요소이므로 false 로 본다.
fn visible_exists(client: &mut CdpClient, selector: &str) -> bool {
    let expr = format!(
        "(()=>{{const e=document.querySelector('{selector}');\
         return !!(e&&e.offsetParent!==null);}})()"
    );
    client.evaluate_bool(&expr).unwrap_or(false)
}

// 새로 이동한 페이지가 로딩 완료(readyState=complete + 모든 iframe complete)될 때까지 기다린다
// (네이버 ALL_DOCS_COMPLETE_JS 미러). 상한(10초) 안에 완료를 못 봐도 진행한다(무한 wedge 방지).
fn wait_for_dom_ready(client: &mut CdpClient) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if client.evaluate_bool(ALL_DOCS_COMPLETE_JS).unwrap_or(false) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

// 네이버 로그인 폼이 "완전히" 로딩될 때까지 기다린다(네이버 wait_for_login_form 미러): 상위 문서
// complete + #id/#pw 보임·입력가능 + 로그인 버튼 + 모든 iframe complete + 리소스 로딩 정착 + 안티봇
// 스크립트 로드 + keydown 암호화 후킹 설치, 넷 다 만족하고 연속 안정일 때만 타이핑한다. 준비 안 된
// 폼에 타이핑해 캡차를 유발하지 않는 것이 우선. Chrome 이 사라지면(연속 CDP 실패) 중단한다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    tracing::info!("[BAND] 로그인 폼 로딩 대기 중...");
    const MAX_CONN_FAIL: u32 = 50; // ~5초 연속 CDP 실패 = Chrome 사라짐
    let mut conn_fail = 0u32;
    let ready_expr = "(()=>{\
        if(document.readyState!=='complete')return false;\
        const ok=el=>!!(el&&el.offsetParent!==null&&!el.disabled);\
        const btn=document.querySelector('#log\\\\.login')\
                  ||document.querySelector('button[type=submit]');\
        if(!(ok(document.querySelector('#id'))\
             &&ok(document.querySelector('#pw'))&&!!btn))return false;\
        const frames=Array.prototype.slice.call(document.querySelectorAll('iframe'));\
        return frames.every(f=>{\
            try{const d=f.contentDocument;return !d||d.readyState==='complete';}\
            catch(e){return true;}\
        });\
    })()";
    let mut streak = 0u32;
    let mut antibot_seen = false;
    let mut prev_res_count: Option<i64> = None;
    loop {
        let form_ready = match client.evaluate_bool(ready_expr) {
            Ok(r) => {
                conn_fail = 0;
                r
            }
            Err(_) => {
                conn_fail += 1;
                if conn_fail >= MAX_CONN_FAIL {
                    tracing::info!("[BAND] ✗ Chrome 연결이 끊겨 로그인 폼 대기를 중단");
                    return false;
                }
                sleep(Duration::from_millis(100));
                continue;
            }
        };
        if !antibot_seen {
            antibot_seen = client.evaluate_bool(ANTIBOT_READY_JS).unwrap_or(false);
        }
        let res_count = client
            .evaluate(RESOURCE_COUNT_JS)
            .ok()
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let resources_settled = res_count >= 0 && prev_res_count == Some(res_count);
        prev_res_count = Some(res_count);
        let keydown_hook_ready = form_ready
            && resources_settled
            && antibot_seen
            && (client.expr_has_listener("document.querySelector('#pw')", "keydown")
                || client.expr_has_listener("document.querySelector('#id')", "keydown"));
        let gate = login_form_gate_open(
            form_ready,
            resources_settled,
            antibot_seen,
            keydown_hook_ready,
        );
        streak = next_ready_streak(streak, gate);
        if streak >= FORM_READY_STABLE_POLLS {
            tracing::info!("[BAND] ✓ 로그인 폼 완전 로딩 확인(입력 준비 완료)");
            return true;
        }
        sleep(Duration::from_millis(100));
    }
}

/// "폼 준비" 신호의 연속 안정 횟수를 갱신한다(순수 함수, 네이버 미러). 준비됐으면 누적, 한 번이라도
/// 흔들리면 0으로 리셋한다.
fn next_ready_streak(streak: u32, ready_now: bool) -> u32 {
    if ready_now {
        streak.saturating_add(1)
    } else {
        0
    }
}

/// 로그인 폼 진행 게이트(순수 함수, 네이버 미러). 폼 준비 + 리소스 정착 + 안티봇 스크립트 로드 +
/// keydown 후킹 설치, 넷 **모두** 만족해야 진행한다.
fn login_form_gate_open(
    form_ready: bool,
    resources_settled: bool,
    antibot_ready: bool,
    keydown_hook_ready: bool,
) -> bool {
    form_ready && resources_settled && antibot_ready && keydown_hook_ready
}

// 브라우저(창) 포커스를 omnibox(주소창)에서 웹 컨텐츠로 옮긴다(네이버 focus_web_contents 미러).
// CDP 입력은 렌더러 activeElement 만 바꿔 창 미포커스 상태가 안 풀리는데, 그 미포커스가 wtm
// 안티봇의 봇 신호라 캡차를 키운다. 탭을 앞으로 가져오고 중립 body 좌표를 진짜 마우스로 클릭한다.
fn focus_web_contents(client: &mut CdpClient) {
    if let Err(error) = client.call("Page.bringToFront", json!({})) {
        tracing::warn!("[BAND] Page.bringToFront 실패 — 포커스 이동 일부만 적용: {error}");
    }
    if let Ok(Some((x, y))) = neutral_body_point(client) {
        let _ = mouse_click(client, x, y);
    }
}

// 클릭 가능한 요소와 겹치지 않는 viewport 내 빈 좌표를 하나 고른다(네이버 미러). 임의 좌표를
// 클릭해 링크/버튼을 잘못 누르는 사고를 막는다.
fn neutral_body_point(client: &mut CdpClient) -> Result<Option<(f64, f64)>, AutomationError> {
    const JS: &str = "(()=>{\
        const w=innerWidth,h=innerHeight;\
        const cand=[[w*0.5,h*0.12],[w*0.5,h*0.06],[w*0.12,h*0.5],[w*0.88,h*0.5],[w*0.5,h*0.92],[8,8]];\
        const bad=el=>{for(let n=el;n&&n!==document.body;n=n.parentElement){const t=n.tagName;\
            if(t==='A'||t==='BUTTON'||t==='INPUT'||t==='SELECT'||t==='TEXTAREA'||t==='LABEL')return true;\
            if(typeof n.onclick==='function')return true;}return false;};\
        for(const [x,y] of cand){const el=document.elementFromPoint(x,y);if(el&&!bad(el))return [x,y];}\
        return null;})()";
    Ok(parse_xy(&client.evaluate(JS)?))
}

// 페이지를 강제로 전경·포커스로 만든다(네이버 force_page_foreground 미러). 창이 가려져
// visibilityState=hidden 이면 합성 키 이벤트가 렌더러로 전달되지 않아 타이핑이 0자가 된다.
fn force_page_foreground(client: &mut CdpClient) {
    if let Err(error) = client.call("Page.bringToFront", json!({})) {
        tracing::debug!(error = %error, "[BAND] Page.bringToFront 실패(무시하고 진행)");
    }
    if let Err(error) = client.call(
        "Emulation.setFocusEmulationEnabled",
        json!({ "enabled": true }),
    ) {
        tracing::debug!(error = %error, "[BAND] setFocusEmulationEnabled 실패(무시하고 진행)");
    }
}

// 네이버 로그인 버튼을 사람처럼 좌표 마우스 클릭한다(네이버 click_login_button 미러). id 값에 점이
// 있어 CSS 이스케이프(#log\.login)가 필요하며, 좌표를 못 구하면 .click()으로 폴백한다.
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

// "네이버로 가입하기" 버튼/링크를 좌표 마우스 클릭한다(못 구하면 .click() 폴백). 이 클릭이
// redirect_external_account_sign_up?type=naver 로 이동시켜 네이버 로그인 폼을 다시 띄운다.
fn click_signup_button(client: &mut CdpClient) -> Result<bool, AutomationError> {
    const CENTER_JS: &str = "(()=>{\
        const els=Array.prototype.slice.call(\
            document.querySelectorAll('a, button, input[type=submit], input[type=button]'));\
        for(const el of els){\
            const t=String(el.innerText||el.textContent||el.value||'').replace(/\\s+/g,' ');\
            if(el.offsetParent!==null&&t.includes('네이버로 가입')){\
                const r=el.getBoundingClientRect();\
                if(r.width>0&&r.height>0)return [r.left+r.width/2, r.top+r.height/2];}}\
        return null;})()";
    if let Some((x, y)) = parse_xy(&client.evaluate(CENTER_JS)?) {
        mouse_click(client, x, y)?;
        return Ok(true);
    }
    // 좌표를 못 구하면 JS .click() 으로 폴백한다.
    Ok(client
        .evaluate_bool(
            "(()=>{const els=Array.prototype.slice.call(\
                document.querySelectorAll('a, button, input[type=submit], input[type=button]'));\
             for(const el of els){\
                const t=String(el.innerText||el.textContent||el.value||'').replace(/\\s+/g,' ');\
                if(el.offsetParent!==null&&t.includes('네이버로 가입')){el.click();return true;}}\
             return false;})()",
        )
        .unwrap_or(false))
}

// OAuth 동의 페이지에서 (1) 전체동의(agree-all) 체크박스를 먼저 체크하고 (2) 동의/확인 버튼을 좌표
// 마우스 클릭한다(best-effort). 자동 리다이렉트라 눌 게 없으면 no-op(false).
fn click_oauth_consent(client: &mut CdpClient) -> Result<bool, AutomationError> {
    // (1) 개인정보 제3자 제공 동의([필수]: 이용자식별자·네이버아이디·이름·이메일·프로필사진 —
    //     service_scope profile/id·naverid·name·naveremail·profileimage) 전부 체크. 하나만 켜던
    //     문제로 자동 선택이 안 됐다. 전체동의 요소 + 모든 체크박스 + 라벨(styled 체크박스 대비)을
    //     눌러 전부 켠다. 동시에 화면 구조(체크박스 상태·버튼 후보·URL)를 진단으로 받아 로그에
    //     원문을 남긴다 — 그래도 안 켜지면 이 로그가 실제 셀렉터를 드러낸다(형님 "로그에 원문" 원칙).
    const CHECK_ALL_JS: &str = "(()=>{\
        const cbs=Array.prototype.slice.call(document.querySelectorAll('input[type=checkbox]'));\
        const all=Array.prototype.slice.call(document.querySelectorAll('label,button,a,span,div'))\
            .find(e=>{const t=String(e.textContent||'').replace(/\\s+/g,'');\
                return t.indexOf('전체동의')>=0||t.indexOf('모두동의')>=0;});\
        if(all)all.click();\
        const before=cbs.map(cb=>cb.checked);\
        for(const cb of cbs){\
            if(!cb.checked)cb.click();\
            if(!cb.checked&&cb.id){const l=document.querySelector('label[for=\"'+cb.id+'\"]');if(l)l.click();}\
            if(!cb.checked&&cb.closest('label'))cb.closest('label').click();}\
        const states=cbs.map(cb=>(cb.id||cb.name||'?')+':'+cb.checked);\
        const btns=Array.prototype.slice.call(\
            document.querySelectorAll('button,a,input[type=submit],input[type=button]'))\
            .map(b=>String(b.innerText||b.textContent||b.value||'').replace(/\\s+/g,' ').trim())\
            .filter(t=>t.length>0&&t.length<24);\
        return JSON.stringify({url:location.href,cbCount:cbs.length,before:before,\
            after:states,allBtn:!!all,buttons:btns.slice(0,15)});})()";
    let diag = client.evaluate_string(CHECK_ALL_JS).unwrap_or_default();
    tracing::info!("[BAND] OAuth 동의 화면 처리(원문 구조) — {diag}");

    // (2) 동의/확인/허용/계속 버튼을 좌표 클릭(id 후보 우선, 없으면 텍스트로).
    const CENTER_JS: &str = "(()=>{\
        const vis=el=>{if(!el)return false;const r=el.getBoundingClientRect();\
            return r.width>0&&r.height>0&&el.offsetParent!==null&&!el.disabled;};\
        let el=document.querySelector('#agree_btn')||document.querySelector('#agree')\
            ||document.querySelector('#btnAgree');\
        if(!vis(el)){el=null;\
            const cs=Array.prototype.slice.call(\
                document.querySelectorAll('button, a, input[type=submit], input[type=button]'));\
            const norm=c=>String(c.innerText||c.textContent||c.value||'').replace(/\\s+/g,'');\
            for(const c of cs){const t=norm(c);\
                if(vis(c)&&!t.includes('전체')&&(t==='동의하기'||t==='동의'||t==='허용하기'||t==='허용'||t==='확인'||t==='계속')){el=c;break;}}\
            if(!vis(el)){for(const c of cs){const t=norm(c);\
                if(vis(c)&&!t.includes('전체')&&(t.indexOf('동의')>=0||t.indexOf('허용')>=0||t.indexOf('계속')>=0||t.indexOf('확인')>=0)){el=c;break;}}}}\
        if(!vis(el))return null;\
        const r=el.getBoundingClientRect();return [r.left+r.width/2, r.top+r.height/2];})()";
    if let Some((x, y)) = parse_xy(&client.evaluate(CENTER_JS)?) {
        mouse_click(client, x, y)?;
        return Ok(true);
    }
    // 좌표를 못 구하면 JS .click() 폴백.
    Ok(client
        .evaluate_bool(
            "(()=>{const vis=el=>{if(!el)return false;const r=el.getBoundingClientRect();\
                return r.width>0&&r.height>0&&el.offsetParent!==null&&!el.disabled;};\
             let el=document.querySelector('#agree_btn')||document.querySelector('#agree')\
                 ||document.querySelector('#btnAgree');\
             if(!vis(el)){el=null;\
                 const cs=Array.prototype.slice.call(\
                     document.querySelectorAll('button, a, input[type=submit], input[type=button]'));\
                 const norm=c=>String(c.innerText||c.textContent||c.value||'').replace(/\\s+/g,'');\
                 for(const c of cs){const t=norm(c);\
                     if(vis(c)&&!t.includes('전체')&&(t==='동의하기'||t==='동의'||t==='허용하기'||t==='허용'||t==='확인'||t==='계속')){el=c;break;}}\
                 if(!vis(el)){for(const c of cs){const t=norm(c);\
                     if(vis(c)&&!t.includes('전체')&&(t.indexOf('동의')>=0||t.indexOf('허용')>=0||t.indexOf('계속')>=0||t.indexOf('확인')>=0)){el=c;break;}}}}\
             if(vis(el)){el.click();return true;}return false;})()",
        )
        .unwrap_or(false))
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

// 선택자를 마우스로 클릭해 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(키 후킹 암호화 대응,
// 네이버 type_into 미러). 입력 후 필드 값 길이를 확인해 비어 있으면 최대 3회 재시도한다.
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<bool, AutomationError> {
    let expected = text.chars().count();
    let seed = jitter_seed();

    // 창이 가려져 visibilityState=hidden 이면 합성 키가 렌더러로 전달되지 않으므로 전경으로 가져온다.
    force_page_foreground(client);

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

// Network.getAllCookies로 band.us 쿠키를 수거한다. getAllCookies는 경로(Path) 제한과 무관하게
// 모든 쿠키를 돌려줘, 게시용 getKey 가 쓰는 `secretKey`(Path=/s/login/getKey, HttpOnly)까지 담는다.
fn collect_band_cookies(client: &mut CdpClient) -> Result<Vec<Value>, AutomationError> {
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

    // --- classify: 우선순위 ---

    #[test]
    fn classify_prioritizes_success() {
        let s = BandPageSignals {
            logged_in: true,
            naver_form: true,
            captcha: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::Success);
    }

    #[test]
    fn classify_naver_form_beats_lower_signals() {
        let s = BandPageSignals {
            naver_form: true,
            consent: true,
            signup_needed: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::NaverForm);
    }

    #[test]
    fn classify_detects_each_action_and_failure_signal() {
        assert_eq!(
            classify(&BandPageSignals {
                consent: true,
                ..Default::default()
            }),
            BandSignal::Consent
        );
        assert_eq!(
            classify(&BandPageSignals {
                signup_needed: true,
                ..Default::default()
            }),
            BandSignal::SignupNeeded
        );
        assert_eq!(
            classify(&BandPageSignals {
                device: true,
                ..Default::default()
            }),
            BandSignal::Device
        );
        assert_eq!(
            classify(&BandPageSignals {
                captcha: true,
                ..Default::default()
            }),
            BandSignal::Captcha
        );
        assert_eq!(
            classify(&BandPageSignals {
                phone_verify: true,
                ..Default::default()
            }),
            BandSignal::PhoneVerify
        );
        assert_eq!(
            classify(&BandPageSignals {
                otp: true,
                ..Default::default()
            }),
            BandSignal::Otp
        );
        assert_eq!(
            classify(&BandPageSignals {
                long_dormant: true,
                ..Default::default()
            }),
            BandSignal::LongDormant
        );
        assert_eq!(
            classify(&BandPageSignals {
                protected: true,
                ..Default::default()
            }),
            BandSignal::Protected
        );
        assert_eq!(
            classify(&BandPageSignals {
                locked: true,
                ..Default::default()
            }),
            BandSignal::Locked
        );
        assert_eq!(
            classify(&BandPageSignals {
                bad_credentials: true,
                ..Default::default()
            }),
            BandSignal::BadCredentials
        );
        assert_eq!(
            classify(&BandPageSignals {
                blocked: true,
                ..Default::default()
            }),
            BandSignal::Blocked
        );
        assert_eq!(classify(&BandPageSignals::default()), BandSignal::Pending);
    }

    #[test]
    fn classify_signup_beats_device_and_below() {
        // 미가입 계정 화면은 새기기/캡차/비번오류보다 앞에서 잡는다.
        let s = BandPageSignals {
            signup_needed: true,
            device: true,
            bad_credentials: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::SignupNeeded);
    }

    // --- decide_loop_step ---

    #[test]
    fn loop_maps_action_signals_directly() {
        assert_eq!(
            decide_loop_step(None, BandSignal::Success, false),
            LoopAction::Success
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::NaverForm, false),
            LoopAction::TypeLogin
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::Consent, false),
            LoopAction::ClickConsent
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::SignupNeeded, false),
            LoopAction::ClickSignup
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::Device, false),
            LoopAction::HandleDevice
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::PhoneVerify, false),
            LoopAction::HandlePhoneVerify
        );
    }

    #[test]
    fn loop_terminal_failures_confirm_immediately_even_headed() {
        for sig in [
            BandSignal::Otp,
            BandSignal::LongDormant,
            BandSignal::Protected,
            BandSignal::Locked,
        ] {
            assert_eq!(
                decide_loop_step(None, sig, true),
                LoopAction::ConfirmedBlocked,
                "{sig:?} 는 headed 여도 즉시 확정해야 한다"
            );
        }
    }

    #[test]
    fn loop_bad_credentials_needs_two_consecutive_polls() {
        assert_eq!(
            decide_loop_step(None, BandSignal::BadCredentials, false),
            LoopAction::KeepWaiting(Some(BandSignal::BadCredentials))
        );
        assert_eq!(
            decide_loop_step(
                Some(BandSignal::BadCredentials),
                BandSignal::BadCredentials,
                false
            ),
            LoopAction::ConfirmedBad
        );
    }

    #[test]
    fn loop_captcha_headless_confirms_over_two_polls_but_headed_waits() {
        assert_eq!(
            decide_loop_step(None, BandSignal::Captcha, false),
            LoopAction::KeepWaiting(Some(BandSignal::Captcha))
        );
        assert_eq!(
            decide_loop_step(Some(BandSignal::Captcha), BandSignal::Captcha, false),
            LoopAction::ConfirmedBlocked
        );
        // headed: 사람이 풀도록 계속 대기(확정하지 않음).
        assert_eq!(
            decide_loop_step(None, BandSignal::Captcha, true),
            LoopAction::KeepWaiting(None)
        );
    }

    #[test]
    fn loop_pending_resets_negative_so_transient_does_not_latch() {
        assert_eq!(
            decide_loop_step(Some(BandSignal::BadCredentials), BandSignal::Pending, false),
            LoopAction::KeepWaiting(None)
        );
    }

    #[test]
    fn credentials_present_rejects_empty_or_whitespace() {
        assert!(credentials_present("user", "pw"));
        assert!(!credentials_present("", "pw"));
        assert!(!credentials_present("   ", "pw"));
        assert!(!credentials_present("user", ""));
    }

    // --- URL / 텍스트 분류(순수 함수) ---

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
        assert!(is_oauth_consent_url(
            "https://nid.naver.com/login/noauth/x?step=agree_term"
        ));
        assert!(!is_oauth_consent_url(
            "https://nid.naver.com/nidlogin.login?mode=form"
        ));
        assert!(!is_oauth_consent_url("https://www.band.us"));
    }

    #[test]
    fn detects_signup_needed_page() {
        assert!(is_signup_needed_url(
            "https://auth.band.us/login?type=naver&ru=x&_ns=false"
        ));
        assert!(!is_signup_needed_url(
            "https://auth.band.us/redirect_external_account_login?type=naver&keep_login=false&rcv=none"
        ));
        assert!(!is_signup_needed_url(
            "https://nid.naver.com/login/noauth/allow_oauth?step=agree_term"
        ));
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

    #[test]
    fn detects_long_dormant_text() {
        assert!(is_long_dormant_text(
            "장기 미로그인 계정입니다. 본인 확인이 필요합니다."
        ));
        assert!(is_long_dormant_text("오랫동안 로그인하지 않으셨습니다"));
        assert!(!is_long_dormant_text("정상적으로 로그인되었습니다"));
        assert!(!is_long_dormant_text(""));
        // URL 기반 정확 감지(패킷 캡처: auth.band.us/b/inactive_user) + 정상 검증 URL은 제외.
        assert!(is_inactive_user_url(
            "https://auth.band.us/b/inactive_user?redirect_url=https%3A%2F%2Fwww.band.us"
        ));
        assert!(!is_inactive_user_url("https://auth.band.us/b/validation_welcome"));
    }

    #[test]
    fn logged_in_only_on_real_band_home() {
        // 성공(패킷 diff): auth 도메인을 벗어난 band.us 홈만 로그인 완료로 본다.
        assert!(is_logged_in_band_url("https://www.band.us/band-create"));
        assert!(is_logged_in_band_url("https://band.us/band/103043410"));
        assert!(is_logged_in_band_url("https://www.band.us/feed"));
        // 인터스티셜(2단계 인증/캡차/가입중)과 네이버 로그인은 성공 아님 — band_session 있어도 반쪽.
        assert!(!is_logged_in_band_url("https://auth.band.us/b/validation_welcome"));
        assert!(!is_logged_in_band_url(
            "https://auth.band.us/b/validation/recaptcha?next_url=https%3A%2F%2Fband.us"
        ));
        assert!(!is_logged_in_band_url("https://auth.band.us/continue_external_account_sign_up"));
        assert!(!is_logged_in_band_url(
            "https://nid.naver.com/oauth2.0/authorize?redirect_uri=https%3A%2F%2Fauth.band.us"
        ));
        assert!(!is_logged_in_band_url(""));
    }

    #[test]
    fn detects_two_factor_gate_text() {
        // 밴드 2단계 인증 게이트 문구(로그 id=175 원문) — 성공 아님(반쪽 세션 게시 거부).
        assert!(is_two_factor_text(
            "BAND 로그인을 위한 2단계 인증이 필요합니다. 인증을 요청해주세요."
        ));
        assert!(is_two_factor_text("추가 2차 인증 절차입니다"));
        assert!(!is_two_factor_text("정상적으로 로그인되었습니다"));
        assert!(!is_two_factor_text(""));
    }

    #[test]
    fn detects_locked_text() {
        assert!(is_locked_text("아이디 잠금조치와 함께 안내드립니다"));
        assert!(is_locked_text(
            "비정상적인 활동이 감지되어 아이디를 보호(잠금) 조치중입니다"
        ));
        assert!(!is_locked_text("아이디 또는 비밀번호가 올바르지 않습니다"));
    }

    #[test]
    fn id_phone_format_branch() {
        assert!(id_is_phone_format("01012345678"));
        assert!(!id_is_phone_format("0101234567")); // 10자리
        assert!(!id_is_phone_format("010123456789")); // 12자리
        assert!(!id_is_phone_format("02012345678")); // 010 아님
        assert!(!id_is_phone_format("0101234567a")); // 숫자 아님
        assert!(!id_is_phone_format("myid@naver.com"));
    }

    // --- key_info / type_delay / parse_xy / cookies ---

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
    fn key_info_shifted_symbol_maps_to_base_digit_with_shift() {
        let bang = key_info('!');
        assert_eq!(bang.code, "Digit1");
        assert_eq!(bang.vk, 0x31);
        assert!(bang.shift);
    }

    #[test]
    fn key_info_unknown_char_is_best_effort_zero() {
        let k = key_info('가');
        assert_eq!(k.vk, 0);
        assert_eq!(k.code, "");
        assert!(!k.shift);
    }

    #[test]
    fn type_delay_always_within_human_range() {
        for seed in [0u64, 1, 42, 9_999, u64::MAX] {
            for index in 0..64 {
                let d = type_delay_ms(seed, index);
                assert!((TYPE_DELAY_MIN_MS..=TYPE_DELAY_MAX_MS).contains(&d));
            }
        }
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
        assert!(!has_band_session_cookies(&[json!({ "name": "BUC" })]));
        assert!(!has_band_session_cookies(&[]));
    }

    // --- 폼 게이트(순수 함수) ---

    #[test]
    fn form_gate_requires_all_four_conditions() {
        assert!(login_form_gate_open(true, true, true, true));
        assert!(!login_form_gate_open(false, true, true, true));
        assert!(!login_form_gate_open(true, false, true, true));
        assert!(!login_form_gate_open(true, true, false, true));
        assert!(!login_form_gate_open(true, true, true, false));
    }

    #[test]
    fn ready_streak_accumulates_and_resets_on_flap() {
        let mut s = 0;
        s = next_ready_streak(s, true);
        assert_eq!(s, 1);
        s = next_ready_streak(s, true);
        assert_eq!(s, 2);
        s = next_ready_streak(s, false);
        assert_eq!(s, 0);
        s = next_ready_streak(s, true);
        s = next_ready_streak(s, true);
        s = next_ready_streak(s, true);
        assert!(s >= FORM_READY_STABLE_POLLS);
    }
}
