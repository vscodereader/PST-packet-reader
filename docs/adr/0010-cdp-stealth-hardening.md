# 10. CDP 로그인 스텔스 보강 (Playwright stealth 대체)

Date: 2026-06-05

## Status

Accepted — 구현·검증 완료(2026-06-05).

- 4축 스텔스(키 keyCode/code·`navigator.webdriver=false`·행동 위장·`Runtime.enable`
  차단)를 PR #102로 구현해 master 머지. 실제 네이버 계정 로그인에서 **보안문자 없이
  통과 확인**(headed). 본문은 작성 시점(2축)을 그대로 두되, 최종 구현은 4축이다.
- 사수(pallas-dev) 사후 비준 대상. 형식 비준 전이라도 코드는 라이브 상태.

Relates to [ADR-0009](0009-rust-cdp-login.md) — 그 설계의 "위험 & 완화"에 적힌
"필요 시 추후 핑거프린트를 `Page.addScriptToEvaluateOnNewDocument`로 보정"을
구체화한다(supersede가 아니라 확장).

## Context

ADR-0009로 Playwright sidecar를 제거하고 Rust raw CDP 로그인으로 전환하면서,
`puppeteer-extra-plugin-stealth`가 자동으로 해주던 봇 위장(stealth)을 잃었다.
그 결과 네이버 ncaptcha 점수형 판정에서 **보안문자(이미지 캡차)** 가 뜰 수 있다.

PR #102가 그 위장을 직접 다시 넣는 작업의 시작이다. 현재까지 2개:

- keyCode/code/windowsVirtualKeyCode 채우기(합성 키 → `event.keyCode 0` 제거) —
  사수가 PR #62 `login_flow.rs:210`에서 먼저 지적한 항목.
- `navigator.webdriver` 마스킹 + `--disable-blink-features=AutomationControlled`.

추가 stealth를 더 넣기 전에, 사수(pallas-dev)가 PR #49/#62에서 못 박은 규칙
**"라이브러리 도입·구조화는 혼자 결정하지 말고 문서화·협의"** 에 따라 본 ADR로
범위·접근·검증을 먼저 합의한다.

### 핵심 정정 — 앱은 기본 **headed**로 로그인한다

`lib.rs::enqueue_cookie_refresh`는 `headless.unwrap_or(false)`로 큐에 넣고,
오케스트레이터(`auth/login.rs`)는 headless 시 챌린지가 나오면 headed로 승격한다.
즉 **기본 경로는 실제로 보이는 Chrome(headed)** 이다.

실제 headed Chrome은 `navigator.plugins`·`window.chrome`·WebGL/GPU·
`outerWidth/Height`·UA·`hardwareConcurrency`가 **이미 진짜**다(패킷 캡처에서
앱 로그인의 UA가 정상 `Chrome/148` + 완전한 `sec-ch-ua` client hints로 확인됨,
"HeadlessChrome" 토큰 없음). 따라서 흔히 떠도는 "헤드리스용 핑거프린트 evasion"
(plugins 가짜 채우기, webgl vendor 위조, window.chrome 만들기, UA override)을
**headed에 그대로 덮으면 진짜 값과의 불일치가 생겨 오히려 탐지된다**(스텔스 역효과).

→ headed에서 남은 실제 자동화 신호는 두 부류뿐이다:

