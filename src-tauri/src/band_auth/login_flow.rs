//! band.us 이메일 로그인 CDP 시퀀스. 네이버 `auth/login_flow.rs` 의 메커니즘을
//! **그대로 미러**한다: 페이지에서 API(fetch)를 호출하지 않고(봇탐지 표면↑) DOM 을 몰아 —
//! 이메일/비밀번호를 실제 키 이벤트(`Input.dispatchKeyEvent`)로 입력하고, 버튼은 좌표 마우스
//! 클릭(진짜 mouse 이벤트, JS `.click()` 아님)으로 누르며, `document.readyState==='complete'` 를
//! 기다린 뒤 쿠키 + 현재 URL + DOM 텍스트로 페이지 상태를 분류한다.
//!
//! `www.band.us`에서 시작해 소개 화면의 로그인 버튼과 로그인 방법 화면의 이메일 버튼을 실제로
//! 눌러 `email_login?keep_login=false`에 진입한다. 이후 이메일 폼(`#email_login_form`,
//! `#input_email`)을 제출하면 비밀번호 폼(`/email_login/password`,
//! `#email_password_login_form`, `#pw`)으로 이동한다. 각 화면은 전체 DOM + iframe 완료와 정확한
//! 폼 준비를 별도로 기다린 뒤 입력·제출한다. band 가 세션을 세우면 `.band.us`에
//! `band_session` 쿠키가 발급되고 실제 `www.band.us` 홈에 착지한 때만 성공으로 확정한다.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::auth::login_flow::type_into_immediate as type_into_naver;
use crate::naver_automation::{AutomationError, CdpClient};

// 정상 수동 로그인 패킷(2026-07-31)과 같은 진입점/버튼. BBC(device_id)는 코드에서 만들거나
// 재사용하지 않는다. 계정별 새 Chrome 세션이 이 정상 페이지 흐름의 band 스크립트를 실행하면서
// 각자 발급받게 한다.
const BAND_HOME_ENTRY_URL: &str = "https://www.band.us/";
const BAND_INTRO_LOGIN_SELECTOR: &str = "a.login._loginLink";
const BAND_EMAIL_METHOD_SELECTOR: &str = "a.buttonRound.-email[data-login-method='email']";
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

// 직접 이메일 로그인 폼(패킷/HTML 원문 2026-07-31).
const BAND_EMAIL_FORM_SELECTOR: &str = "#email_login_form";
const BAND_EMAIL_SELECTOR: &str = "#input_email";
const BAND_PASSWORD_FORM_SELECTOR: &str = "#email_password_login_form";

// 예기치 않은 레거시 네이버 OAuth 착지의 기존 방어적 처리용 셀렉터.
const NAVER_ID_SELECTOR: &str = "#id";
const NAVER_PW_SELECTOR: &str = "#pw";

// 가입 마지막 단계에서 밴드가 요구하는 생년월일 기본값(형님 지시: 고정 기본값 자동입력). 계정 정보에
// 생년월일이 없으므로 성인·연령제한 회피를 위해 이 값을 넣는다. YYYY/MM/DD 조각으로도 쓴다.
const DEFAULT_SIGNUP_BIRTH_YEAR: &str = "1990";
const DEFAULT_SIGNUP_BIRTH_MONTH: &str = "01";
const DEFAULT_SIGNUP_BIRTH_DAY: &str = "01";

// 폼 준비/결과 DOM 이 흔들리지 않고 자리잡았다고 볼 연속 확인 횟수(네이버 미러). 상위 문서가
// complete 된 뒤에도 캡차/안티봇 iframe·스크립트가 뒤늦게 로드되며 DOM 이 잠깐 출렁이므로,
// 그 과도기에 타이핑/클릭하지 않도록 연속 N회 안정될 때만 진행한다.
// (로그인 폼 게이트/안티봇/리소스정착 판정은 네이버 auth::login_flow::wait_for_login_form을 그대로
//  재사용한다 — 밴드 중복 상수/헬퍼는 2026-07-16 제거. FORM_READY_STABLE_POLLS·ANTIBOT_READY_JS·
//  RESOURCE_COUNT_JS는 그 함수 안으로 이동됨.)

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
        return el.offsetParent!==null&&(t.includes('네이버로')&&t.includes('가입'));});})()";

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

// band 자체 비밀번호 화면(`auth.band.us/email_login/password`)의 비밀번호 오류 표시(순수 판정 JS).
// 패킷 원문(2026-07-15): 비번 틀리면 같은 페이지가 리로드되며 `<p id="error_msg">계정이 없거나
// 비밀번호가 일치하지 않습니다.</p>`가 보인다. 네이버 폼의 `#err_common`과 별개 셀렉터다.
const BAND_PW_ERROR_JS: &str = "(()=>{const e=document.querySelector('#error_msg');\
    return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()";

// 밴드 2단계 인증/추가 검증 게이트 화면인지 본문 텍스트로 판정한다(순수 함수). band_session 쿠키가
// 이 화면(`auth.band.us/b/validation_welcome`)에서 이미 발급되지만, 이는 인증 미완료 반쪽 세션이라
// 게시가 거부된다("session expired"/"not authorized") — 성공으로 보면 안 된다(2026-07-13 로그 확인).
pub(crate) fn is_two_factor_text(text: &str) -> bool {
    text.contains("2단계 인증") || text.contains("2차 인증") || text.contains("2단계 인증이 필요")
}

