# 로그인 자동화 테스트 가이드 (WSL / Windows)

로그인 자동화가 실제로 동작하는지 확인하는 방법이다. 글쓰기/댓글 자동화는 OS 무관하게
이미 양쪽에서 동작하므로, 이 문서는 **로그인 자동화** 검증에 집중한다.

> 보안: 실제 네이버 계정으로만, 본인 계정/허가된 테스트 범위에서만 사용한다. 쿠키·비밀번호
> 원문은 절대 커밋하지 않는다.

## 0. 준비 — sidecar 바이너리 빌드

로그인 sidecar는 빌드 산출물이다(git에 올리지 않음). 먼저 한 번 빌드한다.

```bash
pnpm install
pnpm build:sidecar
```

생성 위치(`src-tauri/binaries/`):

- `naver-login-x86_64-unknown-linux-gnu` — WSL/Linux용
- `naver-login-x86_64-pc-windows-msvc.exe` — Windows용

WSL에는 시스템 Chrome이 필요하다(없으면 설치).

```bash
google-chrome --version   # 없으면 google-chrome-stable 설치
```

## 1. 방법 A — 예제로 테스트 (가장 간단)

`src-tauri/examples/auto_login.rs`가 입력을 만들어 sidecar 바이너리를 실행한다.

**WSL (현재 개발 환경):**

```bash
cd /home/csw/projects/pstmacro/src-tauri
NAVER_ID='your_naver_id' NAVER_PWD='your_password' cargo run --example auto_login
```

**Windows (PowerShell):**

```powershell
cd <repo>\src-tauri
$env:NAVER_ID='your_naver_id'; $env:NAVER_PWD='your_password'; cargo run --example auto_login
```

- 옵션: 끝에 `-- --headless`를 붙이면 브라우저를 숨긴다(2차 인증/캡챠가 있으면 화면이
  보여야 하므로 평소엔 붙이지 않는다).
- 성공하면 쿠키 JSON이 출력된다. 비밀번호가 틀리면 `로그인 실패: 아이디 또는 비밀번호를
확인해주세요`가 나오는데, 이는 **브라우저가 네이버까지 정상 진행했다는 뜻**이다.
- 화면 모드는 WSL에서 WSLg 디스플레이가 필요하다(`echo $DISPLAY`로 확인).

## 2. 방법 B — 바이너리를 직접 실행

입력 JSON을 만들어 바이너리에 넘긴다.

`input.json`:

```json
{
  "accountId": "your_naver_id",
  "id": "your_naver_id",
  "password": "your_password",
  "cookiesPath": "/tmp/cookies.json",
  "headless": false,
  "chromePath": "/usr/bin/google-chrome"
}
```

**WSL:**

```bash
./src-tauri/binaries/naver-login-x86_64-unknown-linux-gnu input.json
```

**Windows:** `chromePath`를 `C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe`로,
`cookiesPath`를 Windows 경로로 바꾼 뒤:

```powershell
.\src-tauri\binaries\naver-login-x86_64-pc-windows-msvc.exe input.json
```

성공하면 `cookiesPath`에 `{ accountId, savedAt, cookies: [...] }` 형태로 저장된다.

## 3. 글쓰기까지 이어서 테스트

로그인으로 저장된 쿠키 파일은 앱 데이터 경로에 들어간다.

- Windows: `%LOCALAPPDATA%\pstmacro\cookies\<계정>.json`
- WSL/Linux: `~/.local/share/pstmacro/cookies/<계정>.json`

이후 글쓰기 화면의 **"로그인 계정 ID"** 칸에 그 계정 ID를 입력하고 실행하면, 저장된
쿠키를 Chrome에 주입해 자동 로그인 상태로 글·댓글을 작성한다([ADR-0007] 참고).

## 참고

- WSL/Linux 지원 결정: [ADR-0008](adr/0008-wsl-linux-login-sidecar.md)
- 쿠키 연결 결정: [ADR-0007](adr/0007-login-cookie-bridge.md)
- 주의: 새 환경의 브라우저 로그인은 네이버가 캡챠/2차 인증을 요구할 수 있다. 그때는
  headed 모드에서 직접 처리한다.

[ADR-0007]: adr/0007-login-cookie-bridge.md