1. `navigator.webdriver`(CDP 제어 노출) — **이미 처리(PR #102)**.
2. **행동 신호** — 마우스 이벤트 0, 글자 사이 타이핑 간격 0, 포커스/클릭을 진짜
   입력이 아닌 JS(`el.focus()`/`b.click()`)로 처리.

핑거프린트 evasion(아래 "headless 한정")은 **headless로 돌 때만** 의미가 있다.

## Decision

스텔스 보강을 **단계적**으로, **headed 기본 경로의 행동 위장을 최우선**으로 진행한다.
모두 백엔드(`auth/login_flow.rs`, `auth/chrome.rs`)에서 raw CDP 호출로 구현하며,
프론트엔드와 Playwright/Node는 재도입하지 않는다(ADR-0009 유지).

### Phase 0 — 완료 (PR #102)

keyCode/code 보강 + `navigator.webdriver` 마스킹 + `AutomationControlled` 비활성.

### Phase 1 — 행동 위장 (headed·headless 공통, 최우선)

- **실제 마우스 이벤트**: `Input.dispatchMouseEvent`(`mouseMoved`→`mousePressed`→
  `mouseReleased`)로 `#id`/`#pw` 포커스와 로그인 버튼 클릭을 좌표 기반으로 수행.
  JS `.focus()`/`.click()` 의존을 줄인다(요소 중심 좌표는 `getBoundingClientRect`로 산출).
- **타이핑 간격 랜덤화**: 글자 사이에 무작위 지연(대략 60~180ms, 인덱스 기반 의사난수)
  을 둔다. `Math.random`/`Date::now` 같은 비결정 API는 테스트를 깨므로, 지연 산출은
  순수 함수(시드+인덱스)로 분리해 단위 테스트한다.

### Phase 2 — headless 한정 핑거프린트 (조건부)

headless로 실행하는 경로에 **한해서만** 적용한다. headed에는 적용하지 않는다
(위 "핵심 정정"). 적용 시에도 **일관성**(UA↔platform↔WebGL↔plugins가 서로
모순되지 않게)과 **탐지 테스트 통과**를 전제한다.

- `Network.setUserAgentOverride`로 UA의 `HeadlessChrome` 토큰 제거 +
  `acceptLanguage`/`platform`/`userAgentMetadata`(Sec-CH-UA) 일관화.
- `Page.addScriptToEvaluateOnNewDocument`로 `navigator.plugins`/`languages`/
  `window.chrome` 등 evasion JS 주입. 각 스니펫은 `puppeteer-extra-plugin-stealth`
  (MIT)의 해당 evasion을 출처로 포팅하되, **override의 `toString()`이 네이티브처럼
  보이도록** 처리한다(어설픈 override 자체가 탐지 신호).

### Phase 3 — 고급 (지연)

CDP 자체 탐지(`Runtime.enable` 누출 회피, `rebrowser-patches` 아이디어), canvas/
AudioContext/WebRTC 지문은 군비경쟁 영역이라 효과 대비 비용이 커 후순위로 둔다.

### 범위 밖 (제품/환경 결정)

- 비행기모드 모바일 IP 회전(`auth/adb.rs`): IP 평판은 양날의 칼이라 별도 논의.
- 매번 fresh 시크릿 프로필(방문기록·쿠키 0): ADR-0009의 "상태 없는 fresh" 정책.

### 검증

`navigator.webdriver`/`plugins` 등 JS 상태는 브라우저 의존이라 단위 테스트가 어렵다.
대신 (1) 지연 산출·키 매핑 같은 **순수 함수는 단위 테스트**, (2) stealth 효과는
탐지 테스트 페이지(예: `sannysoft`류) 또는 실계정 e2e로 **수동 검증** 후 결과를 PR에
기록한다. ncaptcha는 점수형이라 "100% 차단 제거"가 아니라 "캡차 발생률 저감"이 목표다.

## Consequences

쉬워지는 것:

- 봇 점수를 사람 수준으로 낮춰 보안문자 발생률을 줄인다. 행동 위장은 headed 기본
  경로에 바로 효과가 있고, 진짜 값과 충돌하지 않아 안전하다.
- 헛수고 방지: headed에 무의미·위험한 핑거프린트 위조를 하지 않는다.

어려워지는 것 / 위험:

- 스텔스는 군비경쟁 — 다 넣어도 100% 보장이 아니며, 네이버가 탐지를 조이면 재작업.
- 어설픈 override는 그 자체가 탐지 신호가 될 수 있어, Phase 2는 일관성·탐지 테스트
  통과를 반드시 전제한다.
- 마우스 좌표/타이밍 로직이 늘면 유지보수 포인트가 는다 → 셀렉터·좌표·지연을
  `login_flow` 한 곳에 모은다(ADR-0009의 셀렉터 집약 원칙 계승).

후속:

- 합의되면 Phase 1을 별도 이슈/PR로 구현(행동 위장 + 순수 함수 테스트).
- 가능하면 동기의 Playwright 코드에서 실제로 켜져 있던 evasion 목록을 역추적해
  Phase 2 범위를 정확히 한다.