/// 로그인 결과.
pub(crate) enum BandLoginOutcome {
    Ok {
        cookies: Vec<Value>,
    },
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
    /// band 이메일 입력 화면(`/email_login`, `#input_email`). 비밀번호 단계와 구분하며, 아직
    /// 이번 폼을 제출하지 않았고 오류/캡차가 없을 때만 true다.
    pub email_form: bool,
    /// 표준 네이버 로그인 폼(#id/#pw)이 이번 폴링에서 **입력해야 하는** 상태(보이고, 아직 이번
    /// 폼에 제출하지 않았고, 캡차/오류 표시가 없음). 첫 로그인·가입 재로그인 모두 여기로 잡힌다.
    pub naver_form: bool,
    /// band "본인 확인" 화면(`confirm_user_email_login`): "로그인 하기" 클릭 대상.
    pub email_confirm: bool,
    /// band 자체 비밀번호 입력 화면(`email_login/password`): `#pw` 입력 + "확인" 클릭 대상.
    /// 오류(#error_msg)가 떠 있거나 이번 화면에 이미 제출했으면 false 로 둬 재입력하지 않는다.
    pub email_password: bool,
    /// OAuth 동의 페이지(`allow_oauth`/`agree_term`).
    pub consent: bool,
    /// 미가입 계정 화면(`/login?...&_ns=false` 또는 "네이버로 밴드 가입" 버튼).
    pub signup_needed: bool,
    /// 가입 마지막 단계(`external_account_sign_up`): 생년월일 입력 + 전체동의 화면.
    pub signup_final: bool,
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
    EmailForm,
    NaverForm,
    EmailConfirm,
    EmailPassword,
    Consent,
    SignupNeeded,
    SignupFinal,
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
    TypeBandEmail,
    TypeLogin,
    ClickEmailConfirm,
    TypeBandPassword,
    ClickConsent,
    ClickSignup,
    HandleSignupFinal,
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
    } else if s.email_form {
        BandSignal::EmailForm
    } else if s.naver_form {
        BandSignal::NaverForm
    } else if s.email_confirm {
        BandSignal::EmailConfirm
    } else if s.email_password {
        BandSignal::EmailPassword
    } else if s.consent {
        BandSignal::Consent
    } else if s.signup_needed {
        BandSignal::SignupNeeded
    } else if s.signup_final {
        BandSignal::SignupFinal
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
        BandSignal::EmailForm => LoopAction::TypeBandEmail,
        BandSignal::NaverForm => LoopAction::TypeLogin,
        BandSignal::EmailConfirm => LoopAction::ClickEmailConfirm,
        BandSignal::EmailPassword => LoopAction::TypeBandPassword,
        BandSignal::Consent => LoopAction::ClickConsent,
        BandSignal::SignupNeeded => LoopAction::ClickSignup,
        BandSignal::SignupFinal => LoopAction::HandleSignupFinal,
        BandSignal::Device => LoopAction::HandleDevice,
        BandSignal::PhoneVerify => LoopAction::HandlePhoneVerify,
        // 사람이 즉석에서 풀 수 없는 종료 상태 — headed 여도 즉시 실패(Blocked 매핑).
        BandSignal::Otp | BandSignal::LongDormant | BandSignal::Protected | BandSignal::Locked => {
            LoopAction::ConfirmedBlocked
        }
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

/// 정상 band 로그인 진입 URL(순수 함수). 이메일 로그인 주소로 직행하지 않는다.
pub(crate) fn normal_login_entry_url() -> &'static str {
    BAND_HOME_ENTRY_URL
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
/// `auth.band.us/login?...&_ns=false` 로 떨궈 "네이버로 밴드 가입"을 눌러야 한다.
pub(crate) fn is_signup_needed_url(url: &str) -> bool {
    url.contains("_ns=false")
}

/// 현재 URL이 가입 마지막 단계(`external_account_sign_up`)인지(순수 함수). 미가입 계정이
/// "네이버로 밴드 가입"을 눌러 네이버 재인증까지 마치면 이 화면에서 생년월일 + 약관동의를 받는다.
pub(crate) fn is_signup_final_url(url: &str) -> bool {
    url.contains("external_account_sign_up")
}

/// 본문 텍스트가 가입 마지막 단계 화면인지(순수 함수, URL 판정 폴백). "가입 마지막 단계"와
/// "생년월일"이 함께 있으면 가입 완료 폼으로 본다(패킷 원문 문구, 2026-07-13).
pub(crate) fn is_signup_final_text(text: &str) -> bool {
    text.contains("가입 마지막 단계") && text.contains("생년월일")
}

/// 현재 URL이 band "본인 확인" 화면(`confirm_user_email_login`)인지(순수 함수). 네이버 OAuth
/// 통과 후 band 가 "본인이 맞으신가요? / 당신의 계정이라면 로그인하세요."를 띄우고 "로그인 하기"
/// (`<a href='/email_login/password'>`)를 누르면 band 자체 비밀번호 입력 화면으로 넘어간다(패킷
/// 원문 2026-07-15, `auth.band.us`).
pub(crate) fn is_email_confirm_url(url: &str) -> bool {
    url.contains("confirm_user_email_login")
}

/// 본문 텍스트가 band "본인 확인" 화면인지(순수 함수, URL 판정 폴백). 패킷 원문 문구.
pub(crate) fn is_email_confirm_text(text: &str) -> bool {
    text.contains("본인이 맞으신가요") && text.contains("로그인")
}

/// 현재 URL이 band 이메일 입력 첫 화면인지 판정한다. `/email_login/password`도 접두사가
/// 같으므로 명시적으로 제외한다.
pub(crate) fn is_email_login_url(url: &str) -> bool {
    url.contains("auth.band.us/email_login") && !is_email_password_url(url)
}

/// 현재 URL이 band 자체 비밀번호 입력 화면(`email_login/password`)인지(순수 함수). `#pw`에 저장된
/// 비밀번호를 타이핑하고 "확인"(`#email_password_login_form` submit)을 누른다. 이 화면의 `#pw`는
/// 네이버 폼 `#id/#pw`와 달리 `#id`가 없어 네이버 폼 감지(`#id`+`#pw`)와 충돌하지 않는다.
pub(crate) fn is_email_password_url(url: &str) -> bool {
    url.contains("email_login/password")
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

// 정상 수동 로그인 패킷의 입구를 그대로 밟는다:
// www.band.us → 소개 화면 로그인 → auth.band.us 로그인 방법 → 이메일 로그인.
// 프론트엔드는 이 흐름에 관여하지 않으며, 계정별로 만들어진 현재 CDP 세션 안에서만 수행한다.
fn enter_email_login_via_normal_path(
    client: &mut CdpClient,
) -> Result<Option<String>, AutomationError> {
    client.navigate(normal_login_entry_url())?;

    // www.band.us 첫 응답 뒤 /about/kr/intro로 전환될 수 있으므로 URL을 강제로 건너뛰지 않고,
    // 소개 화면의 실제 로그인 버튼과 모든 문서가 안정적으로 준비될 때까지 기다린다.
    if !wait_for_visible_selector(client, BAND_INTRO_LOGIN_SELECTOR, Duration::from_secs(20)) {
        let url = client.current_url().unwrap_or_default();
        return Ok(Some(format!(
            "BAND 소개 화면의 로그인 버튼이 DOM 로딩 후에도 준비되지 않았습니다. 마지막 페이지: {url}"
        )));
    }
    // 페이지 진입 텔레메트리/BBC 초기화가 같은 이벤트 루프에서 마무리될 시간을 준 뒤 실제
    // 좌표 마우스 이벤트로 누른다.
    sleep(Duration::from_secs(1));
    if !click_visible_selector(client, BAND_INTRO_LOGIN_SELECTOR)? {
        let url = client.current_url().unwrap_or_default();
        return Ok(Some(format!(
            "BAND 소개 화면의 로그인 버튼을 누르지 못했습니다. 마지막 페이지: {url}"
        )));
    }

    // 로그인 방법 페이지가 완전히 로드되고 이메일 선택 버튼이 안정된 뒤에만 다음 클릭을 한다.
    if !wait_for_visible_selector(client, BAND_EMAIL_METHOD_SELECTOR, Duration::from_secs(20)) {
        let url = client.current_url().unwrap_or_default();
        return Ok(Some(format!(
            "BAND 로그인 방법 화면의 이메일 로그인 버튼이 DOM 로딩 후에도 준비되지 않았습니다. 마지막 페이지: {url}"
        )));
    }
    sleep(Duration::from_secs(1));
    if !click_visible_selector(client, BAND_EMAIL_METHOD_SELECTOR)? {
        let url = client.current_url().unwrap_or_default();
        return Ok(Some(format!(
            "BAND 로그인 방법 화면의 이메일 로그인 버튼을 누르지 못했습니다. 마지막 페이지: {url}"
        )));
    }

    // 버튼 클릭으로 이동한 이메일 화면도 전체 DOM과 정확한 폼이 안정될 때까지 기다린다.
    // 성공 뒤 기존 반응형 루프가 동일 게이트를 다시 확인하고 기존 입력/제출/쿠키 저장을 재사용한다.
    if !wait_for_band_form(client, BAND_EMAIL_FORM_SELECTOR, BAND_EMAIL_SELECTOR) {
        let url = client.current_url().unwrap_or_default();
        return Ok(Some(format!(
            "BAND 이메일 로그인 화면이 정상 버튼 경로 뒤에도 준비되지 않았습니다. 마지막 페이지: {url}"
        )));
    }

    Ok(None)
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

    // 정상 수동 경로와 동일하게 band 홈 → 소개 화면 로그인 → 로그인 방법 화면의 이메일 버튼을
    // 실제 좌표 마우스로 누른다. 이 과정에서 band 자체 스크립트가 새 세션의 BBC/device_id와
    // 텔레메트리 문맥을 만들게 하며, 값을 코드에서 고정·복사하지 않는다.
    if let Some(message) = enter_email_login_via_normal_path(client)? {
        return Ok(BandLoginOutcome::Error(message));
    }

    let timeout = if wait_for_human {
        HEADED_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    let deadline = Instant::now() + timeout;
    let mut last_negative: Option<BandSignal> = None;
    // 이메일 첫 화면에 이미 입력·제출했는지. POST 직후 같은 DOM이 잠깐 남아 있을 때 이중
    // 제출하는 것을 막는다.
    let mut email_submitted = false;
    // 현재 표시된 네이버 폼에 이미 입력·제출했는지. 첫 로그인 후 다시 폼이 뜨는 경우는 오직
    // "가입하기" 클릭 뒤이므로(재로그인), 그 arm 에서만 false 로 되돌려 재입력을 허용한다. 이 플래그로
    // 제출 직후 같은 폼에 이중 제출하는 것을 막는다.
    let mut submitted_for_form = false;
    // band 자체 비밀번호 화면(#pw)에 이미 입력·제출했는지(제출 직후 recaptcha 처리 중 재입력 방지).
    let mut band_pw_submitted = false;
    // 본인확인(휴대전화) 번호 입력·확인을 이미 1회 시도했는지(중복 제출 방지).
    let mut phone_attempted = false;
    // 첫 타이핑 직전 1회만 웹 컨텐츠로 창 포커스를 옮긴다(주소창 선택 해제 → 캡차 완화, 네이버 미러).
    let mut focused_once = false;

    loop {
        // (a) 인증 성공 직후/새 기기 확인 페이지의 "등록 안함" 다이얼로그를 best-effort 로 눌러 마무리한다
        // (네이버와 동일한 CdpClient 헬퍼 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        // 새로 이동한 페이지를 읽거나 동작하기 전에 DOM 로딩 완료(readyState=complete + 모든 iframe)
        // 를 반드시 확인한다. 예전처럼 결과를 무시하고 입력을 진행하지 않는다.
        if !wait_for_dom_ready(client) {
            let url = client.current_url().unwrap_or_default();
            return Ok(BandLoginOutcome::Error(format!(
                "BAND 로그인 페이지 DOM 로딩이 완료되지 않았습니다. 마지막 페이지: {url}"
            )));
        }

        let signals = read_signals(
            client,
            email_submitted,
            submitted_for_form,
            band_pw_submitted,
        )?;
        match decide_loop_step(last_negative, classify(&signals), wait_for_human) {
            LoopAction::Success => {
                let cookies = collect_band_cookies(client)?;
                return Ok(BandLoginOutcome::Ok { cookies });
            }
            LoopAction::TypeBandEmail => {
                // 첫 이메일 화면은 자체 DOM 게이트를 한 번 더 통과해야 한다. 폼·입력·submit
                // 버튼이 안정적으로 준비되기 전에는 계정관리의 이메일을 입력하지 않는다.
                if !wait_for_band_form(client, BAND_EMAIL_FORM_SELECTOR, BAND_EMAIL_SELECTOR) {
                    let url = client.current_url().unwrap_or_default();
                    return Ok(BandLoginOutcome::Error(format!(
                        "BAND 이메일 로그인 폼(#email_login_form/#input_email)이 준비되지 않았습니다. 마지막 페이지: {url}"
                    )));
                }
                if !focused_once {
                    focus_web_contents(client);
                    focused_once = true;
                }
                if !type_into(client, BAND_EMAIL_SELECTOR, id)? {
                    return Ok(BandLoginOutcome::Error(
                        "BAND 이메일 자동 입력에 실패했습니다(이메일 칸이 비어 로그인을 중단)."
                            .to_owned(),
                    ));
                }
                sleep(Duration::from_secs(1));
                click_band_form_submit(client, BAND_EMAIL_FORM_SELECTOR, BAND_EMAIL_SELECTOR)?;
                email_submitted = true;
                last_negative = None;
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
            LoopAction::ClickEmailConfirm => {
                // "본인이 맞으신가요?" 화면의 "로그인 하기"(a[href='/email_login/password']) 클릭 →
                // band 자체 비밀번호 입력 화면으로 이동. 다음 폴링에서 EmailPassword 로 재판정된다.
                let _ = click_email_confirm(client);
                last_negative = None;
            }
            LoopAction::TypeBandPassword => {
                // band 자체 비밀번호 화면: 하위com 에 저장된 그 계정 비번(pw)을 #pw 에 한 글자씩
                // 타이핑한다. 실제 keyup 이벤트가 band 의 checkConfirmButton() 을 돌려 "확인" 버튼을
                // 활성화하고, 이어서 폼 제출(→ band recaptcha·서명 JS)까지 눌러 마무리한다. 비번이
                // 틀리면 다음 폴링에서 #error_msg → BadCredentials 로 확정된다. (DOM 완료는 루프
                // 상단 wait_for_dom_ready 가 이미 보장한다.)
                if !wait_for_band_form(client, BAND_PASSWORD_FORM_SELECTOR, "#pw") {
                    let url = client.current_url().unwrap_or_default();
                    return Ok(BandLoginOutcome::Error(format!(
                        "BAND 비밀번호 로그인 폼(#email_password_login_form/#pw)이 준비되지 않았습니다. 마지막 페이지: {url}"
                    )));
                }
                if !type_into(client, "#pw", pw)? {
                    return Ok(BandLoginOutcome::Error(
                        "밴드 비밀번호 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단)."
                            .to_owned(),
                    ));
                }
                sleep(Duration::from_secs(1));
                click_band_password_submit(client)?;
                band_pw_submitted = true;
                last_negative = None;
            }
            LoopAction::ClickConsent => {
                let _ = click_oauth_consent(client);
                last_negative = None;
            }
            LoopAction::ClickSignup => {
                // "네이버로 밴드 가입"을 눌러 external_account_sign_up 으로 이동시킨다. 그러면
                // 네이버 로그인 폼이 다시 뜨므로 재입력을 허용하도록 제출 플래그를 되돌린다.
                let _ = click_signup_button(client);
                submitted_for_form = false;
                last_negative = None;
            }
            LoopAction::HandleSignupFinal => {
                // 가입 마지막 단계(생년월일 + 전체동의). 생년월일은 계정 정보에 없으므로 고정
                // 기본값(DEFAULT_SIGNUP_BIRTHDATE)을 넣고 전체동의 후 완료 버튼을 누른다(형님 지시:
                // 고정 기본값 자동입력). DOM 구조를 원문 로그로 남겨 셀렉터가 틀리면 드러나게 한다.
                let _ = handle_signup_final(client);
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
    email_submitted: bool,
    submitted_for_form: bool,
    band_pw_submitted: bool,
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
    // band 이메일/비밀번호 화면에서 오류(#error_msg)가 떠 있는지. band 페이지 고유 셀렉터라
    // has_session 게이트 없이도 오탐이 없다. 오류가 뜨면 재입력하지 않고 BadCredentials로 확정한다.
    let band_email_page = is_email_login_url(&url);
    let band_pw_page = is_email_password_url(&url);
    let band_form_error = (band_email_page || band_pw_page)
        && client.evaluate_bool(BAND_PW_ERROR_JS).unwrap_or(false);

    // 비번 오류: 네이버 폼 #err_common(보이고 텍스트 있음, 캡차/로그인됨 아님) 또는 band #error_msg.
    let bad_credentials = band_form_error
        || (!has_session
            && !captcha
            && client
                .evaluate_bool(
                    "(()=>{const e=document.querySelector('#err_common');\
                     return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()",
                )
                .unwrap_or(false));

    // band 이메일 첫 화면: 정확한 URL + 폼/입력이 보이고, 아직 제출하지 않았고,
    // 오류/캡차가 없을 때만 입력 액션으로 분류한다.
    let email_form_visible = client
        .evaluate_bool(
            "(()=>{const f=document.querySelector('#email_login_form');\
             const e=document.querySelector('#input_email');\
             const b=f&&f.querySelector('button[type=submit]');\
             return !!(f&&f.offsetParent!==null&&e&&e.offsetParent!==null&&!e.disabled&&!e.readOnly\
                 &&b&&b.offsetParent!==null);})()",
        )
        .unwrap_or(false);
    let email_form =
        band_email_page && email_form_visible && !email_submitted && !band_form_error && !captcha;

    // band "본인 확인"(confirm_user_email_login) 화면: URL 또는 본문 문구. 직접 이메일
    // 로그인에서는 정상적으로 거치지 않지만 기존 후속 방어 로직은 유지한다.
    let email_confirm = is_email_confirm_url(&url) || is_email_confirm_text(&body_text);
    // band 자체 비밀번호 입력 화면: 이 화면이고, 오류가 없고, 아직 이번 화면에 제출하지 않았을 때만
    // "입력해야 하는" 상태로 본다(오류 뜬 화면·제출 직후 recaptcha 대기 중 재입력 방지).
    let email_password = band_pw_page && !band_form_error && !band_pw_submitted;
    // 폼이 보이고, 아직 이번 폼에 제출하지 않았고, 캡차/오류 표시가 없을 때만 "입력해야 하는" 폼으로
    // 본다 — 오류가 뜬 폼(비번오류)이나 캡차가 뜬 폼에 재입력하지 않는다.
    let naver_form =
        form_visible && !submitted_for_form && !captcha && !bad_credentials && !has_session;

    // 동의 화면 감지: URL(allow_oauth/agree_term) 또는 화면 DOM(동의하기 버튼 + 개인정보 제3자 제공).
    // 실제 동의 화면 URL 은 oauth2.0/authorize 라 URL 만으론 못 잡아 DOM 감지가 주력이다(2026-07-13).
    let consent =
        is_oauth_consent_url(&url) || client.evaluate_bool(CONSENT_DOM_JS).unwrap_or(false);
    let signup_needed =
        is_signup_needed_url(&url) || client.evaluate_bool(SIGNUP_BUTTON_JS).unwrap_or(false);
    // 가입 마지막 단계: URL(external_account_sign_up) 또는 본문("가입 마지막 단계"+"생년월일").
    let signup_final = is_signup_final_url(&url) || is_signup_final_text(&body_text);
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
        email_form,
        naver_form,
        email_confirm,
        email_password,
        consent,
        signup_needed,
        signup_final,
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

// 상위 문서와 읽을 수 있는 iframe이 모두 complete이고, 지정 요소가 화면에 보이는 상태가 연속
// 3회 유지될 때까지 기다린다. 정상 진입의 두 버튼 모두 DOM 완료 전에 절대 누르지 않는다.
fn wait_for_visible_selector(client: &mut CdpClient, selector: &str, timeout: Duration) -> bool {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned());
    let probe = format!(
        "(()=>{{\
            if(!({ALL_DOCS_COMPLETE_JS}))return false;\
            const e=document.querySelector({selector});\
            if(!e||e.offsetParent===null)return false;\
            const r=e.getBoundingClientRect();\
            return r.width>0&&r.height>0&&!e.disabled;\
        }})()"
    );
    let deadline = Instant::now() + timeout;
    let mut stable_polls = 0u8;
    loop {
        if client.evaluate_bool(&probe).unwrap_or(false) {
            stable_polls += 1;
            if stable_polls >= 3 {
                return true;
            }
        } else {
            stable_polls = 0;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

// 화면에 보이는 정확한 셀렉터의 중앙을 CDP 실제 마우스 이벤트로 누른다. 좌표 계산이 불가능한
// 예외적인 경우에만 페이지 고유 click 핸들러를 보존하는 JS click으로 폴백한다.
fn click_visible_selector(client: &mut CdpClient, selector: &str) -> Result<bool, AutomationError> {
    let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned());
    let center = client.evaluate(&format!(
        "(()=>{{const e=document.querySelector({selector});\
         if(!e||e.offsetParent===null)return null;\
         const r=e.getBoundingClientRect();if(r.width<=0||r.height<=0)return null;\
         return [r.left+r.width/2,r.top+r.height/2];}})()"
    ))?;
    if let Some((x, y)) = parse_xy(&center) {
        mouse_click(client, x, y)?;
        return Ok(true);
    }

    Ok(client
        .evaluate_bool(&format!(
            "(()=>{{const e=document.querySelector({selector});\
             if(!e||e.offsetParent===null)return false;e.click();return true;}})()"
        ))
        .unwrap_or(false))
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

// BAND 이메일/비밀번호 각 화면의 전체 DOM과 정확한 폼 구조가 안정적으로 준비될 때까지
// 기다린다. submit 버튼은 입력 전 disabled가 정상이라 존재·표시만 확인하고, 실제 키 이벤트가
// 페이지 JS를 거쳐 활성화하도록 기존 제출 헬퍼에 맡긴다.
fn wait_for_band_form(client: &mut CdpClient, form_selector: &str, input_selector: &str) -> bool {
    let form = serde_json::to_string(form_selector).unwrap_or_else(|_| "\"\"".to_owned());
    let input = serde_json::to_string(input_selector).unwrap_or_else(|_| "\"\"".to_owned());
    let probe = format!(
        "(()=>{{\
            if(!({ALL_DOCS_COMPLETE_JS}))return false;\
            const f=document.querySelector({form});\
            const i=document.querySelector({input});\
            const b=f&&f.querySelector('button[type=submit]');\
            const visible=e=>!!(e&&e.offsetParent!==null&&e.getBoundingClientRect().width>0\
                &&e.getBoundingClientRect().height>0);\
            return visible(f)&&visible(i)&&!i.disabled&&!i.readOnly&&visible(b);\
        }})()"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut stable_polls = 0u8;
    loop {
        if client.evaluate_bool(&probe).unwrap_or(false) {
            stable_polls += 1;
            if stable_polls >= 3 {
                return true;
            }
        } else {
            stable_polls = 0;
        }
        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100));
    }
}

// 네이버 로그인 폼(밴드도 네이버 OAuth 로그인 폼을 그대로 받는다)이 완전히 로딩될 때까지 기다린다.
// **단일 소스 재사용(2026-07-16)**: 예전엔 밴드가 네이버 게이트를 복사했다가 네이버 v3→v4 폼 변경 때
// 밴드만 안 고쳐져 로그인 폼에서 무한 대기하는 사고가 났다(실기기 로그). 이제 네이버 구현을 직접
// 재사용해, 네이버 로그인 폼이 또 바뀌어도 한 곳(auth::login_flow)만 고치면 밴드도 같이 반영된다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    crate::auth::login_flow::wait_for_login_form(client)
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

// 네이버 로그인 버튼을 사람처럼 좌표 마우스 클릭한다(네이버 click_login_button 미러). id 값에 점이
// 있어 CSS 이스케이프(#log\.login)가 필요하며, 좌표를 못 구하면 .click()으로 폴백한다.
fn click_login_button(client: &mut CdpClient) -> Result<(), AutomationError> {
    // 단일 소스 재사용(2026-07-16): 네이버 로그인 버튼 클릭 구현을 그대로 쓴다(구/신 v4 폼 모두 지원).
    crate::auth::login_flow::click_login_button(client)
}

// band "본인이 맞으신가요?" 화면의 "로그인 하기"를 클릭한다. 이 버튼은
// `<a href='/email_login/password' class="uBtn -tcType -confirm">로그인 하기</a>`(패킷 원문
// 2026-07-15)라 href 로 정확히 잡고, 못 잡으면 버튼 텍스트("로그인하기")로 폴백한다. 좌표 마우스
// 클릭 후 폴백으로 .click(). 클릭하면 band 자체 비밀번호 입력 화면으로 이동한다.
fn click_email_confirm(client: &mut CdpClient) -> Result<bool, AutomationError> {
    const CENTER_JS: &str = "(()=>{\
        let b=document.querySelector(\"a[href*='email_login/password']\");\
        if(!b){const els=Array.prototype.slice.call(document.querySelectorAll('a, button'));\
            b=els.find(el=>el.offsetParent!==null&&\
                String(el.innerText||el.textContent||'').replace(/\\s+/g,'').includes('로그인하기'));}\
        if(!b)return null;const r=b.getBoundingClientRect();\
        if(r.width<=0||r.height<=0)return null;return [r.left+r.width/2, r.top+r.height/2];})()";
    if let Some((x, y)) = parse_xy(&client.evaluate(CENTER_JS)?) {
        mouse_click(client, x, y)?;
        return Ok(true);
    }
    Ok(client
        .evaluate_bool(
            "(()=>{let b=document.querySelector(\"a[href*='email_login/password']\");\
             if(!b){const els=Array.prototype.slice.call(document.querySelectorAll('a, button'));\
                b=els.find(el=>el.offsetParent!==null&&\
                    String(el.innerText||el.textContent||'').replace(/\\s+/g,'').includes('로그인하기'));}\
             if(b){b.click();return true;}return false;})()",
        )
        .unwrap_or(false))
}

// band 자체 비밀번호 화면의 "확인"(`#email_password_login_form` 의 submit)을 눌러 제출한다. 이 버튼은
// 처음 disabled 이고 band 의 checkConfirmButton() 이 입력 이벤트에서 활성화한다 — type_into 의 실제
// keyup 이 이미 활성화하지만, 안전하게 입력 이벤트를 한 번 더 흘리고(disabled 해제) 좌표 마우스
// 클릭으로 제출한다. 제출은 band 폼 핸들러(recaptcha·서명 JS)를 태운다. 좌표를 못 구하면
// requestSubmit()/click() 폴백(둘 다 submit 이벤트를 발화해 recaptcha 를 태운다).
fn click_band_password_submit(client: &mut CdpClient) -> Result<(), AutomationError> {
    click_band_form_submit(client, BAND_PASSWORD_FORM_SELECTOR, "#pw")
}

// BAND 이메일/비밀번호 폼은 동일한 submit 구조와 입력 이벤트 활성화 방식을 쓴다. 기존
// 비밀번호 제출 구현을 공통화해 두 단계 모두 페이지 자체 서명/reCAPTCHA submit 핸들러를 탄다.
fn click_band_form_submit(
    client: &mut CdpClient,
    form_selector: &str,
    input_selector: &str,
) -> Result<(), AutomationError> {
    let form = serde_json::to_string(form_selector).unwrap_or_else(|_| "\"\"".to_owned());
    let input = serde_json::to_string(input_selector).unwrap_or_else(|_| "\"\"".to_owned());
    let _ = client.evaluate(&format!(
        "(()=>{{const p=document.querySelector({input});\
         if(p){{['keyup','input','change'].forEach(t=>p.dispatchEvent(new Event(t,{{bubbles:true}})));}}\
         const f=document.querySelector({form});\
         const b=f&&f.querySelector('button[type=submit]');if(b)b.disabled=false;return true;}})()"
    ))?;
    let center = client.evaluate(&format!(
        "(()=>{{const f=document.querySelector({form});\
         const b=f&&f.querySelector('button[type=submit]');if(!b)return null;\
         const r=b.getBoundingClientRect();if(r.width<=0||r.height<=0)return null;\
         return [r.left+r.width/2, r.top+r.height/2];}})()"
    ))?;
    if let Some((x, y)) = parse_xy(&center) {
        mouse_click(client, x, y)?;
    } else {
        client.evaluate(&format!(
            "(()=>{{const f=document.querySelector({form});if(!f)return false;\
             const b=f.querySelector('button[type=submit]');\
             if(f.requestSubmit){{f.requestSubmit(b||undefined);}}else if(b){{b.click();}}else{{f.submit();}}\
             return true;}})()"
        ))?;
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
            if(el.offsetParent!==null&&(t.includes('네이버로')&&t.includes('가입'))){\
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
                if(el.offsetParent!==null&&(t.includes('네이버로')&&t.includes('가입'))){el.click();return true;}}\
             return false;})()",
        )
        .unwrap_or(false))
}

// 가입 마지막 단계(external_account_sign_up)를 처리한다: (1) 생년월일을 고정 기본값으로 채우고
// (text/number 입력·연월일 select 양쪽 best-effort) (2) 전체동의 체크 (3) 완료/가입 버튼을 좌표
// 클릭한다. 정확한 셀렉터를 패킷에서 못 구했으므로 DOM 구조(입력·select·버튼)를 원문 진단으로 남겨
// 안 맞으면 실제 셀렉터가 로그에 드러나게 한다(형님 "로그에 원문" 원칙).
fn handle_signup_final(client: &mut CdpClient) -> Result<bool, AutomationError> {
    let fill_js = format!(
        "(()=>{{\
        const Y='{y}',M='{m}',D='{d}',YMD='{y}{m}{d}';\
        const setVal=(el,v)=>{{try{{\
            const proto=el.tagName==='SELECT'?window.HTMLSelectElement.prototype:window.HTMLInputElement.prototype;\
            const desc=Object.getOwnPropertyDescriptor(proto,'value');\
            if(desc&&desc.set)desc.set.call(el,v);else el.value=v;\
            el.dispatchEvent(new Event('input',{{bubbles:true}}));\
            el.dispatchEvent(new Event('change',{{bubbles:true}}));}}catch(e){{}}}};\
        const num=s=>parseInt(String(s).replace(/[^0-9]/g,''),10);\
        const selects=Array.prototype.slice.call(document.querySelectorAll('select'));\
        const pick=(sel,vals)=>{{for(const o of sel.options){{const ov=String(o.value),ot=String(o.textContent||'');\
            if(vals.some(v=>ov===v||num(ov)===num(v)||ot.replace(/[^0-9]/g,'')===String(num(v))))\
                {{setVal(sel,o.value);return true;}}}}return false;}};\
        if(selects.length>=3){{pick(selects[0],[Y,'1990']);pick(selects[1],[M,'1']);pick(selects[2],[D,'1']);}}\
        /* 실제 band 가입 폼(패킷 2026-07-13): 생년월일=<input type=date id=bday>. band JS의 \
           getBirthdate()는 #bday 부모에 '-active'(클릭 시 추가)가 있어야만 값을 읽어 hidden \
           birthdate(YYYYMMDD+)를 채운다. 그래서 값(ISO)+부모 -active+change+hidden 을 함께 세팅한다. */\
        const bday=document.querySelector('#bday')\
            ||document.querySelector('input[type=date]');\
        if(bday){{const iso=Y+'-'+M+'-'+D;\
            try{{const dsc=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value');\
                if(dsc&&dsc.set)dsc.set.call(bday,iso);else bday.value=iso;}}catch(e){{bday.value=iso;}}\
            try{{bday.valueAsDate=new Date(iso+'T00:00:00Z');}}catch(e){{}}\
            const bp=bday.parentElement;if(bp)bp.classList.add('-active');\
            bday.dispatchEvent(new Event('input',{{bubbles:true}}));\
            bday.dispatchEvent(new Event('change',{{bubbles:true}}));\
            const hb=document.querySelector('input[name=birthdate]');if(hb)setVal(hb,YMD+'+');}}\
        const inputs=Array.prototype.slice.call(document.querySelectorAll('input'))\
            .filter(i=>['text','number','tel',''].indexOf(String(i.type||'').toLowerCase())>=0\
                       &&i.offsetParent!==null&&i.type!=='checkbox'&&i.type!=='radio');\
        if(inputs.length===1)setVal(inputs[0],YMD);\
        else if(inputs.length>=3){{setVal(inputs[0],Y);setVal(inputs[1],M);setVal(inputs[2],D);}}\
        else for(const i of inputs){{const h=(i.placeholder||'')+(i.name||'')+(i.id||'');\
            if(/(birth|생년|년|month|month|day|일|월)/i.test(h))setVal(i,YMD);}}\
        const boxes=Array.prototype.slice.call(document.querySelectorAll('[role=\"checkbox\"]'))\
            .filter(cb=>cb.id!=='keep');\
        const chk=cb=>cb.getAttribute('aria-checked')==='true'||cb.checked===true;\
        const toggle=el=>{{if(!el)return;el.click();\
            if(!chk(el))el.dispatchEvent(new MouseEvent('click',{{bubbles:true}}));\
            if(!chk(el))el.dispatchEvent(new KeyboardEvent('keydown',{{key:' ',keyCode:32,bubbles:true}}));}};\
        let all=document.querySelector('#agree_select');\
        if(!all)all=boxes.find(cb=>{{const t=String((cb.parentElement||cb).textContent||'').replace(/\\s+/g,'');\
            return t.indexOf('전체동의')>=0||t.indexOf('모두동의')>=0;}});\
        if(all&&!chk(all))toggle(all);\
        for(const cb of boxes){{if(!chk(cb))toggle(cb);}}\
        const cbs=Array.prototype.slice.call(document.querySelectorAll('input[type=checkbox]'));\
        for(const cb of cbs){{if(!cb.checked)cb.click();\
            if(!cb.checked&&cb.closest('label'))cb.closest('label').click();}}\
        const inputDiag=inputs.map(i=>(i.type||'text')+':'+(i.id||i.name||i.placeholder||'?')+'='+String(i.value).slice(0,10));\
        const selDiag=selects.map(s=>(s.id||s.name||'?')+'='+s.value);\
        const cbDiag=cbs.map(cb=>(cb.id||cb.name||'?')+':'+cb.checked);\
        const btns=Array.prototype.slice.call(document.querySelectorAll('button,a,input[type=submit],input[type=button]'))\
            .map(b=>String(b.innerText||b.textContent||b.value||'').replace(/\\s+/g,' ').trim())\
            .filter(t=>t.length>0&&t.length<24);\
        const bdayEl=document.querySelector('#bday')||document.querySelector('input[type=date]');\
        const hbEl=document.querySelector('input[name=birthdate]');\
        const bdayDiag=bdayEl?(bdayEl.value+'|active:'+(bdayEl.parentElement&&bdayEl.parentElement.classList.contains('-active'))):'none';\
        const hbDiag=hbEl?hbEl.value:'none';\
        return JSON.stringify({{url:location.href,bday:bdayDiag,birthdate:hbDiag,inputs:inputDiag,selects:selDiag,cbs:cbDiag,buttons:btns.slice(0,15)}});}})()",
        y = DEFAULT_SIGNUP_BIRTH_YEAR, m = DEFAULT_SIGNUP_BIRTH_MONTH, d = DEFAULT_SIGNUP_BIRTH_DAY
    );
    let diag = client.evaluate_string(&fill_js).unwrap_or_default();
    tracing::info!("[BAND] 가입 마지막 단계 처리(생년월일 {DEFAULT_SIGNUP_BIRTH_YEAR}-{DEFAULT_SIGNUP_BIRTH_MONTH}-{DEFAULT_SIGNUP_BIRTH_DAY} + 전체동의, 원문 구조) — {diag}");

    // 완료/가입/확인/다음 버튼을 좌표 클릭(전체동의 라벨 제외, 정확매칭 우선).
    const SUBMIT_JS: &str = "(()=>{\
        const vis=el=>{if(!el)return false;const r=el.getBoundingClientRect();\
            return r.width>0&&r.height>0&&el.offsetParent!==null&&!el.disabled;};\
        const cs=Array.prototype.slice.call(\
            document.querySelectorAll('button, a, input[type=submit], input[type=button]'));\
        const norm=c=>String(c.innerText||c.textContent||c.value||'').replace(/\\s+/g,'');\
        let el=null;\
        for(const c of cs){const t=norm(c);\
            if(vis(c)&&!t.includes('전체')&&(t==='완료'||t==='가입'||t==='가입하기'||t==='확인'||t==='다음'||t==='동의하고가입'||t==='시작하기')){el=c;break;}}\
        if(!vis(el)){for(const c of cs){const t=norm(c);\
            if(vis(c)&&!t.includes('전체')&&(t.indexOf('완료')>=0||t.indexOf('가입')>=0||t.indexOf('확인')>=0||t.indexOf('다음')>=0||t.indexOf('시작')>=0)){el=c;break;}}}\
        if(!vis(el))return null;\
        const r=el.getBoundingClientRect();return [r.left+r.width/2, r.top+r.height/2];})()";
    if let Some((x, y)) = parse_xy(&client.evaluate(SUBMIT_JS)?) {
        mouse_click(client, x, y)?;
        return Ok(true);
    }
    Ok(false)
}

// OAuth 동의 페이지에서 (1) 전체동의(agree-all) 체크박스를 먼저 체크하고 (2) 동의/확인 버튼을 좌표
// 마우스 클릭한다(best-effort). 자동 리다이렉트라 눌 게 없으면 no-op(false).
fn click_oauth_consent(client: &mut CdpClient) -> Result<bool, AutomationError> {
    // (1) 동의 체크박스 전부 체크. **실측(패킷 2026-07-15 밴드 다양한 경우)**: 네이버 OAuth 동의
    //     체크박스는 `<input type=checkbox>`가 아니라 `<div role="checkbox" aria-checked>`(커스텀)다 —
    //     예전 `input[type=checkbox]` 조회가 0개를 봐 아무것도 못 켜고 "동의하기"가 막혔다(cbCount:0).
    //     전체동의(#agree_select) 하나만 클릭하면 네이버 JS(agreeAllEventCallbackFunc)가 필수([필수]
    //     개인정보 제3자 제공, meta-mandatory) 포함 전부 cascade 한다. 로그인 유지(id=keep)는 제외.
    //     진단으로 각 role=checkbox의 aria-checked 상태를 남겨 안 켜지면 로그가 원문을 드러낸다.
    const CHECK_ALL_JS: &str = "(()=>{\
        const boxes=Array.prototype.slice.call(document.querySelectorAll('[role=\"checkbox\"]'))\
            .filter(cb=>cb.id!=='keep');\
        const chk=cb=>cb.getAttribute('aria-checked')==='true'||cb.checked===true;\
        const toggle=el=>{if(!el)return;el.click();\
            if(!chk(el))el.dispatchEvent(new MouseEvent('click',{bubbles:true}));\
            if(!chk(el))el.dispatchEvent(new KeyboardEvent('keydown',{key:' ',keyCode:32,bubbles:true}));};\
        let all=document.querySelector('#agree_select');\
        if(!all)all=boxes.find(cb=>{const t=String((cb.parentElement||cb).textContent||'').replace(/\\s+/g,'');\
            return t.indexOf('전체동의')>=0||t.indexOf('모두동의')>=0;});\
        if(all&&!chk(all))toggle(all);\
        for(const cb of boxes){if(!chk(cb))toggle(cb);}\
        const cbs=Array.prototype.slice.call(document.querySelectorAll('input[type=checkbox]'));\
        for(const cb of cbs){if(!cb.checked)cb.click();\
            if(!cb.checked&&cb.closest('label'))cb.closest('label').click();}\
        const states=boxes.map(cb=>(cb.id||(cb.getAttribute('meta-mandatory')==='true'?'[필수]':'?'))\
            +':'+cb.getAttribute('aria-checked'));\
        const btns=Array.prototype.slice.call(\
            document.querySelectorAll('button,a,input[type=submit],input[type=button]'))\
            .map(b=>String(b.innerText||b.textContent||b.value||'').replace(/\\s+/g,' ').trim())\
            .filter(t=>t.length>0&&t.length<24);\
        return JSON.stringify({url:location.href,roleCbCount:boxes.length,agreeAll:!!all,\
            after:states,buttons:btns.slice(0,15)});})()";
    let diag = client.evaluate_string(CHECK_ALL_JS).unwrap_or_default();
    tracing::info!("[BAND] OAuth 동의 화면 처리(원문 구조) — {diag}");

    // (2) 동의/확인/허용/계속 버튼을 좌표 클릭(id 후보 우선, 없으면 텍스트로).
    const CENTER_JS: &str = "(()=>{\
        const vis=el=>{if(!el)return false;const r=el.getBoundingClientRect();\
            return r.width>0&&r.height>0&&el.offsetParent!==null&&!el.disabled;};\
        let el=document.querySelector('button.agree')||document.querySelector('#agree_btn')\
            ||document.querySelector('#agree')||document.querySelector('#btnAgree');\
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
             let el=document.querySelector('button.agree')||document.querySelector('#agree_btn')\
                 ||document.querySelector('#agree')||document.querySelector('#btnAgree');\
             if(!vis(el)){el=null;\
                 const cs=Array.prototype.slice.call(\
                     document.querySelectorAll('button, a, input[type=submit], input[type=button]'));\
                 const norm=c=>String(c.innerText||c.textContent||c.value||'').replace(/\\s+/g,'');\
                 for(const c of cs){const t=norm(c);\
                     if(vis(c)&&!t.includes('전체')&&(t==='동의하기'||t==='동의'||t==='허용하기'||t==='허용'||t==='확인'||t==='계속')){el=c;break;}}\
                 if(!vis(el)){for(const c of cs){const t=norm(c);\
                     if(vis(c)&&!t.includes('전체')&&(t.indexOf('동의')>=0||t.indexOf('허용')>=0||t.indexOf('계속')>=0||t.indexOf('확인')>=0)){el=c;break;}}}}\
             if(vis(el)){el.click();return true;}\
             /* 버튼을 못 찾으면 네이버 OAuth 동의 폼을 직접 제출한다(oauth_consent.js가 button.agree \
                클릭 시 하는 것과 동일: form[name=oauthagreeFrm].submit → POST allow_oauth). 실측 2026-07-16. */\
             const f=document.querySelector('form[name=oauthagreeFrm]');\
             if(f){f.submit();return true;}return false;})()",
        )
        .unwrap_or(false))
}

// evaluate가 돌려준 `[x, y]`(returnByValue) 배열을 좌표로 파싱한다(없으면 None).
fn parse_xy(value: &Value) -> Option<(f64, f64)> {
    let arr = value.as_array()?;
    Some((arr.first()?.as_f64()?, arr.get(1)?.as_f64()?))
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

// 종목토론방 네이버 로그인과 완전히 같은 입력 함수를 재사용한다. 첫 시도는 글자 사이 지연이
// 0이므로 DOM 게이트가 열린 직후 ID/PW가 즉시 채워진다. 네이버 쪽 함수가 진단 문자열을
// 돌려주면 입력 실패, None이면 성공이다.
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<bool, AutomationError> {
    Ok(type_into_naver(client, selector, text)?.is_none())
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

    // --- band 자체 이메일 로그인 화면(본인확인 → 비밀번호 입력) ---

    #[test]
    fn detects_email_confirm_page_by_url_and_text() {
        assert!(is_email_confirm_url(
            "https://auth.band.us/confirm_user_email_login"
        ));
        assert!(!is_email_confirm_url(
            "https://auth.band.us/email_login/password"
        ));
        // 패킷 원문 문구(글자 그대로): "본인이 맞으신가요?" + "당신의 계정이라면 로그인하세요."
        assert!(is_email_confirm_text(
            "본인이 맞으신가요? 당신의 계정이라면 로그인하세요. 로그인 하기"
        ));
        assert!(!is_email_confirm_text("비밀번호 입력"));
    }

    #[test]
    fn detects_email_password_page_by_url() {
        assert!(is_email_password_url(
            "https://auth.band.us/email_login/password"
        ));
        assert!(is_email_password_url(
            "https://auth.band.us/email_login/password?login_type="
        ));
        assert!(!is_email_password_url(
            "https://auth.band.us/confirm_user_email_login"
        ));
    }

    #[test]
    fn detects_direct_email_login_page_without_matching_password_step() {
        assert!(is_email_login_url(
            "https://auth.band.us/email_login?keep_login=false"
        ));
        assert!(is_email_login_url("https://auth.band.us/email_login"));
        assert!(!is_email_login_url(
            "https://auth.band.us/email_login/password?login_type="
        ));
    }

    #[test]
    fn classify_email_form_starts_with_email_typing() {
        let signals = BandPageSignals {
            email_form: true,
            ..Default::default()
        };
        assert_eq!(classify(&signals), BandSignal::EmailForm);
        assert_eq!(
            decide_loop_step(None, BandSignal::EmailForm, false),
            LoopAction::TypeBandEmail
        );
    }

    #[test]
    fn classify_maps_email_confirm_and_password() {
        assert_eq!(
            classify(&BandPageSignals {
                email_confirm: true,
                ..Default::default()
            }),
            BandSignal::EmailConfirm
        );
        assert_eq!(
            classify(&BandPageSignals {
                email_password: true,
                ..Default::default()
            }),
            BandSignal::EmailPassword
        );
    }

    #[test]
    fn classify_success_beats_email_screens() {
        let s = BandPageSignals {
            logged_in: true,
            email_confirm: true,
            email_password: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::Success);
    }

    #[test]
    fn classify_bad_credentials_when_password_error_gates_email_password() {
        // 비번 오류 화면: read_signals 가 email_password=false(오류로 게이트), bad_credentials=true 로
        // 만든다. classify 는 재입력(EmailPassword) 대신 BadCredentials 를 골라야 한다("창 닫고 다음 계정").
        let s = BandPageSignals {
            email_password: false,
            bad_credentials: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), BandSignal::BadCredentials);
    }

    #[test]
    fn email_confirm_click_then_password_type_actions() {
        assert_eq!(
            decide_loop_step(None, BandSignal::EmailConfirm, false),
            LoopAction::ClickEmailConfirm
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::EmailPassword, false),
            LoopAction::TypeBandPassword
        );
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
    fn detects_signup_final_step() {
        assert!(is_signup_final_url(
            "https://auth.band.us/external_account_sign_up"
        ));
        assert!(is_signup_final_url(
            "https://auth.band.us/continue_external_account_sign_up"
        ));
        assert!(!is_signup_final_url("https://auth.band.us/login?_ns=false"));
        assert!(is_signup_final_text(
            "BAND 가입 마지막 단계입니다. 생년월일 전체동의 (선택 항목 포함) 이용약관 동의 (필수)"
        ));
        assert!(!is_signup_final_text(
            "BAND 로그인 이메일로 로그인 휴대폰 번호로 로그인"
        ));
        // classify: 가입 마지막 단계는 고유 액션 신호로 잡힌다.
        assert_eq!(
            classify(&BandPageSignals {
                signup_final: true,
                ..Default::default()
            }),
            BandSignal::SignupFinal
        );
        assert_eq!(
            decide_loop_step(None, BandSignal::SignupFinal, false),
            LoopAction::HandleSignupFinal
        );
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
    fn login_entry_url_targets_band_home_instead_of_direct_email_login() {
        let url = normal_login_entry_url();
        assert_eq!(url, "https://www.band.us/");
        assert!(!url.contains("auth.band.us/email_login"));
    }

    #[test]
    fn normal_entry_uses_exact_intro_and_email_method_buttons() {
        assert_eq!(BAND_INTRO_LOGIN_SELECTOR, "a.login._loginLink");
        assert_eq!(
            BAND_EMAIL_METHOD_SELECTOR,
            "a.buttonRound.-email[data-login-method='email']"
        );
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
        assert!(!is_inactive_user_url(
            "https://auth.band.us/b/validation_welcome"
        ));
    }

    #[test]
    fn logged_in_only_on_real_band_home() {
        // 성공(패킷 diff): auth 도메인을 벗어난 band.us 홈만 로그인 완료로 본다.
        assert!(is_logged_in_band_url("https://www.band.us/band-create"));
        assert!(is_logged_in_band_url("https://band.us/band/103043410"));
        assert!(is_logged_in_band_url("https://www.band.us/feed"));
        // 인터스티셜(2단계 인증/캡차/가입중)과 네이버 로그인은 성공 아님 — band_session 있어도 반쪽.
        assert!(!is_logged_in_band_url(
            "https://auth.band.us/b/validation_welcome"
        ));
        assert!(!is_logged_in_band_url(
            "https://auth.band.us/b/validation/recaptcha?next_url=https%3A%2F%2Fband.us"
        ));
        assert!(!is_logged_in_band_url(
            "https://auth.band.us/continue_external_account_sign_up"
        ));
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

    // --- parse_xy / cookies ---

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

    // (로그인 폼 게이트 순수함수 테스트는 네이버 auth::login_flow로 이동 — 밴드는 그 구현을 재사용.)
}
