# 네이버 증권 토론 자동화 실행 가이드

이 문서는 현재 Rust/Tauri/pnpm 환경에서 네이버 증권 토론 자동화를 실행하고 결과를 확인하는 방법을 정리한다.

## 목적

프로그램은 로그인된 Chrome 세션을 사용해서 네이버 증권 토론방에서 다음 작업 중 하나를 수행한다.

- `1. 글쓰기`: 랜덤 토론방으로 이동한 뒤 제목/본문을 등록한다.
- `2. 댓글쓰기`: 랜덤 토론방으로 이동하고 임의의 게시글을 연 뒤 댓글을 등록한다.

로그인은 프로그램이 직접 수행하지 않는다. 사용자가 Chrome에서 먼저 네이버 로그인을 완료한 뒤 프로그램을 실행한다.

## 실행 전 준비

PowerShell에서 아래 순서대로 실행한다.

```powershell
chcp 65001
[Console]::InputEncoding = [System.Text.UTF8Encoding]::new()
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()
```

기존 Chrome을 종료한다.

```powershell
taskkill /F /IM chrome.exe
```

Chrome이 실행 중이 아니면 오류가 나올 수 있지만 무시해도 된다.

Chrome을 DevTools 원격 디버깅 모드로 실행한다.

```powershell
& "C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222 --user-data-dir="$env:TEMP\pstmacro-chrome-debug"
```

Chrome 창이 뜨면 그 창에서 네이버 로그인을 완료한다.

## Chrome 연결 확인

PowerShell에서 아래 명령을 실행한다.

```powershell
Invoke-WebRequest http://127.0.0.1:9222/json/version -UseBasicParsing
```

`StatusCode : 200`이 나오면 Windows 쪽 Chrome DevTools 포트는 정상이다.

WSL에서 접근 가능한지 확인한다.

```powershell
wsl -d Ubuntu -- bash -lc "curl -s http://172.24.32.1:9223/json/version | head"
```

`"Browser": "Chrome/..."` 내용이 나오면 WSL에서도 접근 가능하다.

## 프로그램 실행

아래 명령을 실행한다.

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro/src-tauri && cargo run --bin naver_discussion_cli -- --host 172.24.32.1 --port 9223'
```

실행하면 아래 메뉴가 나온다.

```text
작업 선택:
1. 글쓰기
2. 댓글쓰기
번호 입력:
```

## 글쓰기 실행 예시

```text
번호 입력: 1
제목 입력: 테스트 제목
내용 입력:
여러 줄 입력 가능. 마지막 줄에 END 입력 후 Enter를 누르면 실행합니다.
테스트 내용입니다.
END
```

성공하면 아래와 비슷하게 표시된다.

```text
완료
로그인 확인: 닉네임
선택 카테고리: ...
선택 순위: ...
선택 종목: ...
입력 대상: 글쓰기
등록 실행: 완료
현재 URL: ...
```

## 댓글쓰기 실행 예시

```text
번호 입력: 2
댓글 내용 입력:
여러 줄 입력 가능. 마지막 줄에 END 입력 후 Enter를 누르면 실행합니다.
테스트 댓글입니다.
END
```

댓글쓰기 전에 프로필 생성이 필요하면 프로그램이 자동으로 `글쓰기 > 설정하기 > 소개 2222 > 완료` 흐름을 수행한다. 그 뒤 랜덤 게시글을 열고 댓글 등록 패킷을 실행한다.

## 결과 확인 방법

실행 결과는 두 군데에서 확인한다.

1. PowerShell 출력
   - `완료`
   - `로그인 확인`
   - `입력 대상`
   - `등록 실행: 완료`

2. Chrome 화면
   - 글쓰기 실행 후 토론방 목록을 새로고침해서 새 글이 보이는지 확인한다.
   - 댓글쓰기 실행 후 열린 게시글의 댓글 영역에서 댓글이 보이는지 확인한다.

## 패킷 기반으로 구현된 함수

패킷 기반 함수는 아래와 같다.

- 로그인 확인: `read_login_profile_from_packet`
- 글쓰기 등록 흐름: `submit_post_and_refresh`
- 글쓰기 실제 패킷 전송: `NaverPacketClient::submit_post`
- 댓글 등록 흐름: `submit_comment_and_refresh`
- 댓글 실제 패킷 전송: `NaverPacketClient::submit_comment`

글쓰기 등록 패킷:

```text
POST https://m.stock.naver.com/front-api/discussion/form?discussionType=...&itemCode=...
POST https://m.stock.naver.com/front-api/discussion/add
content-type: application/json
```

`form` 응답의 `result.txId`를 받은 뒤 `add` 요청에 넣어야 한다.

댓글 등록 패킷:

```text
GET https://apis.naver.com/commentBox/cbox/web_naver_token_json.json?...
POST https://apis.naver.com/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=
content-type: application/x-www-form-urlencoded
```

패킷 증빙 파일은 바탕화면에 생성되어 있다.

```text
C:\Users\user\Desktop\post_packet.txt
C:\Users\user\Desktop\comment_packet.txt
C:\Users\user\Desktop\profile_packet.txt
```

## 주의사항

- Chrome은 반드시 `--remote-debugging-port=9222` 옵션으로 실행해야 한다.
- WSL에서 접근할 때는 현재 환경 기준 `172.24.32.1:9223`을 사용한다.
- 로그인은 Chrome에서 먼저 완료해야 한다.
- 네이버페이 약관 동의 화면이 뜨면 사용자가 직접 확인하고 처리해야 한다.
- 실제 서비스 정책 위반 여부는 별도로 확인해야 한다.
- 실제 글쓰기/댓글 생성 HTTP 요청은 Chrome JavaScript가 아니라 Rust `reqwest` 클라이언트가 보낸다.
