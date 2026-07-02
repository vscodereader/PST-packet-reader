//! CDP 로그인 시퀀스. 시스템 Chrome에 attach한 `CdpClient`로 네이버 로그인 폼을
//! 실제 키 이벤트(`Input.dispatchKeyEvent`)로 채우고, 결과를 분류한다.
//!
//! ⚠️ `Runtime.evaluate`로 `input.value=`를 직접 설정하지 않는다. 네이버 로그인은
//! keydown을 후킹해 암호화 페이로드를 만들기 때문에 값만 꽂으면 암호화가 깨진다.

use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::naver_automation::{AutomationError, CdpClient};

const LOGIN_URL: &str = "https://nid.naver.com/nidlogin.login?mode=form&url=https://www.naver.com/";
// headless: 캡차가 보이면 곧장 headed로 승격해야 하므로 짧게 기다린다(#14: 40→20초).
const HEADLESS_TIMEOUT: Duration = Duration::from_secs(20);
// headed: 캡차 외의 추가 인증/오류는 즉시 실패시키므로(#267-13에서
// 캡챠 외 전부 칼같이 실패), 여기서는 pending(네비게이션 정리) 여유만 짧게 둔다. 캡차는 아래
// CAPTCHA_GRACE로 따로 기다린다(기존 180초 사람 대기 제거 → 체감 속도 #14).
const HEADED_PENDING_TIMEOUT: Duration = Duration::from_secs(12);
// 보류(OnHold) 계정 재로그인에서 캡차가 떴을 때, 사용자가 직접 보안문자를 입력해 풀 수 있도록
// 창을 열어두는 상한(사용자 지시 1번: 성공할 때까지 열어두되 무한 대기는 막는 상한 120초).
// 이 안에 로그인(쿠키)되면 성공으로 확정해 창을 닫고 활성화한다. 첫 로그인(일반 계정)은 이
// 대기를 적용하지 않고 즉시 보류로 떨어뜨린다(decide_loop_step의 FailCaptchaToHold).
const MANUAL_CAPTCHA_TIMEOUT: Duration = Duration::from_secs(120);
// 결과 폴링 간격. 고정 대기가 아니라 결과 DOM이 자리잡는 즉시 다음으로 넘어가기 위해 촘촘히
// 본다(사수 지시: 고정 400ms 금지 → 100ms로 DOM 반응성 확보). 음성 신호 2회 latch도 이만큼
// 빨라져 비번오류/차단 확정이 ~0.2초로 떨어진다.
const POLL_INTERVAL: Duration = Duration::from_millis(100);
// 아이디/비밀번호 입력 사이·클릭 직전의 사람 같은 멈춤(행동 기반 봇탐지 완화). 사수 지시로
// 0.8초→100ms로 줄이되 0으로는 만들지 않는다(타이밍 지문 유지 — 흐름은 그대로, 시간만 단축).
const FIELD_PAUSE: Duration = Duration::from_millis(100);
// 로그인 버튼 클릭 직후 네비게이션이 정리될 settle. 폴링 간격(100ms)보다 길게 둬, 클릭 직후
// 깜빡이는 #err_common/과도기 폼 소멸을 실패로 latch하지 않게 한다.
const CLICK_SETTLE: Duration = Duration::from_millis(800);
// pending(성공·캡차·명시 오류가 아닌 중간 상태)이 이만큼 지속되면 취소한다(#267-13 후속).
// 인식하지 못한 추가 인증 화면(예: 2단계 인증)이 로그인 폼을 유지해 blocked로도 안 잡힐 때,
// 전체 타임아웃(12초)까지 기다리지 않고 빠르게 실패시킨다. 정상 로그인은 보통 클릭 후 수 초
// 내 성공이라 6초면 안전하다.
const PENDING_STALL: Duration = Duration::from_secs(6);

// 봇탐지(ncaptcha/wtm) 완화용 스텔스 스크립트. 페이지 스크립트보다 먼저 모든 새 문서에서
// 실행되어 자동화 흔적을 일반 크롬과 동일하게 맞춘다.
//
// 네이버 안티봇 번들(wtm.pstatic.net)의 검사는
//   getWebdriver(){ return void 0!==navigator.webdriver ? Boolean(navigator.webdriver).toString() : "" }
// 형태다. 일반(비자동화) 크롬은 `navigator.webdriver === false` 라 "false"를 보고한다.
//
// ⚠️ 핵심(지문 비교로 실측, 2026-06-29): 예전엔 `Object.defineProperty(navigator,'webdriver',…)`로
// **인스턴스에 직접** 박았는데, 그러면 `navigator.hasOwnProperty('webdriver')===true`가 되어
// **일반 크롬(프로토타입에만 존재 → own=false)과 달라지는 탐지 흔적**을 스스로 남겼다(매크로만
// 캡차가 뜨던 직접 원인 후보). 그래서 일반 크롬과 **위치까지 동일**하도록 `Navigator.prototype`에
// 정의한다(인스턴스에는 own 속성을 만들지 않는다). 값은 그대로 `false`.
//
// languages 도 빈 incognito 프로필에선 `["ko-KR"]` 1개뿐이라 일반 크롬(`ko-KR,ko,en-US,en`)과
// 달라 탐지 표면이 된다(같은 실측). 프로토타입에 4개 배열로 맞춰 인스턴스 own 흔적 없이 정렬한다.
const STEALTH_INIT_JS: &str = "(()=>{try{\
    Object.defineProperty(Navigator.prototype,'webdriver',\
        {get:()=>false,configurable:true,enumerable:true});}catch(e){}\
    try{Object.defineProperty(Navigator.prototype,'languages',\
        {get:()=>['ko-KR','ko','en-US','en'],configurable:true,enumerable:true});}catch(e){}})();";

/// 챌린지(추가 인증) 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChallengeKind {
    Captcha,
    Otp,
    Device,
}

/// 로그인 결과.
pub(crate) enum LoginOutcome {
    Ok {
        cookies: Vec<Value>,
    },
    ChallengeRequired {
        kind: ChallengeKind,
    },
    BadCredentials,
    /// 로그인 접근이 차단된 상태(폼이 사라지고 세션도 없음). 일반 오류와 구분해 계정
    /// 상태를 `blocked`로 표시하기 위해 별도 variant로 둔다.
    Blocked,
    /// 계정 보호조치(`idSafetyRelease`)로 로그인이 막힌 확정 상태. 휴리스틱 `Blocked`와 달리
    /// 착지 URL로 명확히 판별되므로, 사람이 풀 수 없는 종료 상태로 보고 headed여도 즉시 실패한다
    /// (180초 대기 회피, #228). 계정 상태는 `Blocked`로 매핑하되 메시지만 보호조치용으로 둔다.
    Protected,
    /// 계정 잠금조치로 로그인이 막힌 확정 상태(#243). 보호조치(`idSafetyRelease` URL 리다이렉트)와
    /// 달리, 잠금은 같은 `nidlogin.login`에서 200 + 잠금 안내 HTML 본문으로 응답되고 성공 쿠키가
    /// 없다(패킷 login-lock2). URL이 아니라 본문 텍스트로 식별하며, 사람이 즉석에서 풀 수 없는
    /// 종료 상태라 `Protected`처럼 headed여도 즉시 실패한다. 계정 상태는 `Blocked`로 매핑하고
    /// 메시지만 잠금용으로 둔다.
    Locked,
    /// 캡차(보안문자)가 떴지만 자동 통과 시간(`CAPTCHA_GRACE`, 10초) 안에 풀리지 않은 상태(#267
    /// 후속). 일반 `Error`와 달리 사람이 직접 풀면 회복 가능한 상태라, 계정을 "보류"(`OnHold`)로
    /// 표시해 사용자가 보류 계정만 골라 다시 풀 수 있게 한다.
    CaptchaUnsolved,
    /// 로그인 후 "본인확인(휴대전화 번호)" 화면이 떠 로그인을 보류한 상태(전화번호 패킷분석).
    /// 캡차와 동일하게 계정을 "보류"(`OnHold`)로 둔다. ID가 010+숫자8자리면 그 번호를 입력·확인까지
    /// 시도하되 바로 성공하지 않으면, 형식이 아니면 즉시, 이 상태로 떨어진다.
    PhoneVerify,
    Error(String),
}

/// 페이지 판정 신호(분류 입력). 분류 로직을 브라우저에서 분리해 단위 테스트한다.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PageSignals {
    pub logged_in: bool,
    pub captcha: bool,
    pub otp: bool,
    pub device: bool,
    pub bad_credentials: bool,
    pub blocked: bool,
    /// 계정 보호조치 페이지(`idSafetyRelease`) 착지. 휴리스틱 `blocked`와 달리 명확한 종료 신호.
    pub protected: bool,
    /// 계정 잠금 안내 본문("아이디 잠금") 감지(#243). `blocked`와 겹치지만 명확한 종료 신호.
    pub locked: bool,
    /// 로그인 후 "본인확인(휴대전화 번호)" 화면(`#phone_value` tel 입력칸) 감지. 캡차처럼 보류로 처리.
    pub phone_verify: bool,
}

/// 진행 중/확정 신호.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signal {
    Pending,
    Success,
    Challenge(ChallengeKind),
    BadCredentials,
    Blocked,
    /// 보호조치 확정(착지 URL 기반). headed여도 즉시 실패시키기 위해 `Blocked`와 분리한다.
    Protected,
    /// 계정 잠금 확정(본문 텍스트 기반, #243). `Protected`와 같이 headed여도 즉시 실패시킨다.
    Locked,
    /// 본인확인(휴대전화 번호) 화면 감지(전화번호 패킷분석). ID가 010+8자리면 번호 입력·확인까지
    /// 시도하고, 아니면 즉시 보류(OnHold)로 떨어뜨린다.
    PhoneVerify,
}

/// 폴링 한 스텝의 판정 결과. 루프는 이 값을 실제 동작(반환/대기)으로 옮긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopDecision {
    Success,
    /// headless에서 캡차를 만남 — headed로 승격해 사람/스텔스가 풀 기회를 준다(캡차 한정).
    PromoteChallenge(ChallengeKind),
    /// 보류(OnHold) 계정 재로그인에서 캡차를 만남 — 사용자가 직접 풀도록 창을 성공까지
    /// 열어둔다(상한 `MANUAL_CAPTCHA_TIMEOUT`, 120초). 성공하면 활성화·창 닫힘.
    WaitCaptcha,
    /// 첫 로그인(일반 계정)에서 캡차를 만남 — 사용자 지시대로 grace 없이 즉시 실패시키고
    /// 계정을 보류(OnHold)로 둔다. 이후 사용자가 보류 계정만 골라 재로그인해 직접 푼다.
    FailCaptchaToHold,
    /// 캡차가 아닌 추가 인증(본인인증 OTP·새 기기 인증)을 만남 — 캡차 외라 즉시
    /// 실패시킨다(#267-13: 캡챠 외 전부 칼같이 실패). 사람 대기(180초)를 적용하지 않는다.
    FailUnsupportedChallenge(ChallengeKind),
    ConfirmedBad,
    ConfirmedBlocked,
    /// 보호조치 확정 — 즉시 실패(`LoginOutcome::Protected`)로 옮긴다.
    ConfirmedProtected,
    /// 잠금 확정 — 즉시 실패(`LoginOutcome::Locked`)로 옮긴다(#243).
    ConfirmedLocked,
    /// 본인확인(휴대전화) 화면 — 루프가 ID 형식을 보고 번호 입력·확인을 시도하거나 즉시 보류로
    /// 떨어뜨린다(클라이언트 동작이 필요해 순수 decide_loop_step이 아닌 루프 arm에서 처리).
    HandlePhoneVerify,
    /// 아직 결과 미확정(중간 상태) — 계속 폴링한다.
    KeepWaiting,
}

/// 현재 페이지 신호로 이번 폴링의 동작을 결정한다(순수 함수).
///
/// - BadCredentials/Blocked: 즉시 확정(사용자 지시). 결과 DOM 게이트가 클릭 직후 과도기
///   깜빡임을 이미 걸러주므로 2회 latch 없이 즉시 실패시켜도 안전하다.
/// - 캡차: headless면 headed로 승격, headed+보류재로그인(`manual_captcha`)이면 직접 입력
///   대기(WaitCaptcha), headed+첫 로그인이면 즉시 보류(FailCaptchaToHold).
/// - 캡차 외 추가 인증(본인인증/기기인증)은 즉시 실패(#267-13).
fn decide_loop_step(
    signal: Signal,
    wait_for_human: bool,
    manual_captcha: bool,
) -> LoopDecision {
    match signal {
        Signal::Success => LoopDecision::Success,
        // 보호조치는 착지 URL로 명확히 판별되므로 headed의 사람 대기 없이 즉시 확정한다(#228).
        Signal::Protected => LoopDecision::ConfirmedProtected,
        // 잠금도 본문 텍스트로 명확히 판별되므로 보호조치와 동일하게 즉시 확정한다(#243).
        Signal::Locked => LoopDecision::ConfirmedLocked,
        // 캡차 처리(사용자 지시):
        // - headless: headed로 승격해 풀 기회를 준다.
        // - headed + 보류(OnHold) 재로그인(manual_captcha): 사용자가 직접 풀도록 창을 성공까지
        //   열어둔다(WaitCaptcha, 상한 120초).
        // - headed + 첫 로그인(일반): grace 없이 즉시 실패 → 계정을 보류로(FailCaptchaToHold).
        Signal::Challenge(ChallengeKind::Captcha) => {
            if !wait_for_human {
                LoopDecision::PromoteChallenge(ChallengeKind::Captcha)
            } else if manual_captcha {
                LoopDecision::WaitCaptcha
            } else {
                LoopDecision::FailCaptchaToHold
            }
        }
        // 본인인증(OTP)·새 기기 인증(Device)은 캡차 외라 즉시 실패시킨다(#267-13).
        Signal::Challenge(kind) => LoopDecision::FailUnsupportedChallenge(kind),
        // 본인확인(휴대전화 번호) 화면 — 루프 arm이 ID 형식 보고 번호 입력·확인 시도 또는 즉시 보류.
        Signal::PhoneVerify => LoopDecision::HandlePhoneVerify,
        // 비번오류/차단은 즉시 확정한다(사용자 지시: 즉시 실패). 클릭 직후 과도기 깜빡임은
        // 결과 DOM 게이트(리소스 정착 + 모든 iframe complete 3회 안정)가 이미 걸러, 여기 도달
        // 시점엔 DOM이 정착돼 있으므로 2회 latch가 더는 필요 없다.
        Signal::BadCredentials => LoopDecision::ConfirmedBad,
        Signal::Blocked => LoopDecision::ConfirmedBlocked,
        Signal::Pending => LoopDecision::KeepWaiting,
    }
}

