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
// 실행되어 CDP 제어 흔적인 `navigator.webdriver` 를 일반 크롬과 동일한 값으로 맞춘다.
//
// 네이버 안티봇 번들(wtm.pstatic.net)의 검사는
//   getWebdriver(){ return void 0!==navigator.webdriver ? Boolean(navigator.webdriver).toString() : "" }
// 형태다. 일반(비자동화) 크롬은 `navigator.webdriver === false` 라 "false"를 보고한다. 따라서
// `undefined`(필드 없음)로 두면 오히려 일반 크롬과 달라지므로, 정확히 `false`로 맞춘다.
// Chrome 실행 플래그 `--disable-blink-features=AutomationControlled` 가 headed에선 네이티브로
// false를 주지만, 헤드리스에선 webdriver가 노출될 수 있어 JS로 한 번 더 false로 덮는다.
const STEALTH_INIT_JS: &str = "Object.defineProperty(navigator,'webdriver',{get:()=>false});";

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
    match run_inner(client, id, pw, wait_for_human, manual_captcha) {
        Ok(outcome) => (outcome, None),
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

    // 위 wait_for_login_form이 "폼 완전 로딩"을 확인(로그)한 뒤에만 여기 도달한다. 곧장
    // 아이디 입력 → 2초 대기 → 비밀번호 입력 → 2초 대기 → 로그인 클릭.
    // 3회 재시도 후에도 필드가 비어 있으면(일시적 렌더/타이밍 문제) type_into가 false를
    // 돌려준다. 빈/부분 자격증명으로 로그인 버튼을 누르면 결과가 #err_common/타임아웃으로
    // 분류돼 일시적 타이핑 실패가 영구 BadCredentials/Error로 둔갑하므로, 클릭하지 않고
    // 명확한 입력 실패로 중단한다.
    if !type_into(client, "#id", id)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(아이디 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }
    sleep(FIELD_PAUSE);
    if !type_into(client, "#pw", pw)? {
        return Ok(LoginOutcome::Error(
            "로그인 폼 자동 입력에 실패했습니다(비밀번호 칸이 비어 로그인을 중단). 잠시 후 다시 시도하세요."
                .to_owned(),
        ));
    }

    // 비밀번호 입력 후 사람처럼 잠깐 멈췄다가 로그인 버튼을 누른다(2초→0.8초, #14).
    sleep(FIELD_PAUSE);
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
    loop {
        // 인증 성공 직후 뜨는 "새 기기 등록" 페이지면 "등록 안함"을 눌러 마무리한다
        // (설계 5단계: browser_flow의 기존 로직 재사용). 없으면 무시한다.
        let _ = client.click_device_dontsave_if_present(Duration::from_millis(300));

        let signals = read_signals(client)?;

        // 성공(세션 쿠키)은 DOM 게이트와 무관하게 즉시 확정한다 — 성공은 쿠키로 판정하므로
        // 착지 페이지(naver.com)의 광고 iframe 로딩을 기다리느라 정상 로그인을 늦추거나 놓치지
        // 않는다. 보류 캡차 직접 입력 중에 사용자가 풀어 로그인돼도 여기서 즉시 성공 확정된다.
        if signals.logged_in {
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

        // 사수 지시: 결과 폴링도 DOM이 전부 붙을 때까지 기다린 뒤 판정한다. 상위 문서 + 모든
        // iframe이 complete로 연속 RESULT_DOM_STABLE_POLLS회 안정될 때까지는 결과를 판정하지
        // 않고 폴링만 계속한다(과도기 DOM 오판 방지). 단, 캡차 직접 입력 대기 중(captcha_deadline
        // 설정됨)이면 전체 deadline을 적용하지 않는다 — 위 캡차 상한이 따로 끊는다.
        let dom_ready = client.evaluate_bool(ALL_DOCS_COMPLETE_JS).unwrap_or(false);
        result_dom_streak = next_ready_streak(result_dom_streak, dom_ready);
        if !result_dom_gate_open(result_dom_streak) {
            if captcha_deadline.is_none() && Instant::now() >= deadline {
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
            LoopDecision::FailCaptchaToHold => return Ok(LoginOutcome::CaptchaUnsolved),
            // 캡차가 아닌 추가 인증(본인인증 OTP/새 기기 인증) — 캡차 외라 즉시 실패(#267-13).
            LoopDecision::FailUnsupportedChallenge(kind) => {
                return Ok(LoginOutcome::Error(format!(
                    "{} 화면이 떠 자동 로그인을 중단했습니다(캡차 외 즉시 실패).",
                    challenge_kind_label(kind)
                )));
            }
            LoopDecision::ConfirmedBad => return Ok(LoginOutcome::BadCredentials),
            LoopDecision::ConfirmedBlocked => return Ok(LoginOutcome::Blocked),
            LoopDecision::ConfirmedProtected => return Ok(LoginOutcome::Protected),
            LoopDecision::ConfirmedLocked => return Ok(LoginOutcome::Locked),
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

        // 캡차 직접 입력 대기 중이 아닐 때만 전체 deadline을 적용한다(네비게이션 정체 등).
        if captcha_deadline.is_none() && Instant::now() >= deadline {
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

/// 로그인 폼 진행 게이트(순수 함수). 사수 의도(돔이 전부 제대로 붙음)를 세 신호 **모두**로
/// 엄격 판정한다(폴백 없음):
/// ① `form_ready`(상위문서 complete + #id/#pw 보임·입력가능 + 로그인 버튼).
/// ② `resources_settled`(직전 폴 대비 완료 리소스 수 불변 = 새로 끝난 로딩이 없음 = 로딩 정착).
/// ③ `antibot_ready`(봇탐지/keydown 암호화 스크립트가 실제 로드됐는지).
/// 셋 다 만족하고 연속(FORM_READY_STABLE_POLLS회) 안정일 때만 진행한다. 하나라도 안 되면 10초
/// 상한까지 대기하고, 끝내 안 되면 타이핑하지 않고 로그인을 실패시킨다(캡차 유발 방지가 우선).
fn login_form_gate_open(form_ready: bool, resources_settled: bool, antibot_ready: bool) -> bool {
    form_ready && resources_settled && antibot_ready
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
        let gate = login_form_gate_open(form_ready, resources_settled, antibot_seen);
        streak = next_ready_streak(streak, gate);
        if streak >= FORM_READY_STABLE_POLLS {
            tracing::info!(
                "[LOGIN] ✓ 로그인 폼 완전 로딩 확인 (readyState=complete · #id/#pw 입력 가능 · 로그인 버튼 준비 · 리소스 로딩 정착(새 리소스 없음) · 안티봇/암호화 스크립트 로드 확인 · {FORM_READY_STABLE_POLLS}회 연속 안정)"
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

// 선택자를 마우스로 클릭해 포커스한 뒤 한 글자씩 실제 키 이벤트로 입력한다(keydown 후킹 암호화
// 대응). 글자 사이 인위적 지연 없이 빠르게 연타한다(#267 후속). 입력 후 필드 값 길이를 확인해, 비어 있으면
// (타이밍/렌더 문제로 헛친 경우) 최대 3회 재시도한다. 채워졌으면 `Ok(true)`, 3회 후에도 비어
// 있으면 `Ok(false)`를 반환해 호출자가 판단하게 한다.
fn type_into(client: &mut CdpClient, selector: &str, text: &str) -> Result<bool, AutomationError> {
    let expected = text.chars().count();

    for attempt in 0..3 {
        // 첫 시도는 글자 사이 지연 없이 빠르게 친다(#267: 타이핑 리듬 지문 제거). 재시도부터는
        // 글자마다 작은 지연을 줘, 0지연 연타로 마지막 글자들이 입력칸에 덜 반영되던 경우를 복구한다.
        let per_key_delay = if attempt == 0 {
            None
        } else {
            Some(Duration::from_millis(35))
        };
        // 기존 값 비우기(재시도 시 중복 입력 방지). 셀렉터는 고정 안전 문자열(#id/#pw).
        let clear = format!(
            "(()=>{{const el=document.querySelector('{selector}');\
             if(el){{el.value='';return true;}}return false;}})()"
        );
        client.evaluate(&clear)?;
        // 포커스는 사람처럼 마우스 클릭으로. 좌표를 못 구하면 JS focus로 폴백.
        if !mouse_click_selector(client, selector)? {
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
        if got >= expected {
            return Ok(true);
        }
        sleep(Duration::from_millis(500));
    }
    // 3회 후에도 채우지 못함 — 호출자가 빈 자격증명으로 진행하지 않도록 false를 알린다.
    Ok(false)
}

// 셀렉터에 해당하는 "화면에 보이는" 요소가 있는지 확인한다. `offsetParent`가 null이면
// 숨겨진 요소이므로(예: 항상 DOM에 존재하는 Caps Lock 경고) false로 본다.
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
    // HTML로 응답되고 성공 쿠키가 없어, URL이 아니라 본문 텍스트로 식별한다(패킷 login-lock2:
    // "비정상적인 활동이 반복되어 아이디 잠금조치와 함께 …"). 단순 "아이디 잠금"은 비번오류
    // 경고문 등에 섞여 오탐할 수 있어, 잠금 페이지 고유어인 "아이디 잠금조치"로 좁힌다. nid
    // 도메인 안에서만 검사하고, classify에서 회복 가능한 신호(캡차/비번오류 등) 뒤에 둔다.
    let locked = on_login
        && client
            .evaluate_bool("(document.body?.innerText||'').includes('아이디 잠금조치')")
            .unwrap_or(false);

    Ok(PageSignals {
        logged_in,
        captcha,
        otp,
        device,
        bad_credentials,
        blocked,
        protected,
        locked,
    })
}

// Network.getCookies로 .naver.com 쿠키를 수거한다.
fn collect_naver_cookies(client: &mut CdpClient) -> Result<Vec<Value>, AutomationError> {
    let result = client.call(
        "Network.getCookies",
        json!({ "urls": ["https://www.naver.com", "https://nid.naver.com"] }),
    )?;
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
        // 엄격 게이트(사수 의도): 폼 준비 + 리소스 정착 + 안티봇 스크립트 로드, 셋 다 만족해야 진행.
        assert!(login_form_gate_open(true, true, true));
        // 셋 중 하나라도 빠지면 진행하지 않는다(타이핑 미진행 → 캡차 방지).
        assert!(!login_form_gate_open(false, true, true)); // 폼 미준비
        assert!(!login_form_gate_open(true, false, true)); // 리소스 아직 로딩 중(정착 안 됨)
        assert!(!login_form_gate_open(true, true, false)); // 안티봇/암호화 스크립트 미로드
        assert!(!login_form_gate_open(false, false, false));
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
