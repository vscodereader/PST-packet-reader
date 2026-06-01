# 0008. WSL/Linux에서도 로그인 자동화 sidecar 동작

Date: 2026-06-01

## Status

Accepted

## Context

로그인 자동화 sidecar(`src-tauri/src/naver-login/`, Playwright)는 원래
Windows 전용으로 빌드됐다. `bundle-login`이 Windows 바이너리만 만들고 Linux에는
실행 즉시 종료하는 더미(stub)를 두었으며, 로그인 시 Windows용 `chrome.exe` 경로를
`executablePath`로 넘겼다. 그 결과 WSL/Linux 개발 환경에서는 로그인 자동화를 전혀
테스트할 수 없었다(글쓰기/댓글 자동화는 OS 무관하게 이미 양쪽에서 동작).

개발자가 WSL에서도 로그인 자동화를 실행/검증할 수 있어야 한다는 요구가 있었다.

## Decision

로그인 sidecar를 Linux/WSL에서도 실제로 동작하도록 한다. 모든 변경은 **플랫폼
분기**로 처리해 Windows 동작은 그대로 보존한다.

- `bundle-login.mjs`: Linux 더미 stub 대신 **실제 Linux 바이너리**
  (`pkg --target node22-linux-x64`)를 빌드한다. Windows 바이너리 빌드는 유지한다.
- `naver-login.ts`의 `buildLaunchOptions`: Windows에서만 `channel: "chrome"`을 쓰고,
  Linux/WSL에서는 `executablePath`로 시스템 Chrome을 직접 실행한다(Playwright는
  Linux에서 Windows용 `chrome.exe`를 실행할 수 없으므로).
- `auth::config::chrome_path()`: Windows는 Windows Chrome 경로를, Linux/WSL은 시스템
  Chrome(`/usr/bin/google-chrome` 등) 경로를 반환한다. `CHROME_PATH` 환경변수 override
  유지.
- `auth::paths::app_data_root()`: `LOCALAPPDATA`가 없는 Linux/WSL에서는
  `XDG_DATA_HOME` 또는 `~/.local/share`로 대체한다(쿠키/계정 파일 저장 위치).
- `examples/auto_login.rs`: `node --experimental-strip-types`로 TS를 직접 실행하던 것을
  **빌드된 sidecar 바이너리 실행**으로 바꾼다(Node의 TS 지원 여부와 무관하게 동작).
  `chromePath`가 `Result`로 잘못 직렬화되던 기존 버그도 함께 고친다.

전제 조건: Linux/WSL에 시스템 Chrome이 설치돼 있어야 하고, 화면 모드(headed)는 WSLg
디스플레이가 필요하다.

## Consequences

- 쉬워지는 것: WSL에서도 `pnpm build:sidecar` 후 로그인 자동화를 실행/검증할 수 있다.
  더미 stub로 인한 혼란이 사라진다. 글쓰기 자동화와 합쳐 "로그인→글/댓글"을 한 환경에서
  점검할 수 있다.
- 검증: 가짜 계정으로 Linux 바이너리/예제를 실행하면 시스템 Chrome이 열려 네이버
  로그인 페이지까지 진행하고 실패를 감지한다(브라우저 실행 경로 검증 완료). 실제 계정은
  성공 쿠키(NID_AUT/NID_SES)를 받아 파일로 저장한다.
- 어려워지는 것 / 위험:
  - Linux 바이너리 빌드 시 pkg가 Linux Node 베이스도 받으므로 첫 빌드가 더 무겁다.
  - 새 리눅스 브라우저로의 로그인은 네이버가 봇으로 보고 캡챠/2차 인증을 띄울 수 있다.
    이 경우 headed 모드(WSLg)에서 사용자가 직접 처리해야 한다.
  - WSL/Linux의 쿠키 저장 위치가 Windows와 다르다(`~/.local/share/pstmacro`).