/// 페이지 신호를 로그인 진행/결과 신호로 분류한다(순수 함수).
pub(crate) fn classify(signals: &PageSignals) -> Signal {
    if signals.logged_in {
        Signal::Success
    } else if signals.protected {
        // 보호조치는 휴리스틱 blocked보다 먼저 본다 — 보호조치 페이지도 로그인 폼이 없어
        // blocked 조건을 동시에 만족하지만, 명확한 종료 신호인 Protected로 분류해야 한다.
        Signal::Protected
    } else if signals.captcha {
        Signal::Challenge(ChallengeKind::Captcha)
    } else if signals.otp {
        Signal::Challenge(ChallengeKind::Otp)
    } else if signals.device {
        Signal::Challenge(ChallengeKind::Device)
    } else if signals.phone_verify {
        // 본인확인(휴대전화) 화면. 캡차/OTP/기기 다음, 비번오류/잠금/차단보다 앞에서 잡는다 —
        // 이 화면은 로그인 폼(#id)이 없어 blocked 휴리스틱과 겹칠 수 있으므로 먼저 분류한다.
        Signal::PhoneVerify
    } else if signals.bad_credentials {
        Signal::BadCredentials
    } else if signals.locked {
        // 잠금은 캡차/OTP/기기인증/비번오류 같은 **회복 가능한** 명시적 신호보다 뒤에, blocked
        // 휴리스틱보다는 앞에 둔다(#243). 보호조치(URL `idSafetyRelease`)와 달리 잠금은 본문
        // 텍스트로 식별해 정밀도가 낮으므로, 회복 가능한 페이지에 잠금 경고 문구가 섞여 있어도
        // 그 신호를 먼저 살려 사용자가 풀 기회를 잃지 않게 한다. 잠금 안내 HTML은 로그인 폼이
        // 없어 blocked 조건을 동시에 만족하므로, 그보다는 앞에서 명확한 종료 신호로 분류한다.
        Signal::Locked
    } else if signals.blocked {
        Signal::Blocked
    } else {
        Signal::Pending
    }
}

/// 로그인을 수행하고 결과를 분류해 반환한다.
///
/// `wait_for_human`이 true이면(headed) 캡차/2차 인증이 떠도 즉시 포기하지 않고,
/// 사용자가 열린 Chrome 창에서 직접 푸는 동안 성공(쿠키) 또는 타임아웃까지 기다린다.
/// false이면(headless) 챌린지를 만나는 즉시 `ChallengeRequired`로 반환해 호출자가
/// headed로 승격하도록 한다.
pub(crate) fn run(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
    manual_captcha: bool,
) -> (LoginOutcome, Option<String>) {
    // graceful 실패(Ok(LoginOutcome::Error))도 "자세히 보기"용 trace를 갖게 한다. 예전에는
    // Ok 가지 전부를 trace=None으로 흘려, 폼 자동입력 실패("아이디 칸이 비어") 같은 비-예외
    // 실패는 알림에 백트레이스가 안 붙어 추적이 끊겼다. 이제 타이핑 실패는 run_inner가
    // diag(포커스/안티봇/입력 글자수 진단 + 백트레이스)를 채우고, 그 외 Error는 여기서 최소
    // 백트레이스라도 붙여 — 모든 로그인 실패가 알림 "자세히 보기"에서 추적 가능해진다.
    let mut diag: Option<String> = None;
    match run_inner(client, id, pw, wait_for_human, manual_captcha, &mut diag) {
        Ok(outcome) => {
            let trace = match &outcome {
                LoginOutcome::Error(_) => {
                    Some(diag.unwrap_or_else(crate::util::backtrace_string))
                }
                _ => None,
            };
            (outcome, trace)
        }
        // CDP/자동화 실패 — 메시지는 사용자용, trace(위치 앵커+백트레이스)는 "자세히 보기"용
        // 으로 분리해 함께 돌려준다(#210). 메시지에는 백트레이스를 섞지 않는다.
        Err(error) => (
            LoginOutcome::Error(error.message().to_owned()),
            Some(error.trace()),
        ),
    }
}

/// 자격증명이 로그인 시도에 충분한지(둘 다 비어있지 않은지) 검사한다(순수 함수).
/// `type_into`는 빈 문자열에 대해 "0글자를 성공적으로 입력"으로 `Ok(true)`를 돌려주므로,
/// 빈 자격증명이 그대로 로그인 버튼 클릭까지 진행되는 것을 막으려면 시도 전에 걸러야 한다.
pub(crate) fn credentials_present(id: &str, pw: &str) -> bool {
    !id.trim().is_empty() && !pw.is_empty()
}

