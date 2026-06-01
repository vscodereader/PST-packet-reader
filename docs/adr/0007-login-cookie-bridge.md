# 0007. 로그인 자동화 쿠키를 글쓰기 자동화에 연결 (cookie bridge)

Date: 2026-06-01

## Status

Accepted

## Context

이 프로젝트에는 서로 다른 시점에 개발된 두 자동화가 있다.

- **로그인 자동화** (`src-tauri/src/auth/`, feat/41, PR #49로 master 병합): Playwright
  sidecar가 네이버에 로그인하고 쿠키를 파일
  (`%LOCALAPPDATA%\pstmacro\cookies\<계정>.json`)에 저장한다. Windows 배포 빌드에서만
  동작하며, Linux/WSL에서는 의도된 더미 stub이다.
- **글쓰기/댓글 자동화** (`src-tauri/src/naver_automation/`): 켜져 있는 Chrome의
  DevTools 세션에서 `Network.getCookies`로 쿠키를 읽어 Rust `reqwest`로 글/댓글 등록
  패킷을 전송한다. 지금까지는 사용자가 시크릿 Chrome에 **직접 로그인**한 세션을
  전제로 했다.

두 자동화는 쿠키를 다루는 방식이 달랐다. 로그인 자동화는 쿠키를 **파일**에 저장하고,
글쓰기 자동화는 **켜져 있는 Chrome 세션**에서 읽는다. 따라서 코드를 한 브랜치로 병합한
것만으로는 "자동 로그인 → 자동 글/댓글 작성"이 끝까지 이어지지 않았다.

## Decision

글쓰기 자동화 실행 시 **계정 ID(선택)** 를 받아, 그 계정의 저장된 쿠키를 Chrome
DevTools `Network.setCookie`로 **Chrome 세션에 주입**한 뒤 기존 흐름을 그대로 실행한다.

- `NaverDiscussionRequest` / `NaverPostWithCommentRequest` / `DiscussionBatchRequest`에
  `account_id: Option<String>` 필드를 추가한다.
- `account_id`가 지정되면 `CdpClient::inject_account_cookies`가
  `auth::read_account_cookies`로 파일 쿠키를 읽어 `Network.setCookie`로 주입한다.
  쿠키 객체를 CDP 파라미터로 바꾸는 변환은 순수 함수
  `naver_automation::cookie_bridge`로 분리해 단위 테스트한다.
- `account_id`가 비어 있으면 **기존 동작(사용자가 직접 로그인한 Chrome 세션 사용)** 을
  그대로 유지한다.
- UI(`stock-batch-panel.tsx`)에 "로그인 계정 ID (선택)" 입력을 추가한다. 비우면 기존
  방식이다.

쿠키를 파일에서 직접 읽어 `reqwest` 클라이언트를 만드는 대신 **Chrome 세션에 주입**하는
방식을 택했다. 이렇게 하면 Chrome이 실제로 로그인된 상태가 되어 화면 표시·토론방
이동·새로고침 등 기존 흐름이 수동 로그인 때와 **완전히 동일하게** 동작하고, 글/댓글
등록 코드를 한 줄도 바꾸지 않아도 된다.

## Consequences

- 쉬워지는 것: 로그인 자동화가 저장한 쿠키로 글/댓글 자동화를 이어서 실행할 수 있다.
  기존 수동 로그인 경로는 그대로 보존된다(`account_id` 미지정 시).
- 어려워지는 것 / 위험:
  - 로그인 자동화(Playwright)는 Windows 배포 빌드에서만 실제 쿠키를 만든다. 따라서
    "로그인 → 파일 → 주입 → 글쓰기"의 **끝까지 동작은 Windows에서만 검증 가능**하고,
    WSL 개발 환경에서는 쿠키 변환 단위 테스트까지만 검증된다.
  - 주입은 Chrome 원격 디버깅 세션이 열려 있어야 한다(시크릿 Chrome).
  - 계정 선택 UI는 현재 단순 텍스트 입력이다. 사수 지정 UI(계정 목록/큐 화면)가 적용되면
    그 화면의 계정 선택과 연결하는 후속 작업이 필요하다.
- 후속 작업: 사수 지정 UI 적용 시 계정 목록과 글쓰기 화면의 계정 선택을 통합한다.
