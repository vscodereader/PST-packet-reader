# ADR 0004. Windows 배포판은 시크릿 Chrome과 내부 DevTools 설정만 사용한다

날짜: 2026-05-28

## 상태

Accepted

## 배경

개발 중에는 WSL Ubuntu에서 Tauri 앱을 실행했고, Windows Chrome에 붙기 위해 `172.24.32.1:9223` 같은 개발용 host/port를 화면에 노출했다.

하지만 실제 배포 대상자는 모두 Windows 사용자다.

일반 사용자는 아래 개념을 알 필요가 없다.

- Chrome DevTools host
- Chrome DevTools 포트
- WSL에서 Windows Chrome으로 접근하는 IP
- Windows/WSL 접속 프리셋

이 값들이 화면에 보이면 사용자는 무엇을 입력해야 하는지 알기 어렵다.

또한 실제 운영 기준은 “일반 Chrome이 아니라 시크릿 Chrome에서 네이버 로그인 후 실행”이다.

## 결정

배포판 기준을 Windows + 시크릿 Chrome으로 고정한다.

사용자 화면에서는 DevTools host/port 입력창과 Windows/WSL 선택 버튼을 숨긴다.

대신 화면에는 아래 버튼만 노출한다.

```text
시크릿 Chrome 열기
```

이 버튼은 Rust Tauri command를 통해 Chrome을 아래 옵션으로 실행한다.

```text
--remote-debugging-port=9222
--remote-debugging-address=127.0.0.1
--user-data-dir=%TEMP%\pstmacro-chrome-incognito-debug
--incognito
--disable-quic
--no-first-run
--no-default-browser-check
https://www.naver.com
```

프로그램 내부 실행 값은 Windows에서는 아래로 고정한다.

```text
host: 127.0.0.1
port: 9222
```

WSL 개발환경에서는 기존 개발 편의를 위해 내부 기본값만 유지한다.

```text
host: 172.24.32.1
port: 9223
```

단, WSL 값은 배포 UI에 노출하지 않는다.

## 이유

`127.0.0.1`은 “현재 사용자 본인 컴퓨터”를 뜻한다.

배포받은 사람이 어느 Windows PC에서 실행하더라도 `127.0.0.1`은 그 사람의 PC 자신을 가리킨다.

따라서 사람마다 IP를 입력하게 만들 필요가 없다.

`9222`는 Chrome 원격 디버깅 포트다.

프로그램이 Chrome을 `9222`로 열고, 같은 프로그램이 다시 `9222`로 접속하므로 사용자가 포트 번호를 알 필요가 없다.

시크릿 Chrome과 별도 `user-data-dir`을 사용하는 이유는 아래와 같다.

- 일반 Chrome 프로필과 자동화용 Chrome 프로필을 분리한다.
- 사용자의 기존 Chrome 탭과 쿠키에 영향을 줄 가능성을 줄인다.
- 작업 종료 후 Chrome을 닫으면 시크릿 로그인 세션이 사라진다.

## 결과

좋은 점:

- 일반 사용자가 host/port를 보지 않는다.
- Windows 배포판 사용법이 단순해진다.
- 일반 Chrome 프로필과 자동화용 Chrome 세션을 분리한다.
- WSL 개발용 설정은 내부에 남아 있어 개발 테스트를 계속할 수 있다.

주의할 점:

- 사용자는 시크릿 Chrome에서 네이버 로그인과 2차 인증을 직접 완료해야 한다.
- `9222` 포트가 열려 있는 동안에는 해당 Chrome을 자동화할 수 있으므로 작업 중에는 민감한 다른 사이트를 열지 않는 것이 좋다.
- Chrome 설치 위치가 특이한 PC에서는 Chrome 실행 파일을 찾지 못할 수 있다.
- 네이버 정책이나 자동화 제한 문제는 코드만으로 제거할 수 없다.

## 관련 파일

```text
src-tauri/src/lib.rs
src/features/macro-editor/stock-batch-panel.tsx
src/features/macro-editor/macro-editor.css
scripts/start-chrome-incognito-debug.bat
docs/ABD-discussion-batch-ui-summary.md
docs/windows-incognito-distribution-guide.md
```