fn run_inner(
    client: &mut CdpClient,
    id: &str,
    pw: &str,
    wait_for_human: bool,
    manual_captcha: bool,
    // 폼 자동입력이 실패하면(타이핑이 필드에 안 들어감) 여기에 원인 진단 + 백트레이스를 채운다.
    // 호출부(run)가 이 값을 그대로 "자세히 보기" trace로 띄운다. 성공/타이핑 외 실패는 비워 둔다.
    diag: &mut Option<String>,
) -> Result<LoginOutcome, AutomationError> {
    // 빈/공백 자격증명이면 브라우저 폼을 건드리지 않고 즉시 입력 실패로 중단한다(기존
    // 사이드카도 빈 자격증명이면 브라우저를 띄우지 않았다). BadCredentials로 두면 "비번
    // 틀림"으로 오분류되므로, 사용자 입력 누락을 알리는 명확한 Error로 반환한다.
    if !credentials_present(id, pw) {
        return Ok(LoginOutcome::Error(
            "아이디 또는 비밀번호가 비어 있어 로그인을 시도하지 않았습니다.".to_owned(),
        ));
    }

    // navigate 전에 스텔스 스크립트를 등록해, 로그인 폼이 로드되며 실행되는 ncaptcha JS가
    // navigator.webdriver 를 읽기 전에 가려지도록 한다. CDP 호출이 실패해도 로그인 자체는
    // 진행해야 하므로 best-effort(let _)로 둔다(Page 도메인 미활성 등 환경 차이 흡수).
    let _ = client.call(
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": STEALTH_INIT_JS }),
    );

    client.navigate(LOGIN_URL)?;
    // navigate가 readyState까지 기다려도, 로그인 폼이 렌더되고 네이버의 keydown 암호화
    // 핸들러가 붙기 전에 타이핑하면 글자가 필드에 들어가지 않는다. 폼이 준비될 때까지 기다린다.
    if !wait_for_login_form(client) {
        return Ok(LoginOutcome::Error(
            "로그인 폼(#id/#pw)을 찾지 못했습니다.".to_owned(),
        ));
    }

    // [캡차 완화] CDP 자동화는 렌더러 DOM의 activeElement만 바꿀 뿐 브라우저(창) 포커스는
    // omnibox(주소창)에 남겨, document.hasFocus()=false 가 로그인 내내 유지된다. 그러면
    // 네이버 wtm 안티봇이 "한 번도 포커스되지 않은 페이지"를 봇 신호로 읽어 캡차를 더 띄운다
    // (사용자 관측: 빈 화면을 한 번 클릭하면 주소창 선택이 풀리고 캡차 빈도가 급감). 타이핑 전에
    // 창 포커스를 웹 컨텐츠로 옮겨(=실클릭과 같은 효과) 그 봇 신호를 없앤다. Page 도메인은 이미
    // enable_page_only 로 켜져 있고 Input 도 이미 쓰므로 탐지표면 증가는 사실상 없다.
    focus_web_contents(client);

    // [진단·임시] 수동입력 모드(PSTMACRO_LOGIN_MANUAL): 자동 타이핑을 생략하고 사용자가 이 CDP
    // 크롬 창에서 직접 입력하게 둔다. 같은 환경(CDP·플래그)에서 손입력=성공이면 원인은 합성 입력
    // (행동데이터), 손입력에도 캡차면 원인은 환경(CDP/플래그)임을 가른다(폼은 위에서 이미 준비됨).
    if std::env::var("PSTMACRO_LOGIN_MANUAL").is_ok() {
        return manual_login_wait(client);
    }

    // 위 wait_for_login_form이 "폼 완전 로딩"을 확인(로그)한 뒤에만 여기 도달한다. 곧장
    // 아이디 입력 → 2초 대기 → 비밀번호 입력 → 2초 대기 → 로그인 클릭.
    // 3회 재시도 후에도 필드가 비어 있으면(일시적 렌더/타이밍 문제) type_into가 false를
    // 돌려준다. 빈/부분 자격증명으로 로그인 버튼을 누르면 결과가 #err_common/타임아웃으로
    // 분류돼 일시적 타이핑 실패가 영구 BadCredentials/Error로 둔갑하므로, 클릭하지 않고
    // 명확한 입력 실패로 중단한다.
    if let Some(d) = type_into(client, "#id", id)? {
        *diag = Some(format!("{d}\n\n{}", crate::util::backtrace_string()));
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(아이디 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(FIELD_PAUSE);
    throttle_pause();
    if let Some(d) = type_into(client, "#pw", pw)? {
        *diag = Some(format!("{d}\n\n{}", crate::util::backtrace_string()));
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }

    // '로그인 상태 유지'를 켠다 — 이걸 켜야 브라우저가 npay 약관동의(commonTermAgree)에 싣는 nid
    // 세션 쿠키(NID_JST·NID_SAUTO)가 발급된다(실측 성공 패킷은 이 쿠키를 실어 통과). best-effort.
    enable_keep_signed_in(client);

    // 비밀번호 입력 후 사람처럼 잠깐 멈췄다가 로그인 버튼을 누른다(2초→0.8초, #14).
    sleep(FIELD_PAUSE);
    throttle_pause();
    // 로그인 버튼을 사람처럼 좌표 마우스 클릭(JS .click() 대신 진짜 mouse 이벤트). 좌표를 못
    // 구하면 .click()으로 폴백한다.
    click_login_button(client)?;

    let timeout = if wait_for_human {
        HEADED_PENDING_TIMEOUT
    } else {
        HEADLESS_TIMEOUT
    };
    // 클릭 직후 네비게이션이 정리될 시간을 준다. 이 settle 없이 곧장 읽으면, 클릭 직후
    // 잠깐 렌더된 #err_common이나 네비게이션 중간에 폼이 사라진 과도기 상태를 — 실제로는
    // 성공 중인 로그인인데도 — 실패로 latch한다.
    sleep(CLICK_SETTLE);

    let deadline = Instant::now() + timeout;
    // 보류 재로그인 캡차 직접 입력 상한(MANUAL_CAPTCHA_TIMEOUT). 캡차가 떴을 때만 설정되고,
    // 이 시각을 넘으면 보류로 취소한다. 설정되면 전체 deadline 적용을 멈춰 사용자 입력 시간을 준다.
    let mut captcha_deadline: Option<Instant> = None;
    // pending(중간 상태)이 처음 시작된 시점 + PENDING_STALL. 인식 못 한 추가 인증 화면(2단계 등)이
    // 성공 쿠키도 안 뜨고 명시 오류/캡차도 아닌 채 머물면, 이 시각을 넘는 즉시 실패시킨다(#267-13).
    let mut pending_deadline: Option<Instant> = None;
    // 결과 페이지 DOM이 전부 complete로 안정된 연속 폴링 횟수(사수 지시: 결과 폴링도 돔 싹 다
    // 붙을 때까지). 임계치 전까지는 비번오류/차단/캡차 판정을 미룬다. 한 번이라도 흔들리면 0으로.
    let mut result_dom_streak = 0u32;
    // 결과 게이트의 리소스 정착(②) 추적: 직전 폴의 완료 리소스 수 + 상위문서+iframe이 complete인
    // 폴이 몇 번 지속됐는지(정착이 안 되는 페이지용 상한 폴백에 쓴다).
    let mut prev_result_res_count: Option<i64> = None;
    let mut result_docs_complete_polls = 0u32;
    // 본인확인(휴대전화) 화면에서 번호 입력·확인을 이미 1회 시도했는지 + 그 시도 후 성공을 기다릴
    // 상한. ID가 010+8자리일 때만 설정되고, 이 안에 로그인 안 되면 보류(OnHold)로 떨어뜨린다.
    const PHONE_VERIFY_GRACE: Duration = Duration::from_secs(6);
    let mut phone_attempted = false;
    let mut phone_deadline: Option<Instant> = None;
    loop {
        // 인증 성공 직후 뜨는 "새 기기 등록" 페이지면 "등록 안함"을 눌러 마무리한다
        // (설계 5단계: browser_flow의 기존 로직 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        let signals = read_signals(client)?;

        // 성공(세션 쿠키)은 DOM 게이트와 무관하게 즉시 확정한다 — 성공은 쿠키로 판정하므로
        // 착지 페이지(naver.com)의 광고 iframe 로딩을 기다리느라 정상 로그인을 늦추거나 놓치지
        // 않는다. 보류 캡차 직접 입력 중에 사용자가 풀어 로그인돼도 여기서 즉시 성공 확정된다.
        if signals.logged_in {
            dump_naver_page(client, "로그인 성공(Ok)");
            let cookies = collect_naver_cookies(client)?;
            return Ok(LoginOutcome::Ok { cookies });
        }

        // 보류 캡차 직접 입력 대기 중이면 그 상한(120초)을 먼저 확인한다 — DOM이 잠깐 흔들려
        // 아래 게이트에 막혀 있어도 상한은 지켜, 넘으면 보류(OnHold)로 취소한다.
        if let Some(cd) = captcha_deadline {
            if Instant::now() >= cd {
                return Ok(LoginOutcome::CaptchaUnsolved);
            }
        }
        // 본인확인(휴대전화) 번호 입력·확인 후 성공 대기 상한. DOM이 잠깐 흔들려 아래 게이트에
        // 막혀 있어도 상한은 지켜, 넘으면 보류(OnHold)로 취소한다(전화번호 패킷분석 처리).
        if let Some(pd) = phone_deadline {
            if Instant::now() >= pd {
                return Ok(LoginOutcome::PhoneVerify);
            }
        }

        // 사수 지시: 결과 폴링도 DOM이 전부 붙을 때까지 기다린 뒤 판정한다. 상위 문서 + 모든
        // iframe이 complete로 연속 RESULT_DOM_STABLE_POLLS회 안정될 때까지는 결과를 판정하지
        // 않고 폴링만 계속한다(과도기 DOM 오판 방지). 단, 캡차/본인확인 입력 대기 중이면 전체
        // deadline을 적용하지 않는다 — 각자의 상한(captcha_deadline/phone_deadline)이 따로 끊는다.
        // 사수 의도("결과도 돔 싹 다 붙을 때까지")를 폼 게이트와 대칭으로 강화: 상위문서+모든
        // iframe complete(①)에 더해, 페이지가 받는 모든 리소스 로딩이 정착(②)할 때까지 기다린다.
        // 단 캡차/광고처럼 리소스가 끝없이 로딩돼 정착이 안 되는 페이지에서 판정이 deadline까지
        // 막혀 캡차가 Error로 둔갑하지 않게, docs-complete 후 RESULT_SETTLE_MAX_POLLS(~3초)가 지나면
        // 정착을 못 봐도 진행한다(강화하되 regression은 막는 안전 폴백).
        let all_docs = client.evaluate_bool(ALL_DOCS_COMPLETE_JS).unwrap_or(false);
        let dom_ready = if all_docs {
            result_docs_complete_polls += 1;
            let res_count = client
                .evaluate(RESOURCE_COUNT_JS)
                .ok()
                .and_then(|v| v.as_i64())
                .unwrap_or(-1);
            let settled = res_count >= 0 && prev_result_res_count == Some(res_count);
            prev_result_res_count = Some(res_count);
            settled || result_docs_complete_polls >= RESULT_SETTLE_MAX_POLLS
        } else {
            // 네비게이션으로 문서가 다시 미완성이 되면 정착 측정을 처음부터 다시 한다.
            prev_result_res_count = None;
            result_docs_complete_polls = 0;
            false
        };
        result_dom_streak = next_ready_streak(result_dom_streak, dom_ready);
        if !result_dom_gate_open(result_dom_streak) {
            if captcha_deadline.is_none()
                && phone_deadline.is_none()
                && Instant::now() >= deadline
            {
                let url = client.current_url().unwrap_or_default();
                return Ok(LoginOutcome::Error(format!(
                    "로그인 결과 페이지의 DOM이 끝까지 로딩되지 않아 취소했습니다. 마지막 페이지: {url}"
                )));
            }
            sleep(POLL_INTERVAL);
            continue;
        }

        match decide_loop_step(classify(&signals), wait_for_human, manual_captcha) {
            LoopDecision::Success => {
                dump_naver_page(client, "로그인 성공(Ok)");
                let cookies = collect_naver_cookies(client)?;
                return Ok(LoginOutcome::Ok { cookies });
            }
            LoopDecision::PromoteChallenge(kind) => {
                return Ok(LoginOutcome::ChallengeRequired { kind });
            }
            // 보류(OnHold) 계정 재로그인 캡차: 사용자가 직접 풀도록 창을 성공까지 열어둔다
            // (상한 120초). 그 안에 로그인되면 위 logged_in 단락에서 성공 확정되고, 상한을 넘으면
            // 보류로 유지한다. 캡차 대기 중엔 정체 타이머를 끈다.
            LoopDecision::WaitCaptcha => {
                captcha_deadline
                    .get_or_insert_with(|| Instant::now() + MANUAL_CAPTCHA_TIMEOUT);
                pending_deadline = None;
            }
            // 첫 로그인(일반 계정) 캡차: grace 없이 즉시 실패시키고 계정을 보류(OnHold)로 둔다.
            LoopDecision::FailCaptchaToHold => {
                dump_naver_page(client, "캡차/보안문자(CaptchaUnsolved→보류)");
                return Ok(LoginOutcome::CaptchaUnsolved);
            }
            // 본인확인(휴대전화 번호) 화면(전화번호 패킷분석). ID가 010+8자리면 그 번호를
            // #phone_value에 입력하고 #oab.submit(확인)을 눌러 1회 시도한다(앞 +82 select는 안 건드림).
            // 그 뒤 PHONE_VERIFY_GRACE 안에 로그인되면 위 logged_in에서 성공 확정, 안 되면 보류
            // (OnHold, 위 phone_deadline). ID가 그 형식이 아니면 즉시 보류로 떨어뜨린다.
            LoopDecision::HandlePhoneVerify => {
                if !id_is_phone_format(id) {
                    return Ok(LoginOutcome::PhoneVerify);
                }
                if !phone_attempted {
                    let _ = type_into(client, "#phone_value", id)?;
                    // 확인 버튼 클릭. id에 점이 있어 CSS 이스케이프(\\.)가 필요하다. 못 찾으면 폼의
                    // submit으로 폴백한다.
                    let _ = client.evaluate(
                        "(()=>{const b=document.querySelector('#oab\\\\.submit')\
                         ||document.querySelector('#frmNIDLogin input[type=submit]');\
                         if(b){b.click();return true;}return false;})()",
                    );
                    phone_attempted = true;
                    phone_deadline = Some(Instant::now() + PHONE_VERIFY_GRACE);
                    pending_deadline = None;
                }
            }
            // 캡차가 아닌 추가 인증(본인인증 OTP/새 기기 인증) — 캡차 외라 즉시 실패(#267-13).
            LoopDecision::FailUnsupportedChallenge(kind) => {
                dump_naver_page(client, "미지원 추가인증(OTP/새기기)");
                return Ok(LoginOutcome::Error(format!(
                    "{} 화면이 떠 자동 로그인을 중단했습니다(캡차 외 즉시 실패).",
                    challenge_kind_label(kind)
                )));
            }
            LoopDecision::ConfirmedBad => {
                dump_naver_page(client, "비번오류(BadCredentials)");
                return Ok(LoginOutcome::BadCredentials);
            }
            LoopDecision::ConfirmedBlocked => {
                dump_naver_page(client, "차단(Blocked)");
                return Ok(LoginOutcome::Blocked);
            }
            LoopDecision::ConfirmedProtected => {
                dump_naver_page(client, "보호조치(Protected)");
                return Ok(LoginOutcome::Protected);
            }
            LoopDecision::ConfirmedLocked => {
                dump_naver_page(client, "잠금(Locked)");
                return Ok(LoginOutcome::Locked);
            }
            // 중간 상태(성공·캡차·명시오류 아님) — 인식 못 한 추가 인증 화면일 수 있다.
            // PENDING_STALL을 넘기면 즉시 실패(크롬 종료)시킨다.
            LoopDecision::KeepWaiting => {
                let pd = *pending_deadline.get_or_insert_with(|| Instant::now() + PENDING_STALL);
                if Instant::now() >= pd {
                    let url = client.current_url().unwrap_or_default();
                    return Ok(LoginOutcome::Error(format!(
                        "로그인이 진행되지 않아 취소했습니다(캡차 외 미인식 화면 — 추가 인증 등). 마지막 페이지: {url}"
                    )));
                }
            }
        }

        // 캡차/본인확인 입력 대기 중이 아닐 때만 전체 deadline을 적용한다(네비게이션 정체 등).
        if captcha_deadline.is_none() && phone_deadline.is_none() && Instant::now() >= deadline {
            let url = client.current_url().unwrap_or_default();
            return Ok(LoginOutcome::Error(format!(
                "로그인 시간이 초과되었습니다. 마지막 페이지: {url}"
            )));
        }
        sleep(POLL_INTERVAL);
    }
}

// 챌린지 종류를 사용자 메시지용 한 줄 라벨로 바꾼다(로그인 실패 사유 표시용, 순수 함수).
fn challenge_kind_label(kind: ChallengeKind) -> &'static str {
    match kind {
        ChallengeKind::Captcha => "캡차(보안문자)",
        ChallengeKind::Otp => "본인인증(2차 인증)",
        ChallengeKind::Device => "새 기기 인증",
    }
}

// "폼 준비" 신호가 흔들리지 않고 자리잡았다고 볼 연속 확인 횟수(사수 지시: 돔이 맨 마지막까지
// 로드됐는지 확인하고 넘어가라 — 한 번 true가 떠도 곧바로 진행하지 않고 연속 N회 안정될 때만
// 진행한다). 네이버 로그인은 상위 문서가 complete가 된 뒤에도 캡차/안티봇 iframe·스크립트가
// 뒤늦게 한 번 더 로드되며 readyState/DOM이 잠깐 출렁이는데, 그 과도기에 타이핑하면 keydown
// 암호화 훅이 덜 붙어 캡차가 유발된다. 100ms 폴링 × 3회면 ~0.2초 안정 구간을 확보한다.
const FORM_READY_STABLE_POLLS: u32 = 3;

/// "폼 준비" 신호의 연속 안정 횟수를 갱신한다(순수 함수). 준비됐으면 누적, 한 번이라도
/// 흔들리면 0으로 리셋한다. `FORM_READY_STABLE_POLLS` 이상이면 호출부가 진행한다.
fn next_ready_streak(streak: u32, ready_now: bool) -> u32 {
    if ready_now {
        streak.saturating_add(1)
    } else {
        0
    }
}

// 결과 폴링도 "DOM이 전부 붙은 뒤"에 판정한다(사수 지시: 로그인 폼 대기뿐 아니라 결과 폴링도
// 돔이 싹 다 제대로 붙을 때까지). 클릭 후 착지 페이지의 상위 문서 + 모든 iframe이 complete로
// 연속 안정될 때까지는 비번오류/차단/캡차 판정을 미뤄, 네비게이션 과도기의 덜 그려진 DOM에서
// 결과를 오판(조기 캡차/차단 확정 등)하지 않게 한다. 폼 대기와 동일한 100ms × 3회 안정 기준.
const RESULT_DOM_STABLE_POLLS: u32 = 3;

/// 결과 게이트도 폼 게이트처럼 "리소스 정착(②)"까지 기다리되, 결과 페이지(캡차/광고 등)는 리소스가
/// 끊임없이 로딩돼 정착이 안 될 수 있다. 상위문서+iframe이 complete된 뒤 이만큼(×100ms ≈ 3초)
/// 지나도 정착을 못 보면 진행한다 — 안 그러면 캡차/오류 판정이 deadline까지 막혀 Error로 둔갑한다
/// (강화하되 regression은 막는 안전 폴백).
const RESULT_SETTLE_MAX_POLLS: u32 = 30;

// 착지 페이지의 모든 문서(상위 + 모든 iframe)가 complete인지 본다(폼 셀렉터 없음 — 결과
// 페이지엔 #id/#pw가 없다). 교차 출처 iframe은 contentDocument를 읽을 수 없어 통과(true)로
// 둔다(브라우저 보안상 검사 불가, 막으면 영영 못 넘어간다 — 폼 대기와 동일 규약).
const ALL_DOCS_COMPLETE_JS: &str = "(()=>{\
    if(document.readyState!=='complete')return false;\
    const frames=Array.prototype.slice.call(document.querySelectorAll('iframe'));\
    return frames.every(f=>{\
        try{const d=f.contentDocument;return !d||d.readyState==='complete';}\
        catch(e){return true;}\
    });\
})()";

/// 결과 판정 게이트가 열렸는지(순수 함수). DOM 안정 연속 횟수가 임계치 이상이면 결과를 판정한다.
/// 그 전까지는 호출부가 판정을 미루고 폴링을 계속한다.
fn result_dom_gate_open(streak: u32) -> bool {
    streak >= RESULT_DOM_STABLE_POLLS
}

// 네이버 안티봇/키입력 암호화 스크립트가 "실제로 로드(주입)"됐는지 본다(사수·사용자 지시: DOM이
// 다 붙어도 봇탐지/암호화 스크립트가 늦게 주입되면 그 전에 타이핑→캡차. 그래서 iframe 개수만이
// 아니라 이 스크립트가 네트워크로 받아져 자리잡은 것까지 확인하고 넘어간다). performance resource
// 타이밍은 리소스가 "다운로드 완료"됐을 때만 엔트리가 생기므로, 아래 패턴이 잡히면 스크립트가
// 실제로 붙은 것이다. wtm.pstatic.net=봇탐지 번들, default_ecc=keydown 암호화(eccpw), ncaptcha=캡차.
const ANTIBOT_READY_JS: &str = "(()=>{try{\
    const r=performance.getEntriesByType('resource');\
    return r.some(e=>/wtm\\.pstatic\\.net|default_ecc|ncaptcha|nclk\\.naver/i.test(e.name));\
}catch(e){return false;}})()";

// 페이지가 받은 "완료된 리소스 수"를 센다(사수 의도: 돔이 '전부' 붙었는지 — iframe 몇 개만이
// 아니라 페이지가 받는 모든 리소스(문서·스크립트·iframe·늦게 주입되는 것 포함) 로딩이 멈췄는지).
// PerformanceResourceTiming 엔트리는 리소스가 "끝났을 때"만 추가되므로, 이 수가 폴링 간에 더
// 늘지 않으면 = 그 사이 새로 끝난(=로딩 중이던) 리소스가 없다 = 로딩이 정착했다는 뜻이다.
// 늦게 주입되는 스크립트/iframe도 끝나는 순간 이 수를 늘리므로 "그 순간 iframe만" 한계를 없앤다.
const RESOURCE_COUNT_JS: &str =
    "(()=>{try{return performance.getEntriesByType('resource').length;}catch(e){return -1;}})()";

// [진단·임시] 게이트가 열리는 '바로 그 순간'의 네트워크/안티봇 상태 스냅샷(순수 관측 — 전역을
// 전혀 건드리지 않아 봇탐지 표면 0). performance 리소스 타이밍만 읽는다:
//  - rs             : document.readyState
//  - resCount       : 완료된 리소스 수
//  - sinceLastNetMs : 마지막으로 '완료된' 리소스 이후 경과(ms). 작을수록 방금도 뭔가 끝났다 =
//                     아직 로딩 활발(=탭 로딩바 도는 중). -1은 리소스 엔트리가 아직 없음.
//  - antibot        : default_ecc/wtm/ncaptcha/nclk 중 도착한 것 + 각 도착시각(ms)
//  - iframes        : 현재 iframe 개수
// 주의: performance 엔트리는 '완료' 시에만 생기므로 진행 중(in-flight) 요청을 직접 세지는 못한다.
// sinceLastNetMs 가 작다는 것이 "아직 네트워크가 활발하다(=로딩바 돎)"의 안전한 대리지표다.
const GATE_NET_SNAPSHOT_JS: &str = "(()=>{try{\
    const now=performance.now();\
    const res=performance.getEntriesByType('resource');\
    let last=0;for(const e of res){if(e.responseEnd>last)last=e.responseEnd;}\
    const since=last?Math.round(now-last):-1;\
    const pats=[['default_ecc',/default_ecc/i],['wtm',/wtm\\.pstatic\\.net/i],\
                ['ncaptcha',/ncaptcha/i],['nclk',/nclk\\.naver/i]];\
    const hits=[];for(const p of pats){const m=res.find(e=>p[1].test(e.name));\
        if(m)hits.push(p[0]+'@'+Math.round(m.responseEnd)+'ms');}\
    const frames=document.querySelectorAll('iframe').length;\
    const c=navigator.connection||{};\
    const conn={effectiveType:c.effectiveType,downlinkMbps:c.downlink,\
        downlinkMaxMbps:c.downlinkMax,rttMs:c.rtt,saveData:c.saveData,type:c.type};\
    return JSON.stringify({rs:document.readyState,resCount:res.length,\
        sinceLastNetMs:since,antibot:hits,iframes:frames,conn:conn});\
}catch(e){return '{\"err\":\"'+String(e)+'\"}';}})()";

// [진단·임시] 자동화/CDP 지문 스냅샷 — naver 봇탐지(wtm)가 읽는 클라이언트 신호가 정상 크롬과
// 다른지 확인용. 같은 계정 손 로그인(정상 크롬)=성공인데 매크로=캡차라면, 차이는 이 지문에 있다.
// **핵심은 cdpConsoleTrap**: Runtime.enable 이 켜져 있으면 CDP가 console 인자를 직렬화하며 getter를
// 호출한다(naver wtm 의 실제 CDP 탐지 방식). 우리는 enable_page_only 로 Runtime 을 안 켜므로 false 여야
// 정상이다. true 면 CDP 제어가 페이지에 누출(=캡차 유발 가능)된다는 직접 증거다.
//   webdriver       : false 여야 정상(스텔스 + AutomationControlled off). true/undefined 면 누출.
//   webdriverOwn    : navigator 자신 속성으로 webdriver 가 정의됐나(우리 스텔스 override 가 먹었나).
//   uaHeadless      : userAgent 에 "Headless" 누출(headless 탐지).
//   hasChrome/chromeRuntime/plugins/mimeTypes/languages : 정상 크롬 대비 결핍(headless 텔).
//   permMismatch    : Notification.permission='denied' && permissions.query='prompt' (고전적 headless 텔).
//   glVendor/glRenderer : SwiftShader 등 software GL = headless 텔.
const AUTOMATION_FINGERPRINT_JS: &str = "(async()=>{try{\
    let cdpConsoleTrap=false;\
    try{const t={};Object.defineProperty(t,'i',{get(){cdpConsoleTrap=true;return 1;}});console.debug(t);}catch(e){}\
    let permMismatch=null;\
    try{const p=await navigator.permissions.query({name:'notifications'});\
        permMismatch=(Notification.permission==='denied'&&p.state==='prompt');}catch(e){}\
    let glVendor='',glRenderer='';\
    try{const c=document.createElement('canvas');const gl=c.getContext('webgl')||c.getContext('experimental-webgl');\
        if(gl){const d=gl.getExtension('WEBGL_debug_renderer_info');\
            if(d){glVendor=String(gl.getParameter(d.UNMASKED_VENDOR_WEBGL));\
                  glRenderer=String(gl.getParameter(d.UNMASKED_RENDERER_WEBGL));}}}catch(e){}\
    const nav=navigator;const ua=nav.userAgent;\
    return JSON.stringify({\
        webdriver:nav.webdriver,\
        webdriverOwn:!!Object.getOwnPropertyDescriptor(nav,'webdriver'),\
        uaHeadless:/headless/i.test(ua),\
        hasChrome:!!window.chrome,\
        chromeRuntime:!!(window.chrome&&window.chrome.runtime),\
        plugins:nav.plugins.length,\
        mimeTypes:nav.mimeTypes.length,\
        languages:(nav.languages||[]).join(','),\
        permMismatch:permMismatch,\
        cdpConsoleTrap:cdpConsoleTrap,\
        glVendor:glVendor,glRenderer:glRenderer\
    });\
}catch(e){return '{\"err\":\"'+String(e)+'\"}';}})()";

/// 로그인 폼 진행 게이트(순수 함수). 사수 의도(돔이 전부 붙고 타이핑 준비 완료)를 네 신호
/// **모두**로 엄격 판정한다(폴백 없음):
/// ① `form_ready`(상위문서 complete + #id/#pw 보임·입력가능 + 로그인 버튼).
/// ② `resources_settled`(직전 폴 대비 완료 리소스 수 불변 = 새로 끝난 로딩이 없음 = 로딩 정착).
/// ③ `antibot_ready`(봇탐지/keydown 암호화 스크립트가 실제 로드됐는지 = 파일 다운로드).
/// ④ `keydown_hook_ready`(그 스크립트가 #id/#pw에 실제로 keydown 리스너를 붙였는지 = 후킹 설치).
/// 넷 다 만족하고 연속(FORM_READY_STABLE_POLLS회) 안정일 때만 타이핑한다. 시간 초과 없이 끝까지
/// 기다린다(사수 지시) — 준비 안 된 폼에 타이핑해 캡차를 유발하지 않는 것이 우선. ④ detection은
/// 실측 검증됨(#pw/#id 둘 다 붙고 getEventListeners가 잡음, 2026-06-25 로그).
fn login_form_gate_open(
    form_ready: bool,
    resources_settled: bool,
    antibot_ready: bool,
    keydown_hook_ready: bool,
) -> bool {
    form_ready && resources_settled && antibot_ready && keydown_hook_ready
}

// 로그인 폼이 "완전히" 로딩될 때까지 기다린다: 상위 문서 로딩 완료(readyState=complete) +
// #id/#pw가 화면에 보이고 입력 가능(disabled 아님) + 로그인 버튼 존재 + **모든 하위 문서(iframe)
// 까지 complete**. 마지막 조건이 사수 지시의 핵심이다 — 네이버 로그인은 상위 문서 하나가 아니라
// 캡차/안티봇 iframe 등 여러 문서가 로드되는데, 상위 하나만 complete여도 넘어가면 뒤늦게 붙는
// 키 입력 암호화/봇탐지 스크립트가 덜 자리잡아 캡차가 유발된다. 게다가 한 번 true가 떠도
// 곧바로 진행하지 않고 연속 `FORM_READY_STABLE_POLLS`회 안정될 때만 빠져나가, "맨 마지막"
// 문서까지 자리잡은 것을 확인한다. 진행 상황을 stderr로 출력한다.
fn wait_for_login_form(client: &mut CdpClient) -> bool {
    tracing::info!("[LOGIN] 로그인 폼 로딩 대기 중...");
    // 고정 대기가 아니라 폼 DOM(#id/#pw + 로그인 버튼 + 모든 iframe)이 자리잡는 즉시 진행한다.
    // 사수 지시: DOM이 끝까지 로드될 때까지 **시간 초과로 취소하지 않고 계속 기다린다**. 준비 안
    // 된 폼에 타이핑해 ID/PW가 안 들어가는 일을 원천 차단하기 위함이다. 유일한 중단 조건은 Chrome
    // 연결 자체가 끊긴 경우(사용자가 창을 닫음/크래시) — 그땐 기다릴 대상이 없으므로 CDP 호출이
    // 연속 MAX_CONN_FAIL회 실패하면 중단한다(무한 wedge 방지). 아래 100ms로 촘촘히 폴링한다.
    const MAX_CONN_FAIL: u32 = 50; // ~5초 연속 CDP 실패 = Chrome 사라짐
    let mut conn_fail = 0u32;
    // 상위 문서 + 폼 + 로그인 버튼 + 모든 하위 문서(iframe)가 complete인지 한 번에 본다. 동일
    // 출처 iframe만 contentDocument를 읽을 수 있고, 교차 출처는 검사 불가라 막지 않는다(true 취급).
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
    // 안티봇/암호화 스크립트를 한 번이라도 로드 확인했는지. performance 엔트리는 사라지지 않으니
    // 한 번 true면 계속 true로 둔다(엄격 게이트의 필수 조건 ③).
    let mut antibot_seen = false;
    // 직전 폴의 "완료 리소스 수". 이번 폴과 같으면 그 사이 새로 끝난(=로딩 중이던) 리소스가
    // 없다 = 로딩 정착(조건 ②). 첫 폴은 비교 대상이 없어 정착으로 보지 않는다.
    let mut prev_res_count: Option<i64> = None;
    loop {
        // form_ready 평가가 Err면 CDP 연결 이상(Chrome 닫힘/크래시일 수 있음). 일시적일 수
        // 있어 곧장 중단하지 않고, 연속 MAX_CONN_FAIL회 실패할 때만 Chrome이 사라진 것으로 보고
        // 중단한다. 로딩 중인 "정상 미준비"는 Err가 아니라 Ok(false)라 여기서 안 걸린다.
        let form_ready = match client.evaluate_bool(ready_expr) {
            Ok(r) => {
                conn_fail = 0;
                r
            }
            Err(_) => {
                conn_fail += 1;
                if conn_fail >= MAX_CONN_FAIL {
                    tracing::info!(
                        "[LOGIN] ✗ Chrome 연결이 끊겨 로그인 폼 대기를 중단(창이 닫혔거나 크래시)"
                    );
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
        // ④ keydown 암호화 후킹이 #id/#pw에 실제 설치됐는지까지 게이트 조건에 넣는다(하드 게이트).
        // ①②③가 다 된 뒤에만 관측한다 — 그 전엔 후킹이 붙을 수 없고 CDP 호출도 아낀다. 폴백 없이
        // 붙을 때까지 기다린다(사수 지시: 다 준비된 뒤 타이핑). detection은 실측 검증됨(2026-06-25).
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
            // [진단·임시] 게이트가 열리는 바로 그 순간의 네트워크/안티봇 스냅샷을 먼저 남긴다 —
            // 로딩바가 도는 중(=아직 네트워크 활발)에 게이트가 열리는지 실측 확인용. sinceLastNetMs
            // 가 작으면(예: <300) 방금도 리소스가 끝났다 = 아직 로딩 중인데 타이핑이 나간다는 증거.
            let snap = client
                .evaluate_string(GATE_NET_SNAPSHOT_JS)
                .unwrap_or_else(|e| format!("(스냅샷 실패: {e})"));
            tracing::info!("[LOGIN] ⏱ 게이트 OPEN 직전 네트워크 스냅샷: {snap}");
            // [진단·임시] 자동화/CDP 지문도 같이 남긴다 — 손 로그인(정상 크롬)에서 같은 JS를 찍어
            // 비교하면, 매크로에서만 캡차가 뜨는 차이(특히 cdpConsoleTrap·webdriver·headless 텔)를 짚는다.
            let fp = client
                .evaluate_string(AUTOMATION_FINGERPRINT_JS)
                .unwrap_or_else(|e| format!("(지문 실패: {e})"));
            tracing::info!("[LOGIN] 🕵 자동화/CDP 지문: {fp}");
            tracing::info!(
                "[LOGIN] ✓ 로그인 폼 완전 로딩 확인 (readyState=complete · #id/#pw 입력 가능 · 로그인 버튼 준비 · 리소스 로딩 정착 · 안티봇 스크립트 로드 · keydown 암호화 후킹 설치 확인 · {FORM_READY_STABLE_POLLS}회 연속 안정)"
            );
            return true;
        }
        // 시간 초과로 취소하지 않는다(사수 지시) — DOM이 끝까지 로드될 때까지 계속 기다린다.
        // 중단은 위 form_ready 평가의 연속 CDP 실패(Chrome 사라짐)로만 일어난다.
        sleep(Duration::from_millis(100));
    }
}

// 한 글자에 대응하는 US 키보드 물리키 정보. 합성 키 이벤트에 실제 브라우저와 동일한
// `code`·`windowsVirtualKeyCode`를 채워, DOM `event.keyCode`가 0이 되지 않게 한다.
struct KeyInfo {
    code: String,
    vk: u32,
    shift: bool,
}

// 문자를 US 키보드 배열의 (code, windowsVirtualKeyCode, Shift 필요 여부)로 매핑한다(순수 함수).
// 알파벳/숫자/비밀번호에 흔한 기호를 덮는다. 미지의 문자는 vk=0·code="" 로 떨어뜨려도
// keyDown의 `text`가 글자 입력을 담당하므로 값 자체는 들어간다(베스트에포트).
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
    // (code, windowsVirtualKeyCode, shift) — Shift+숫자 기호와 OEM 구두점.
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

// 브라우저(창) 포커스를 omnibox(주소창)에서 웹 컨텐츠로 옮긴다. CDP 입력은 렌더러
// activeElement만 바꿔 주소창 select-all/창 미포커스 상태가 안 풀리는데, 그 미포커스가
// wtm 안티봇의 봇 신호라 캡차를 키운다(사용자가 빈 화면을 손으로 클릭하면 풀리던 그 상태).
// ①Page.bringToFront 로 탭/창을 앞으로 가져와 포커스를 웹 컨텐츠로 옮기고 ②폼·링크를 피한
// 중립 body 좌표를 진짜 마우스로 클릭해 in-document 포커스 + 포인터 엔트로피를 만든다. 전부
// best-effort — 어느 단계가 실패해도 로그인은 그대로 진행한다. 적용 전후 document.hasFocus()
// 를 로그로 남겨, 이 조치가 실제로 포커스를 옮겼는지 사용자가 로그에서 검증할 수 있게 한다.
fn focus_web_contents(client: &mut CdpClient) {
    let before = client.evaluate_bool("document.hasFocus()").unwrap_or(false);

    // ① 탭/창을 앞으로: 포커스를 웹 컨텐츠로 옮겨 주소창 select-all 을 푼다.
    if let Err(error) = client.call("Page.bringToFront", json!({})) {
        tracing::warn!("[LOGIN] Page.bringToFront 실패 — 포커스 이동 일부만 적용: {error}");
    }

    // ② 폼·버튼·링크와 겹치지 않는 빈 지점을 골라 진짜 마우스로 클릭(없으면 건너뜀).
    let clicked = match neutral_body_point(client) {
        Ok(Some((x, y))) => mouse_click(client, x, y).is_ok(),
        _ => false,
    };

    let after = client.evaluate_bool("document.hasFocus()").unwrap_or(false);
    tracing::info!(
        has_focus_before = before,
        has_focus_after = after,
        body_clicked = clicked,
        "[LOGIN] 웹 컨텐츠 포커스 이동(주소창 선택 해제·캡차 완화 시도)"
    );
}

// 클릭 가능한 요소(a/button/input/select/textarea/label/onclick)와 겹치지 않는 viewport 내
// 빈 좌표를 하나 고른다. 후보 지점을 훑어 elementFromPoint 가 상호작용 요소가 아닌 첫 지점을
// 반환한다(없으면 None). 임의 좌표를 클릭해 링크/버튼을 잘못 누르는 사고를 막기 위함.
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

// (x,y)로 마우스를 옮겨 좌클릭한다 — JS .click()/.focus()가 아니라 진짜 mouse 이벤트라
// 행동 기반 봇탐지(움직임 0)를 완화한다.
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

/// 로그인 직전에 '로그인 상태 유지' 토글을 켠다. 이걸 켜야 브라우저가 npay 약관동의
/// (commonTermAgree)에 싣는 nid 세션 쿠키(NID_JST·NID_SAUTO 등, 만료가 긴 keep-login 쿠키)가
/// 발급된다 — 실측 성공 패킷(`동의+프로필까지`)은 이 쿠키를 실어 통과하는데, 우리 로그인이 이걸
/// 안 켜서 그 쿠키가 아예 안 생겼다. 여러 선택자를 방어적으로 시도하고, 못 찾아도 로그인은
/// 그대로 진행한다(best-effort, 결과는 로그로 남긴다).
fn enable_keep_signed_in(client: &mut CdpClient) {
    const JS: &str = "(()=>{\
         const el=document.querySelector('#keep')\
             ||document.querySelector('input[name=\"nvlong\"]')\
             ||document.querySelector('.keep_check input[type=checkbox]')\
             ||document.querySelector('#switch');\
         if(!el)return 'not-found';\
         if(!el.checked){el.checked=true;\
             el.dispatchEvent(new Event('click',{bubbles:true}));\
             el.dispatchEvent(new Event('change',{bubbles:true}));}\
         const nv=document.querySelector('input[name=\"nvlong\"]');if(nv)nv.value='on';\
         return el.checked?'on':'off';})()";
    match client.evaluate_string(JS) {
        Ok(result) => {
            tracing::info!(result = %result, "[LOGIN][상태유지] 로그인 상태 유지 설정 시도")
        }
        Err(error) => {
            tracing::warn!(error = %error, "[LOGIN][상태유지] 설정 실패 — 건너뜀(로그인은 계속)")
        }
    }
}

// 로그인 버튼을 사람처럼 좌표 클릭한다. 좌표를 못 구하면 .click()으로 폴백(클릭 실패가
// 로그인 자체를 막지 않도록).
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

// 폼 자동입력 실패 원인 진단 스냅샷(순수 데이터). 타이핑이 필드에 안 들어갔을 때 CDP로 읽어
// 채운 뒤 [`format_fill_diag`]로 "자세히 보기"용 한 덩어리 진단 문자열을 만든다. 세 원인
// ①포커스 엇나감 ②안티봇 후킹 미설치 ③value 부분 커밋 을 신호로 구분한다.
struct FillDiag<'a> {
    selector: &'a str,
    expected: usize,
    got: usize,
    /// 타이핑 직후 `document.activeElement.id`(포커스가 실제로 어디 잡혔나).
    active_id: String,
    /// 좌표를 찾아 마우스 클릭으로 포커스했나(false면 JS focus 폴백).
    focused_via_mouse: bool,
    field_visible: bool,
    field_disabled: bool,
    /// 입력칸이 `readOnly` 인지(타이핑 직후 관측). `disabled` 와 별개로 value 가 안 박히는
    /// 원인(가설 B): 폼이 JS로 readonly 를 풀기 전에 타이핑하면 키는 가도 value 가 0이다.
    read_only: bool,
    /// 타이핑 시점에 안티봇/keydown 암호화 스크립트가 로드돼 있었나.
    antibot_ready: bool,
    /// 타이핑한 keydown 의 기본동작이 차단(`preventDefault`)됐는지(가설 A). `Some(true)`=차단
    /// (안티봇이 합성 입력을 막음), `Some(false)`=정상, `None`=keydown 이 document 까지 도달
    /// 안 함(전파 중단 등으로 관측 불가).
    default_prevented: Option<bool>,
}

/// 진단 신호 조합으로 가장 유력한 실패 원인을 한 줄로 추정한다(순수 함수). 우선순위로 판정해
/// 동시에 여러 조건이 걸려도 가장 근본 원인부터 가리킨다.
fn fill_diag_cause(d: &FillDiag) -> &'static str {
    let focus_ok = d.active_id == d.selector.trim_start_matches('#');
    if !d.field_visible {
        "필드가 화면에서 사라짐(타이핑 직전 재렌더) — 게이트가 못 거른 과도기"
    } else if d.field_disabled {
        "필드가 비활성(disabled) — 폼이 아직 잠겨 있음"
    } else if d.read_only {
        "필드가 readOnly — 폼이 입력 잠금 상태에서 타이핑(JS가 풀기 전) → value 미반영 [가설 B]"
    } else if !focus_ok {
        "포커스가 입력칸에 안 잡힘(클릭 좌표 엇나감/오버레이가 가림) — 키가 다른 곳으로 감"
    } else if !d.antibot_ready {
        "안티봇 keydown 후킹이 미설치인 상태에서 입력 — DOM은 됐지만 후킹 늦음(게이트 강화 필요)"
    } else if d.default_prevented == Some(true) {
        "keydown 기본동작이 차단됨(preventDefault) — 안티봇이 합성 입력을 막아 value 미반영 [가설 A]"
    } else if d.got > 0 {
        "포커스·후킹 정상인데 value가 일부만 커밋(빠른 연타 경합) — 재시도로도 복구 실패"
    } else {
        "포커스·후킹 정상·기본동작 차단도 아닌데 키 입력이 value에 전혀 반영 안 됨(원인 미상 — 추가 조사 필요)"
    }
}

/// [`FillDiag`]를 "자세히 보기"에 띄울 사람이 읽는 진단 문자열로 만든다(순수 함수).
fn format_fill_diag(d: &FillDiag) -> String {
    let focus_ok = d.active_id == d.selector.trim_start_matches('#');
    let aid = if d.active_id.is_empty() {
        "(없음)"
    } else {
        d.active_id.as_str()
    };
    let dp = match d.default_prevented {
        Some(true) => "예(차단됨)",
        Some(false) => "아니오",
        None => "관측 안 됨(이벤트 미도달/전파중단)",
    };
    format!(
        "[자동입력 실패 진단] {sel} — 기대 {exp}자 · 실제 입력 {got}자 (3회 재시도 후)\n\
         · 포커스: {focus} (activeElement=#{aid}, 마우스클릭 좌표={mouse})\n\
         · 필드: 보임={vis}, disabled={dis}, readOnly={ro}\n\
         · 안티봇(keydown 암호화) 로드: {anti}\n\
         · keydown 기본동작 차단(preventDefault): {dp}\n\
         → 추정 원인: {cause}",
        sel = d.selector,
        exp = d.expected,
        got = d.got,
        focus = if focus_ok { "정상" } else { "엇나감" },
        mouse = if d.focused_via_mouse {
            "찾음"
        } else {
            "못찾음(JS focus 폴백)"
        },
        vis = d.field_visible,
        dis = d.field_disabled,
        ro = d.read_only,
        anti = d.antibot_ready,
        dp = dp,
        cause = fill_diag_cause(d),
    )
}

// 키 이벤트 계기판 recorder(1회 설치). document 버블 단계에서 keydown/keypress/beforeinput/input/
// compositionstart 를 각각 카운트하고, 마지막 keydown 의 `defaultPrevented`·key·keyCode·isComposing·
// target.id 를 기록한다(호환 위해 `window.__pmDp` 도 그대로 미러링). 이미 설치돼 있으면 리스너를 다시
// 달지 않고 카운터만 리셋한다(같은 로그인에서 #id·#pw 두 번 호출되므로). navigate 로 새 문서가 뜨면
// window 상태가 초기화되니 로그인 간 누수도 없다. best-effort 로 설치한다.
//
// 이 계기판으로 "다른 PC/원격데스크톱에서 마우스는 되는데 키만 통째로 무효"인 원인을 가른다:
//   keydown=0        → 키가 렌더러에 도달조차 못 함(창 백그라운드/원격데스크톱 키 드롭 의심)
//   keyCode==229/composing → OS 한글 IME 조합에 먹힘
//   input>0·value=0  → 폼 JS 가 value 를 되돌림
const INSTALL_KEY_RECORDER_JS: &str = "(()=>{if(!window.__pmKeyRecInstalled){\
    window.__pmKeyRecInstalled=true;\
    window.__pmKeyRec={dp:null,kd:0,kp:0,bi:0,inp:0,comp:0,lastKey:'',lastCode:0,composing:false,tgt:''};\
    const R=window.__pmKeyRec;\
    document.addEventListener('keydown',function(e){R.kd++;R.dp=e.defaultPrevented;window.__pmDp=e.defaultPrevented;R.lastKey=e.key;R.lastCode=e.keyCode;R.composing=e.isComposing;R.tgt=(e.target&&e.target.id)||'';},false);\
    document.addEventListener('keypress',function(){R.kp++;},false);\
    document.addEventListener('beforeinput',function(){R.bi++;},false);\
    document.addEventListener('input',function(){R.inp++;},false);\
    document.addEventListener('compositionstart',function(){R.comp++;},false);}\
    const R=window.__pmKeyRec;R.dp=null;window.__pmDp=null;R.kd=0;R.kp=0;R.bi=0;R.inp=0;R.comp=0;\
    R.lastKey='';R.lastCode=0;R.composing=false;R.tgt='';return true;})()";
// recorder 가 기록한 keydown `defaultPrevented` 를 읽는다: 1=차단(preventDefault 호출됨), 0=정상,
// -1=keydown 이 document 까지 도달 안 함(전파 중단/미관측).
const READ_DP_JS: &str = "(()=>{const v=window.__pmDp;return v==null?-1:(v?1:0);})()";
// 계기판 전체를 JSON 문자열로 읽는다(도달 카운트·IME·visibility·창포커스). 자동입력 0자 실패 시
// 진단 로그로 남겨 원인(렌더러 키 드롭 / IME / value 되돌림)을 가른다.
const READ_KEY_STATS_JS: &str = "(()=>{const b={vis:document.visibilityState,hasFocus:document.hasFocus()};\
    const R=window.__pmKeyRec;if(!R)return JSON.stringify(Object.assign({installed:false},b));\
    return JSON.stringify(Object.assign({installed:true,keydown:R.kd,keypress:R.kp,beforeinput:R.bi,\
    input:R.inp,compositionstart:R.comp,defaultPrevented:R.dp,lastKey:R.lastKey,lastKeyCode:R.lastCode,\
    isComposing:R.composing,lastTarget:R.tgt},b));})()";

/// 로그인 입력 스로틀(ms). 기본 0(동작 변화 없음). 환경변수 `PSTMACRO_LOGIN_THROTTLE_MS` 로 켠다 —
/// 다른 PC/원격데스크톱에서 봇탐지 점수가 높아 캡차가 반복될 때, 글자당 타이핑 지연과 필드 사이
/// 멈춤을 이 값만큼 늘려 "천천히"(사수 지시: throttle) 입력해 행동 기반 봇탐지 점수를 낮춘다.
/// 상한 500ms — 과도한 지연으로 클릭 후 전체 타임아웃을 넘기지 않게 막는다.
fn parse_throttle_ms(raw: Option<&str>) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ms| ms.min(500))
        .unwrap_or(0)
}

