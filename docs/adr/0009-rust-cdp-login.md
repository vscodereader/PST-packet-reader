# 0009. Rust CDP 기반 네이버 로그인 (Playwright sidecar 제거)

Date: 2026-06-01

## Status

Accepted

Supersedes:

- [ADR-0002](0002-naver-http-login-rust-reqwest.md) — reqwest 패킷 재현 방향을
  대체한다(실제 브라우저를 쓰므로 ECC 리버스가 불필요).
- ADR-0008 (WSL/Linux 로그인 sidecar) — TS sidecar 크로스플랫폼 빌드 대신 Rust
  런처의 플랫폼 분기로 대체한다(분기 *로직*은 계승).

## Context

네이버 자동 로그인은 지금까지 TS Playwright sidecar(`src-tauri/src/naver-login/`,
`pkg`로 번들된 externalBin)가 담당하고, Rust(`auth/playwright.rs`)는 그 sidecar를
`tauri-plugin-shell`로 spawn해 쿠키 파일이 써지길 기다리는 구조였다.

한편 다운스트림 자동화(글쓰기·댓글·토론)는 이미 Rust가 raw CDP(Chrome DevTools
Protocol over `tungstenite`)로 Chrome을 직접 조종한다(`naver_automation::CdpClient`).
즉 로그인 직후 단계부터는 Rust가 이미 브라우저를 몰고 있고, TS sidecar가 유일하게
더 하는 일은 "Chrome 실행 + 로그인 폼에 ID/PW 타이핑" 둘뿐이다. 그 둘도 같은
`CdpClient`로 할 수 있다.

## Decision

로그인을 별도 프로세스(sidecar)가 아니라 **Rust 함수**로 만든다. 시스템 Chrome을
CDP로 띄워 실제 로그인 폼을 채우고(페이지 JS가 ECC 암호화 수행), `NID_AUT`/`NID_SES`
쿠키를 수거해 저장한 뒤 타입화된 결과를 반환한다. Node·Playwright·`pkg`·externalBin을
전부 제거하고 단일 Rust 바이너리로 수렴한다.

> **sidecar의 성격:** 상태 없는 로그인 함수. 한 계정의 ID/PW를 받아 fresh(시크릿 등가)
> Chrome으로 네이버에 로그인하고, 성공하면 쿠키 파일을 남기고, 실패 사유를 타입으로
> 구분해 반환한 뒤 브라우저를 종료한다. 그 이상은 책임지지 않는다.

### 신규/변경 모듈 (`src-tauri/src/`)

- `auth/chrome.rs`(신규) — Chrome 런처. 시스템 Chrome을 `--remote-debugging-port`로
  띄우고, fresh `--user-data-dir`(임시), `DevToolsActivePort` 파일 폴링으로 실제 포트를
  확정한다. `Drop`에서 자식 프로세스 kill + 임시 디렉토리 삭제.
- `auth/login_flow.rs`(신규) — 로그인 시퀀스(CDP). `Page.navigate` →
  **`Input.dispatchKeyEvent`/`Input.insertText`**로 사람처럼 타이핑(⚠️ `input.value=`
  직접 설정 금지: 네이버가 keydown을 후킹해 암호화하므로 값만 꽂으면 깨진다) → 로그인
  클릭 → 결과 분류 → 성공 시 `Network.getCookies`로 `.naver.com` 쿠키 수거.
- 오케스트레이터(`auth/mod.rs`) — headless 우선 → 챌린지 시 headed 승격 루프.
  `run_playwright_login`을 대체.
- `auth/playwright.rs`(삭제), `naver_automation::CdpClient`(재사용, 가시성 조정).

### 타입

```rust
pub enum ChallengeKind { Captcha, Otp, Device }

pub enum LoginOutcome {
    Ok { cookies: Vec<serde_json::Value> },
    ChallengeRequired { kind: ChallengeKind },
    BadCredentials,
    Error(String),
}
```

### 승격 루프

1. `chrome::launch(headless = true)` → `CdpClient` attach → `login_flow::run`
2. `Ok{cookies}` → 쿠키 파일 저장 → 완료. `ChallengeRequired{_}` → headless였다면
   Chrome 종료 후 `headless = false`로 처음부터 재실행(사용자가 직접 해결).
   `BadCredentials` → 재시도 없이 오류 반환(계정 잠금 위험). `Error` → 표면화.
3. headed 재실행 후에도 챌린지/오류 → 표면화.

### 삭제 대상

- `src-tauri/src/naver-login/` 전체, `package.json`의 `build:sidecar`·`pkg` deps,
  `tauri.conf.json`의 `externalBin`, CI의 `pnpm build:sidecar` 선행 단계.

## Consequences

- 쉬워지는 것: Node/Playwright/pkg/externalBin 제거 → 단일 Rust 바이너리. 빌드·배포·CI
  단순화. 로그인과 다운스트림이 같은 CDP 인프라를 공유.
- 어려워지는 것 / 위험:
  - `puppeteer-extra-stealth` 상실 → 봇 탐지 가능성. 완화: `--headless=new`, headed 승격
    안전망, fresh 컨텍스트 + 실제 키 이벤트(`Input.dispatchKeyEvent`). 필요 시
    `Page.addScriptToEvaluateOnNewDocument`로 핑거프린트 보정.
  - 네이버 로그인 폼/결과 페이지 구조 변경 시 분류 로직 수정 필요(셀렉터·판정 문자열을
    `login_flow` 한 곳에 모음).
  - `DevToolsActivePort` 미생성/지연 → 포트 폴링 타임아웃·명확한 에러.
  - Linux headed는 WSLg 디스플레이 필요.
- 검증: 순수 함수(포트 파싱, 결과 분류, 승격 루프)는 브라우저 없이 단위 테스트. 실제
  Chrome end-to-end는 수동/통합 단계로 분리(ADR 0008의 검증 방식 계승).