fn login_throttle_ms() -> u64 {
    parse_throttle_ms(std::env::var("PSTMACRO_LOGIN_THROTTLE_MS").ok().as_deref())
}

/// 스로틀이 켜져 있으면 그만큼 추가로 멈춘다(필드 사이 사람 같은 텀 강화). 꺼져 있으면 no-op.
fn throttle_pause() {
    let t = login_throttle_ms();
    if t > 0 {
        sleep(Duration::from_millis(t));
    }
}

/// 타이핑 직전에 페이지를 강제로 "전경·포커스" 상태로 만든다(best-effort). 원격 데스크톱 등에서
/// 창이 가려지면 `visibilityState=hidden` 이 되어 합성 키 이벤트(`Input.dispatchKeyEvent`)가 렌더러로
/// 전달되지 않는다(마우스만 먹혀 포커스는 잡히나 타이핑 0자 — 2026-07-02 실측 확정). 탭을 앞으로
/// 가져오고(`Page.bringToFront`) 포커스 에뮬레이션(`Emulation.setFocusEmulationEnabled`)을 켜 창이
/// 전경이 아니어도 키가 전달되게 한다. 실패해도 타이핑은 그대로 진행한다(계기판 로그로 남는다).
fn force_page_foreground(client: &mut CdpClient) {
    if let Err(error) = client.call("Page.bringToFront", json!({})) {
        tracing::debug!(error = %error, "[LOGIN] Page.bringToFront 실패(무시하고 진행)");
    }
    if let Err(error) =
        client.call("Emulation.setFocusEmulationEnabled", json!({ "enabled": true }))
    {
        tracing::debug!(error = %error, "[LOGIN] setFocusEmulationEnabled 실패(무시하고 진행)");
    }
}

// 선택자를 마우스로 클릭해 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(keydown 후킹 암호화
// 대응). 글자 사이 인위적 지연 없이 빠르게 연타한다(#267 후속). 입력 후 필드 값 길이를 확인해, 비어 있으면
// (타이밍/렌더 문제로 헛친 경우) 최대 3회 재시도한다. 채워졌으면 `Ok(None)`(성공), 3회 후에도
// 비어 있으면 원인 진단 문자열 `Ok(Some(diag))`를 반환해 호출자가 "자세히 보기" trace로 띄운다.
fn type_into(
    client: &mut CdpClient,
    selector: &str,
    text: &str,
) -> Result<Option<String>, AutomationError> {
    let expected = text.chars().count();
    // 실패 시 진단에 쓸 마지막 시도의 관측값(포커스 경로·입력된 글자 수).
    let mut focused_via_mouse = false;
    let mut last_got = 0usize;

    // 타이핑 전에 키 이벤트 계기판 recorder 를 설치한다(도달 카운트·IME·defaultPrevented 관측용).
    // best-effort: CDP 가 잠깐 실패해도 타이핑 자체는 막지 않는다(진단 보강이지 입력 경로가 아니다).
    let _ = client.evaluate(INSTALL_KEY_RECORDER_JS);

    // 창이 가려져 visibilityState=hidden 이면 합성 키가 렌더러로 전달되지 않으므로(실측 확정),
    // 타이핑 직전에 탭을 전경으로 가져오고 포커스 에뮬레이션을 켠다. best-effort.
    force_page_foreground(client);

    // 입력 스로틀(기본 0=꺼짐). 켜져 있으면 첫 시도부터 글자당 지연을 줘 사람처럼 천천히 친다.
    let throttle = login_throttle_ms();
    if throttle > 0 {
        tracing::info!(
            selector,
            throttle_ms = throttle,
            "[LOGIN] 입력 스로틀 적용 — 글자당·필드사이 지연 증가(봇탐지 점수 완화 시도)"
        );
    }

    // recorder 자가진단: JS 로 합성 keydown 을 1발 쏴 계기판 카운터가 오르는지 본다. 오르면 recorder 는
    // 정상 — 이후 실 타이핑에서 keydown 카운트가 0이면 "키가 렌더러에 도달조차 못 함"이 확정된다(recorder
    // 버그가 아니라 원격데스크톱/창 가림 등 키 드롭). 1=정상, 0=리스너 미작동, -1=recorder 미설치.
    // 카운터는 곧바로 복원해 실측을 오염시키지 않는다.
    let recorder_selftest = client
        .evaluate(
            "(()=>{const R=window.__pmKeyRec;if(!R)return -1;const b=R.kd;\
             document.dispatchEvent(new KeyboardEvent('keydown',{bubbles:true}));\
             const ok=R.kd>b;R.kd=b;return ok?1:0;})()",
        )
        .ok()
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    // CDP 로 성공적으로 디스패치한 keyDown 이벤트 수(명령 자체가 먹혔는지). 이 수와 계기판 keydown
    // 카운트가 어긋나면 "CDP 는 명령을 받았는데 렌더러가 이벤트를 버렸다"를 가리킨다.
    let mut dispatched = 0usize;

    for attempt in 0..3 {
        // 첫 시도는 글자 사이 지연 없이 빠르게 친다(#267: 타이핑 리듬 지문 제거). 재시도부터는
        // 글자마다 작은 지연을 줘, 0지연 연타로 마지막 글자들이 입력칸에 덜 반영되던 경우를 복구한다.
        // 스로틀이 켜져 있으면 첫 시도부터 그 값(재시도는 최소 35ms 보장)으로 지연을 준다.
        let per_key_delay = if attempt == 0 {
            (throttle > 0).then(|| Duration::from_millis(throttle))
        } else {
            Some(Duration::from_millis(throttle.max(35)))
        };
        // 기존 값 비우기(재시도 시 중복 입력 방지). 셀렉터는 고정 안전 문자열(#id/#pw).
        let clear = format!(
            "(()=>{{const el=document.querySelector('{selector}');\
             if(el){{el.value='';return true;}}return false;}})()"
        );
        client.evaluate(&clear)?;
        // 포커스는 사람처럼 마우스 클릭으로. 좌표를 못 구하면 JS focus로 폴백.
        focused_via_mouse = mouse_click_selector(client, selector)?;
        if !focused_via_mouse {
            let focus = format!(
                "(()=>{{const el=document.querySelector('{selector}');\
                 if(el){{el.focus();return true;}}return false;}})()"
            );
            client.evaluate(&focus)?;
        }

        for ch in text.chars() {
            let s = ch.to_string();
            let k = key_info(ch);
            // 실제 브라우저와 동일하게 code·windowsVirtualKeyCode·nativeVirtualKeyCode를 채운다.
            // 이게 없으면 DOM `event.keyCode`가 0이라, 네이버 default_ecc.js의 keydown 암호화
            // 훅/봇탐지가 합성 입력으로 판단 → eccpw가 깨지고 캡차/수동입력을 요구한다(패킷
            // 분석상 캡처된 성공 로그인은 NNB 쿠키만·bvsd 빈 채로도 즉시 성공했다).
            // Shift는 대문자뿐 아니라 Shift로 입력하는 기호(!@#$ 등)에도 일반화한다. Shift 없이
            // 대문자를 보내면 네이버가 "Caps Lock 켜짐"으로 오판해 경고를 띄우는 문제도 함께 막는다.
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
            dispatched += 1;
            // 첫 시도는 지연 0(빠른 입력, #267). 재시도부터는 글자마다 작은 지연을 줘, 빠른
            // 연타로 마지막 글자들이 입력칸에 덜 반영되던 경우를 복구한다.
            if let Some(d) = per_key_delay {
                sleep(d);
            }
        }

        let got = client
            .evaluate(&format!(
                "(()=>{{const el=document.querySelector('{selector}');\
                 return el&&el.value?el.value.length:0;}})()"
            ))?
            .as_u64()
            .unwrap_or(0) as usize;
        last_got = got;
        tracing::debug!(
            selector,
            attempt = attempt + 1,
            got,
            expected,
            "[LOGIN] 타이핑 시도 후 입력칸 글자수(0 지속 시 키가 value 에 안 박힘)"
        );
        if got >= expected {
            return Ok(None);
        }
        sleep(Duration::from_millis(500));
    }
    // 3회 후에도 채우지 못함 — 어느 원인인지(포커스 엇나감/안티봇 미설치/value 부분 커밋)
    // 구분할 수 있게 현재 상태를 한 번에 스냅샷해 진단 문자열로 돌려준다. 호출자는 이를
    // "자세히 보기" trace로 띄우고, 빈 자격증명으로는 진행하지 않는다.
    let active_id = client
        .evaluate_string(
            "(()=>{const ae=document.activeElement;return ae&&ae.id?ae.id:'';})()",
        )
        .unwrap_or_default();
    let field_visible = client
        .evaluate_bool(&format!(
            "(()=>{{const e=document.querySelector('{selector}');\
             return !!(e&&e.offsetParent!==null);}})()"
        ))
        .unwrap_or(false);
    let field_disabled = client
        .evaluate_bool(&format!(
            "(()=>{{const e=document.querySelector('{selector}');\
             return !!(e&&e.disabled);}})()"
        ))
        .unwrap_or(false);
    let read_only = client
        .evaluate_bool(&format!(
            "(()=>{{const e=document.querySelector('{selector}');\
             return !!(e&&e.readOnly);}})()"
        ))
        .unwrap_or(false);
    let antibot_ready = client.evaluate_bool(ANTIBOT_READY_JS).unwrap_or(false);
    // recorder 가 기록한 keydown 기본동작 차단 여부를 tri-state 로 읽는다(1=차단/0=정상/-1=미관측).
    let default_prevented = match client.evaluate(READ_DP_JS).ok().and_then(|v| v.as_i64()) {
        Some(1) => Some(true),
        Some(0) => Some(false),
        _ => None,
    };
    // [키 이벤트 계기판] 원인을 한 번에 가르는 진단을 로그로 남긴다:
    //   · key_stats: keydown/keypress/beforeinput/input/compositionstart 도달 카운트 + 마지막 키
    //     (key/keyCode/isComposing/target) + visibilityState + 창포커스(document.hasFocus)
    //   · recorder_selftest: 1이면 recorder 정상 → keydown 카운트 0이면 "렌더러 키 드롭" 확정
    //   · cdp_dispatched: CDP 가 받은 keyDown 수(계기판 keydown 과 어긋나면 렌더러가 버린 것)
    //   · browser: 크롬 버전(성공 PC vs 실패 PC 비교용)
    // 판별표: keydown=0 → 렌더러 도달 실패(원격데스크톱/창 가림) · keyCode=229/isComposing=true → IME
    //   · input>0 인데 value=0 → 폼 JS 가 value 되돌림 · selftest=1 & keydown=0 → 렌더러 드롭 확정.
    let key_stats = client.evaluate_string(READ_KEY_STATS_JS).unwrap_or_default();
    let browser = client
        .call("Browser.getVersion", json!({}))
        .ok()
        .and_then(|v| v.get("product").and_then(Value::as_str).map(ToOwned::to_owned))
        .unwrap_or_default();
    tracing::warn!(
        selector,
        recorder_selftest,
        cdp_dispatched = dispatched,
        browser = %browser,
        key_stats = %key_stats,
        "[LOGIN][키진단] 자동입력 0자 원인 판별용 계기판(keydown=0→렌더러 드롭 / keyCode=229·isComposing→IME / input>0·value=0→폼이 되돌림)"
    );
    // [로그인 실패 시 네이버 페이지·필드 DOM 원문 그대로 덤프] (사용자·사수 지시 2026-07-01: 우리
    // 요약이 아니라 네이버가 실제로 준 것을 그대로). 특히 document.hasFocus()=false 면 "창(OS)
    // 포커스 없음"이라 합성 키 입력이 value에 조합되지 않는 원인이다(렌더러 activeElement는 #id로
    // 잡혀도 창 포커스가 없으면 타이핑이 안 먹는다). 필드 outerHTML·상단 겹침요소·iframe여부·페이지
    // 본문 텍스트(잠금/오류 안내 원문 포함)를 한 덩어리로 남긴다.
    let raw_dump = client
        .evaluate_string(&format!(
            "(()=>{{const el=document.querySelector('{selector}');\
             const r=el?el.getBoundingClientRect():null;\
             const top=r?document.elementFromPoint(r.left+r.width/2,r.top+r.height/2):null;\
             return JSON.stringify({{\
               hasFocus:document.hasFocus(),url:location.href,title:document.title,\
               inIframe:window.top!==window.self,\
               fieldHtml:el?el.outerHTML.slice(0,300):'(field 없음)',\
               topElem:top?(top.tagName+'#'+(top.id||'')+'.'+(top.className||'')).slice(0,120):'(없음)',\
               topIsField:!!(top&&el&&(top===el||el.contains(top))),\
               bodyText:(document.body?document.body.innerText:'').replace(/\\s+/g,' ').slice(0,400)\
             }});}})()"
        ))
        .unwrap_or_default();
    tracing::warn!(
        selector,
        raw = %raw_dump,
        "[LOGIN][원문덤프] 자동입력 실패 — 네이버 로그인 페이지·필드 DOM 원문(hasFocus·필드HTML·겹침·본문)"
    );
    let diag = format_fill_diag(&FillDiag {
        selector,
        expected,
        got: last_got,
        active_id,
        focused_via_mouse,
        field_visible,
        field_disabled,
        read_only,
        antibot_ready,
        default_prevented,
    });
    tracing::warn!("[LOGIN] ✗ {diag}");
    Ok(Some(diag))
}

// 셀렉터에 해당하는 "화면에 보이는" 요소가 있는지 확인한다. `offsetParent`가 null이면
// 숨겨진 요소이므로(예: 항상 DOM에 존재하는 Caps Lock 경고) false로 본다.
// [진단·임시] 수동입력 진단모드(PSTMACRO_LOGIN_MANUAL). 자동 타이핑/클릭을 생략하고, 열린 CDP
// 크롬 창에서 사용자가 직접 로그인할 때까지(최대 180초) 세션 쿠키를 폴링한다. 같은 환경에서
// 손입력=성공이면 합성 입력(행동데이터)이 원인, 손입력에도 캡차면 환경(CDP/플래그)이 원인.
fn manual_login_wait(client: &mut CdpClient) -> Result<LoginOutcome, AutomationError> {
    tracing::info!(
        "[LOGIN] 🧪 수동입력 진단모드 — 자동 타이핑 생략. 열린 Chrome 창에서 직접 아이디/비밀번호를 입력해 로그인하세요(최대 180초). 캡차가 뜨는지 눈으로 확인하세요."
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut captcha_logged = false;
    loop {
        let cookies = collect_naver_cookies(client)?;
        if has_session_cookies(&cookies) {
            tracing::info!(
                "[LOGIN] 🧪 수동입력 성공 — 세션 쿠키 확인. 환경(CDP/플래그)은 정상 → 원인은 매크로의 합성 입력(행동데이터)."
            );
            return Ok(LoginOutcome::Ok { cookies });
        }
        if !captcha_logged && visible_exists(client, "#captchaDiv, #captcha, img#captchaimg") {
            tracing::info!(
                "[LOGIN] 🧪 수동입력 중에도 캡차 감지 — 입력이 아니라 환경(CDP/플래그)이 원인일 가능성."
            );
            captcha_logged = true;
        }
        if Instant::now() >= deadline {
            tracing::info!("[LOGIN] 🧪 수동입력 대기 시간초과(180초)");
            return Ok(LoginOutcome::Error(
                "수동입력 진단: 180초 내 로그인되지 않음".to_owned(),
            ));
        }
        sleep(Duration::from_millis(500));
    }
}

fn visible_exists(client: &mut CdpClient, selector: &str) -> bool {
    let expr = format!(
        "(()=>{{const e=document.querySelector('{selector}');\
         return !!(e&&e.offsetParent!==null);}})()"
    );
    client.evaluate_bool(&expr).unwrap_or(false)
}

// 현재 페이지에서 로그인 성공/챌린지/실패 신호를 읽는다.
fn read_signals(client: &mut CdpClient) -> Result<PageSignals, AutomationError> {
    let cookies = collect_naver_cookies(client)?;
    let logged_in = has_session_cookies(&cookies);

    let captcha = visible_exists(client, "#captchaDiv, #captcha, img#captchaimg");
    let otp = visible_exists(client, "#otp, input[name=otp], #cellphoneCertify");
    let current_url = client.current_url().unwrap_or_default();
    // 낯선 기기 추가 인증 페이지(기기 등록 확인). 성공 후의 "등록안함" 다이얼로그와 달리
    // 쿠키가 아직 없는 상태에서 사용자 조작을 요구한다.
    let device = current_url.contains("deviceConfirm") || current_url.contains("deviceCheck");
    // 실제 로그인 오류는 `#err_common`이 "보이는" 상태로 텍스트를 가진다. `.error_message`는
    // "Caps Lock is on." 경고가 항상 숨은 채 DOM에 존재하므로 단순 존재 검사는 오판한다.
    let bad_credentials = client
        .evaluate_bool(
            "(()=>{const e=document.querySelector('#err_common');\
             return !!(e&&e.offsetParent!==null&&(e.textContent||'').trim().length>0);})()",
        )
        .unwrap_or(false);
    let on_login = current_url.contains("nid.naver.com");
    let blocked = {
        let has_form = client
            .evaluate_bool("!!document.querySelector('form#frmNIDLogin, #id')")
            .unwrap_or(false);
        on_login && !has_form && !logged_in
    };
    // 계정 보호조치(평소와 다른 환경 로그인 차단) 페이지. 로그인 POST 응답이 JS로
    // /user2/help/idSafetyRelease 로 보내며, 리다이렉트 체인의 모든 단계가 이 경로를 유지한다
    // (#228, 패킷 분석). 사람이 즉석에서 풀 수 없는 종료 상태라 즉시 실패시킨다.
    let protected = current_url.contains("idSafetyRelease");
    // 계정 잠금 안내 페이지(#243). 보호조치와 달리 별도 URL 없이 같은 nid에서 200 + 잠금 안내
    // HTML로 응답되고 성공 쿠키가 없어, URL이 아니라 본문 텍스트로 식별한다. 잠금 페이지는 문구가
    // 두 가지로 관측됐다: 옛 "…아이디 잠금조치와 함께…", 그리고 2026-06-30 패킷 확인한 현행
    // "비정상적인 활동이 감지되어 아이디를 보호(잠금) 조치중입니다 … '본인 확인하기'를 통해 해제".
    // 옛 문자열 '아이디 잠금조치'만으론 현행 페이지를 못 잡아(보호(잠금) 조치중) 일반 차단으로
    // 떨어져 "잠시 후 다시" 같은 틀린 안내가 떴다 — 두 문구를 모두 본다. 단순 "아이디 잠금"은
    // 비번오류 경고문 등에 섞여 오탐할 수 있어, 잠금 페이지 고유어("아이디 잠금조치"·"보호(잠금)"·
    // "비정상적인 활동이 감지")로 좁힌다. nid 도메인 안에서만 검사하고, classify에서 회복 가능한
    // 신호(캡차/비번오류 등) 뒤에 둔다.
    let locked = on_login
        && client
            .evaluate_bool(
                "(() => { const t = document.body?.innerText || ''; \
                 return t.includes('아이디 잠금조치') || t.includes('보호(잠금)') \
                 || t.includes('비정상적인 활동이 감지'); })()",
            )
            .unwrap_or(false);
    // 본인확인(휴대전화 번호) 화면(전화번호 패킷분석). 화면에 보이는 tel 입력칸 `#phone_value`로
    // 식별한다(placeholder "휴대전화 번호"). 이 화면은 로그인 폼(#id)이 없어 blocked 휴리스틱과
    // 겹치므로 classify에서 blocked보다 앞서 처리한다.
    let phone_verify = visible_exists(client, "#phone_value");

    Ok(PageSignals {
        logged_in,
        captcha,
        otp,
        device,
        bad_credentials,
        blocked,
        protected,
        locked,
        phone_verify,
    })
}

/// 로그인 ID가 "010 + 숫자 8자리"(총 11자리) 휴대전화 형식인지(순수 함수). 본인확인 화면에서
/// 이 형식이면 그 번호를 입력·확인까지 시도하고, 아니면 즉시 보류로 떨어뜨린다(사용자 지시).
pub(crate) fn id_is_phone_format(id: &str) -> bool {
    let t = id.trim();
    t.len() == 11 && t.starts_with("010") && t.bytes().all(|b| b.is_ascii_digit())
}

/// [사수·사용자 지시: 네이버 실제 본문 그대로] 로그인이 실패로 종결되는 순간(보호조치·잠금·차단·
/// 비번오류·미지원 인증) 현재 네이버 페이지의 **원문**(착지 URL·제목·본문 텍스트)을 그대로 로그로
/// 남긴다. 우리 판정 문구가 아니라 네이버가 실제로 준 내용을 눈으로 확인하기 위함(예: 보호조치
/// 페이지의 실제 안내 문구). best-effort — 읽기 실패해도 로그인 결과 처리엔 영향 없다.
fn dump_naver_page(client: &mut CdpClient, reason: &str) {
    let raw = client
        .evaluate_string(
            "(()=>{try{return JSON.stringify({\
             url:location.href,title:document.title,\
             bodyText:(document.body?document.body.innerText:'').replace(/\\s+/g,' ').slice(0,1200)\
             });}catch(e){return '(원문 읽기 실패)';}})()",
        )
        .unwrap_or_default();
    tracing::warn!(
        reason,
        raw = %raw,
        "[LOGIN][원문] 네이버 로그인 실패 페이지 원문(착지 URL·제목·본문 텍스트 그대로)"
    );
}

// 브라우저의 모든 쿠키를 수거해 .naver.com 만 남긴다. getAllCookies는 URL/경로 필터 없이 전량을
// 주므로, 특정 URL만 조회하는 getCookies가 놓치던 nid 세션 쿠키(NID_JST 등 `.nid.naver.com`
// host-only)까지 담아 이후 약관/가입 요청의 인증 실패를 막는다.
fn collect_naver_cookies(client: &mut CdpClient) -> Result<Vec<Value>, AutomationError> {
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
                .is_some_and(|d| d.contains("naver.com"))
        })
        .collect();
    Ok(cookies)
}

/// 수거한 쿠키에 `NID_AUT`·`NID_SES`가 모두 있는지 확인한다(순수 함수).
pub(crate) fn has_session_cookies(cookies: &[Value]) -> bool {
    let names: Vec<&str> = cookies
        .iter()
        .filter_map(|c| c.get("name").and_then(Value::as_str))
        .collect();
    names.contains(&"NID_AUT") && names.contains(&"NID_SES")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(active_id: &str, antibot: bool, got: usize, vis: bool, dis: bool) -> FillDiag<'static> {
        FillDiag {
            selector: "#id",
            expected: 6,
            got,
            active_id: active_id.to_owned(),
            focused_via_mouse: true,
            field_visible: vis,
            field_disabled: dis,
            read_only: false,
            antibot_ready: antibot,
            default_prevented: None,
        }
    }

    #[test]
    fn throttle_off_when_unset_or_empty_or_zero() {
        // 기본(미설정)·빈문자열·"0"·비숫자는 전부 0(=스로틀 꺼짐, 동작 변화 없음).
        assert_eq!(parse_throttle_ms(None), 0);
        assert_eq!(parse_throttle_ms(Some("")), 0);
        assert_eq!(parse_throttle_ms(Some("0")), 0);
        assert_eq!(parse_throttle_ms(Some("abc")), 0);
    }

    #[test]
    fn throttle_parses_and_caps_at_500() {
        // 유효 값은 그대로, 공백은 무시, 상한 500ms 로 클램프한다.
        assert_eq!(parse_throttle_ms(Some("120")), 120);
        assert_eq!(parse_throttle_ms(Some("  80 ")), 80);
        assert_eq!(parse_throttle_ms(Some("500")), 500);
        assert_eq!(parse_throttle_ms(Some("9999")), 500);
    }

    #[test]
    fn fill_diag_blames_focus_when_active_element_elsewhere() {
        // 포커스가 #id가 아닌 다른 곳(#pw)에 잡혔으면 '키가 다른 곳으로 감'을 가리킨다.
        let s = format_fill_diag(&diag("pw", true, 0, true, false));
        assert!(s.contains("포커스가 입력칸에 안 잡힘"), "{s}");
        assert!(s.contains("엇나감"), "{s}");
    }

    #[test]
    fn fill_diag_blames_antibot_when_focused_but_hook_missing() {
        // 포커스는 맞는데 안티봇 후킹이 아직 없으면 '게이트 강화 필요'를 가리킨다(사수 의도 위반).
        let s = format_fill_diag(&diag("id", false, 0, true, false));
        assert!(s.contains("안티봇 keydown 후킹"), "{s}");
        assert!(s.contains("게이트 강화"), "{s}");
    }

    #[test]
    fn fill_diag_blames_partial_commit_when_some_chars_typed() {
        // 포커스·후킹 정상인데 일부 글자만 들어갔으면 연타 경합(부분 커밋)을 가리킨다.
        let s = format_fill_diag(&diag("id", true, 3, true, false));
        assert!(s.contains("일부만 커밋"), "{s}");
    }

    #[test]
    fn fill_diag_blames_visibility_and_disabled_first() {
        // 보임/disabled 문제는 포커스·안티봇보다 앞서 근본 원인으로 잡는다.
        let gone = format_fill_diag(&diag("pw", false, 0, false, false));
        assert!(gone.contains("화면에서 사라짐"), "{gone}");
        let locked = format_fill_diag(&diag("pw", false, 0, true, true));
        assert!(locked.contains("비활성(disabled)"), "{locked}");
    }

    #[test]
    fn fill_diag_blames_readonly_before_focus_and_antibot() {
        // 가설 B: 포커스/안티봇이 정상이라도 필드가 readOnly 면 그걸 먼저 근본 원인으로 잡고,
        // 진단 문자열에 readOnly 상태와 가설 라벨이 드러난다.
        let mut d = diag("id", true, 0, true, false);
        d.read_only = true;
        let s = format_fill_diag(&d);
        assert!(s.contains("readOnly=true"), "{s}");
        assert!(s.contains("가설 B"), "{s}");
    }

    #[test]
    fn fill_diag_blames_preventdefault_when_focus_and_hook_ok() {
        // 가설 A: 포커스·후킹 정상·readOnly 아님인데 keydown 기본동작이 차단됐으면(안티봇이
        // 합성 입력을 막음) preventDefault 를 근본 원인으로 가리킨다.
        let mut d = diag("id", true, 0, true, false);
        d.default_prevented = Some(true);
        let s = format_fill_diag(&d);
        assert!(s.contains("preventDefault): 예(차단됨)"), "{s}");
        assert!(s.contains("가설 A"), "{s}");
    }

    #[test]
    fn fill_diag_unknown_only_when_not_prevented() {
        // 기본동작 차단이 '아니오(Some(false))'로 관측됐는데도 value 가 0이면, 가설 A 가 아니라
        // 진짜 미상으로 남긴다(차단도 아니면서 안 들어감 → 추가 조사 필요).
        let mut d = diag("id", true, 0, true, false);
        d.default_prevented = Some(false);
        let s = format_fill_diag(&d);
        assert!(s.contains("원인 미상"), "{s}");
        assert!(s.contains("preventDefault): 아니오"), "{s}");
    }

    #[test]
    fn ready_streak_accumulates_and_resets_on_flap() {
        // 폼 준비 신호가 연속될 때만 누적되고, 한 번이라도 흔들리면 0으로 리셋된다(사수 지시:
        // 돔이 맨 마지막까지 안정될 때만 진행). 3회 연속이어야 FORM_READY_STABLE_POLLS 충족.
        let mut s = 0;
        s = next_ready_streak(s, true);
        assert_eq!(s, 1);
        s = next_ready_streak(s, true);
        assert_eq!(s, 2);
        // 과도기(iframe 뒤늦게 로딩 등)로 흔들리면 리셋.
        s = next_ready_streak(s, false);
        assert_eq!(s, 0);
        // 다시 연속 3회면 임계치 충족.
        s = next_ready_streak(s, true);
        s = next_ready_streak(s, true);
        s = next_ready_streak(s, true);
        assert!(s >= FORM_READY_STABLE_POLLS);
    }

    #[test]
    fn login_form_gate_requires_form_resources_and_antibot_all() {
        // 엄격 게이트(사수 의도): 폼 준비 + 리소스 정착 + 안티봇 스크립트 로드 + keydown 후킹 설치,
        // 넷 다 만족해야 진행.
        assert!(login_form_gate_open(true, true, true, true));
        // 넷 중 하나라도 빠지면 진행하지 않는다(타이핑 미진행 → 캡차 방지).
        assert!(!login_form_gate_open(false, true, true, true)); // 폼 미준비
        assert!(!login_form_gate_open(true, false, true, true)); // 리소스 아직 로딩 중(정착 안 됨)
        assert!(!login_form_gate_open(true, true, false, true)); // 안티봇/암호화 스크립트 미로드
        assert!(!login_form_gate_open(true, true, true, false)); // keydown 후킹 미설치
        assert!(!login_form_gate_open(false, false, false, false));
    }

    #[test]
    fn result_dom_gate_opens_only_after_consecutive_stable_polls() {
        // 사수 지시: 결과 폴링도 DOM이 전부 붙을 때까지 판정을 미룬다. 임계치 미만이면 닫힘.
        assert!(!result_dom_gate_open(0));
        assert!(!result_dom_gate_open(RESULT_DOM_STABLE_POLLS - 1));
        // 연속 안정이 임계치에 도달하면 게이트가 열려 결과를 판정한다.
        assert!(result_dom_gate_open(RESULT_DOM_STABLE_POLLS));
        assert!(result_dom_gate_open(RESULT_DOM_STABLE_POLLS + 5));
    }

    #[test]
    fn result_dom_streak_resets_when_dom_flaps_during_navigation() {
        // 결과 페이지도 next_ready_streak로 누적/리셋한다(폼 대기와 동일 규약). 과도기에 DOM이
        // 흔들리면(false) 0으로 리셋돼, 다시 연속 안정될 때까지 판정을 미룬다.
        let mut s = next_ready_streak(0, true);
        s = next_ready_streak(s, true);
        assert!(!result_dom_gate_open(s)); // 아직 2회 — 닫힘
        s = next_ready_streak(s, false); // 네비게이션 과도기로 흔들림 → 리셋
        assert_eq!(s, 0);
        assert!(!result_dom_gate_open(s));
        // 다시 연속 RESULT_DOM_STABLE_POLLS회면 게이트 열림.
        for _ in 0..RESULT_DOM_STABLE_POLLS {
            s = next_ready_streak(s, true);
        }
        assert!(result_dom_gate_open(s));
    }

    #[test]
    fn classify_prioritizes_success() {
        let s = PageSignals {
            logged_in: true,
            captcha: true,
            ..Default::default()
        };
        assert_eq!(classify(&s), Signal::Success);
    }

    #[test]
    fn classify_detects_challenge_and_errors() {
        assert_eq!(
            classify(&PageSignals {
                captcha: true,
                ..Default::default()
            }),
            Signal::Challenge(ChallengeKind::Captcha)
        );
        assert_eq!(
            classify(&PageSignals {
                otp: true,
                ..Default::default()
            }),
            Signal::Challenge(ChallengeKind::Otp)
        );
        assert_eq!(
            classify(&PageSignals {
                bad_credentials: true,
                ..Default::default()
            }),
            Signal::BadCredentials
        );
        assert_eq!(
            classify(&PageSignals {
                blocked: true,
                ..Default::default()
            }),
            Signal::Blocked
        );
        assert_eq!(classify(&PageSignals::default()), Signal::Pending);
    }

    #[test]
    fn classify_phone_verify_wins_over_blocked() {
        // 본인확인(휴대전화) 화면도 로그인 폼(#id)이 없어 blocked 휴리스틱을 동시에 만족하지만,
        // PhoneVerify로 먼저 분류돼야 한다(전화번호 패킷분석).
        assert_eq!(
            classify(&PageSignals {
                phone_verify: true,
                blocked: true,
                ..Default::default()
            }),
            Signal::PhoneVerify
        );
    }

    #[test]
    fn phone_verify_decision_and_id_format() {
        // 본인확인 화면은 루프 arm 처리(HandlePhoneVerify)로 넘긴다 — manual/headed 무관.
        for (wfh, manual) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                decide_loop_step(Signal::PhoneVerify, wfh, manual),
                LoopDecision::HandlePhoneVerify
            );
        }
        // ID가 010+숫자8자리(총11자리)면 휴대전화 형식.
        assert!(id_is_phone_format("01011111111"));
        assert!(id_is_phone_format(" 01087654321 ")); // 공백 trim
        // 아닌 형식은 모두 false.
        assert!(!id_is_phone_format("0101111111")); // 10자리
        assert!(!id_is_phone_format("010111111111")); // 12자리
        assert!(!id_is_phone_format("01111111111")); // 010으로 시작 안 함
        assert!(!id_is_phone_format("0101111111a")); // 숫자 아님
        assert!(!id_is_phone_format("invest_king7")); // 일반 ID
        assert!(!id_is_phone_format(""));
    }

    #[test]
    fn classify_protected_wins_over_blocked() {
        // 보호조치 페이지도 로그인 폼이 없어 blocked 휴리스틱을 동시에 만족하지만,
        // 명확한 종료 신호인 Protected로 분류돼야 한다(#228).
        assert_eq!(
            classify(&PageSignals {
                protected: true,
                blocked: true,
                ..Default::default()
            }),
            Signal::Protected
        );
    }

    #[test]
    fn classify_locked_wins_over_blocked() {
        // 잠금 안내 페이지도 로그인 폼이 없어 blocked 휴리스틱을 동시에 만족하지만,
        // 명확한 종료 신호인 Locked로 분류돼야 한다(#243).
        assert_eq!(
            classify(&PageSignals {
                locked: true,
                blocked: true,
                ..Default::default()
            }),
            Signal::Locked
        );
    }

    #[test]
    fn classify_recoverable_signals_win_over_locked() {
        // 잠금은 본문 텍스트 식별이라 정밀도가 낮으므로, 회복 가능한 명시적 신호(캡차/비번오류)가
        // 함께 잡히면 그쪽을 우선해 사용자가 풀 기회를 잃지 않게 한다(#243 리뷰 보완).
        assert_eq!(
            classify(&PageSignals {
                bad_credentials: true,
                locked: true,
                ..Default::default()
            }),
            Signal::BadCredentials
        );
        assert_eq!(
            classify(&PageSignals {
                captcha: true,
                locked: true,
                ..Default::default()
            }),
            Signal::Challenge(ChallengeKind::Captcha)
        );
    }

    #[test]
    fn credentials_present_rejects_empty_or_whitespace() {
        assert!(credentials_present("user", "pw"));
        assert!(!credentials_present("", "pw"));
        assert!(!credentials_present("   ", "pw"));
        assert!(!credentials_present("user", ""));
    }

    // --- decide_loop_step(signal, wait_for_human, manual_captcha) ---

    #[test]
    fn loop_success_returns() {
        assert_eq!(
            decide_loop_step(Signal::Success, false, false),
            LoopDecision::Success
        );
        assert_eq!(
            decide_loop_step(Signal::Success, true, true),
            LoopDecision::Success
        );
    }

    #[test]
    fn loop_bad_credentials_confirms_immediately() {
        // 사용자 지시: 비번오류는 즉시 확정한다(2회 대기 없음). 결과 DOM 게이트가 과도기 깜빡임을
        // 이미 걸러주므로, 여기 도달 시점엔 DOM이 정착돼 있다.
        for (wfh, manual) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                decide_loop_step(Signal::BadCredentials, wfh, manual),
                LoopDecision::ConfirmedBad
            );
        }
    }

    #[test]
    fn loop_blocked_confirms_immediately() {
        // 사용자 지시: 차단도 즉시 확정한다(2회 대기 없음).
        for (wfh, manual) in [(false, false), (true, false), (true, true)] {
            assert_eq!(
                decide_loop_step(Signal::Blocked, wfh, manual),
                LoopDecision::ConfirmedBlocked
            );
        }
    }

    #[test]
    fn loop_pending_keeps_waiting() {
        assert_eq!(
            decide_loop_step(Signal::Pending, false, false),
            LoopDecision::KeepWaiting
        );
    }

    #[test]
    fn loop_protected_and_locked_fail_fast() {
        for wfh in [false, true] {
            assert_eq!(
                decide_loop_step(Signal::Protected, wfh, false),
                LoopDecision::ConfirmedProtected
            );
            assert_eq!(
                decide_loop_step(Signal::Locked, wfh, false),
                LoopDecision::ConfirmedLocked
            );
        }
    }

    #[test]
    fn loop_captcha_first_login_fails_to_hold_but_onhold_waits() {
        let captcha = Signal::Challenge(ChallengeKind::Captcha);
        // headless: headed로 승격(manual 무관).
        assert_eq!(
            decide_loop_step(captcha, false, false),
            LoopDecision::PromoteChallenge(ChallengeKind::Captcha)
        );
        assert_eq!(
            decide_loop_step(captcha, false, true),
            LoopDecision::PromoteChallenge(ChallengeKind::Captcha)
        );
        // headed + 첫 로그인(manual=false): 즉시 실패 → 보류(FailCaptchaToHold).
        assert_eq!(
            decide_loop_step(captcha, true, false),
            LoopDecision::FailCaptchaToHold
        );
        // headed + 보류 재로그인(manual=true): 사용자가 직접 풀도록 대기(WaitCaptcha).
        assert_eq!(
            decide_loop_step(captcha, true, true),
            LoopDecision::WaitCaptcha
        );
    }

    #[test]
    fn loop_non_captcha_challenge_fails_fast_in_both_modes() {
        // 본인인증(OTP)·새 기기 인증(Device)은 캡차 외 전부 즉시 실패(#267-13).
        for wfh in [false, true] {
            assert_eq!(
                decide_loop_step(Signal::Challenge(ChallengeKind::Otp), wfh, false),
                LoopDecision::FailUnsupportedChallenge(ChallengeKind::Otp)
            );
            assert_eq!(
                decide_loop_step(Signal::Challenge(ChallengeKind::Device), wfh, true),
                LoopDecision::FailUnsupportedChallenge(ChallengeKind::Device)
            );
        }
    }

    // --- key_info: 합성 키 이벤트가 실제 브라우저 keyCode와 일치하는지 ---

    #[test]
    fn key_info_lowercase_letter_has_keycode_without_shift() {
        let k = key_info('a');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41); // VK_A == 'A'
        assert!(!k.shift);
    }

    #[test]
    fn key_info_uppercase_letter_sends_shift() {
        let k = key_info('A');
        assert_eq!(k.code, "KeyA");
        assert_eq!(k.vk, 0x41); // 대문자도 물리키는 'A'(=0x41)
        assert!(k.shift);
    }

    #[test]
    fn key_info_digit_has_keycode() {
        let k = key_info('7');
        assert_eq!(k.code, "Digit7");
        assert_eq!(k.vk, 0x37); // '7'
        assert!(!k.shift);
    }

    #[test]
    fn key_info_shifted_symbol_maps_to_base_digit_with_shift() {
        // '!' 는 Shift+1 → 물리키는 Digit1, vk 는 '1'(=0x31), shift=true.
        let bang = key_info('!');
        assert_eq!(bang.code, "Digit1");
        assert_eq!(bang.vk, 0x31);
        assert!(bang.shift);
        // '@' 는 Shift+2.
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
        // 한글 등 매핑 없는 문자는 vk=0·code="" — text 가 입력을 담당한다.
        let k = key_info('가');
        assert_eq!(k.vk, 0);
        assert_eq!(k.code, "");
        assert!(!k.shift);
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
    fn session_cookie_detection() {
        let cookies = vec![
            json!({ "name": "NID_AUT", "value": "a" }),
            json!({ "name": "NID_SES", "value": "b" }),
        ];
        assert!(has_session_cookies(&cookies));
        assert!(!has_session_cookies(&[json!({ "name": "NID_AUT" })]));
        assert!(!has_session_cookies(&[]));
    }
}
