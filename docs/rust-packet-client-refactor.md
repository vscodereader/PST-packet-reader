# Rust 패킷 클라이언트 전환 상세 기록

작성일: 2026-05-28

## 이 문서의 목적

이 문서는 네이버 증권 토론 자동화 작업을 다시 읽었을 때, 왜 코드를 이렇게 바꿨는지와 어떤 파일이 어떤 역할을 하는지 바로 이해하기 위해 작성했다.

처음 구현은 Rust/Tauri 코드에서 Chrome DevTools를 통해 브라우저 탭 안의 JavaScript `fetch`를 실행하는 방식이었다. 이 방식은 화면 자동화보다 패킷 구조에 가까웠지만, 실제 HTTP 요청을 보내는 주체가 Rust가 아니라 Chrome 페이지 런타임이었다. 사수 피드백은 "프론트로 넘겨서 처리하면 Rust를 쓰는 의미가 약하다"는 것이었다.

그래서 현재 구현은 아래 구조로 바뀌었다.

1. Chrome은 로그인과 2차 인증을 사용자가 완료한 세션을 제공한다.
2. Rust는 Chrome DevTools Protocol로 현재 로그인 세션의 네이버 쿠키를 읽는다.
3. Rust는 `reqwest` HTTP 클라이언트에 쿠키와 헤더를 넣는다.
4. Rust가 직접 네이버 API로 글쓰기/댓글쓰기 HTTP 요청을 보낸다.
5. 요청 성공 후 Chrome 화면은 새로고침해서 결과를 확인한다.

즉, 글쓰기와 댓글 등록의 실제 패킷 요청 주체는 이제 브라우저 프론트가 아니라 Rust 코드다.

## 사수 요구사항을 어떻게 해석했는가

사수 요구사항은 크게 두 가지였다.

첫 번째는 기능을 한 파일에 몰아넣지 말고 함수로 나누는 것이다. 예를 들어 페이지 이동 함수, 글쓰기 버튼 클릭 함수, 글 작성 함수, 등록 함수, 댓글 작성 함수처럼 기능 단위가 드러나야 한다.

두 번째는 F12에서 XPath나 selector만 보는 방식이 아니라, Wireshark와 F12 Network를 비교해 실제 HTTP 요청 패킷을 확인하고 그 패킷을 함수화하는 것이다. 여기서 XPath, CSS selector, outerHTML은 화면 요소를 찾는 DOM 정보이고, 패킷은 `method`, `host`, `path`, `headers`, `body`, `response`가 있는 HTTP 요청/응답 정보다.

현재 코드는 이 요구사항을 아래처럼 반영한다.

- 화면 이동과 랜덤 선택처럼 UI 흐름이 필요한 작업은 `discussion_room.rs`, `browser_flow.rs`, `post_form.rs`에 함수로 분리했다.
- 글쓰기 등록과 댓글 등록처럼 실제 서버에 데이터를 보내는 작업은 `packet_client.rs`에서 Rust HTTP 요청 함수로 분리했다.
- 패킷과 함수의 대응 관계는 `docs/packet-function-map.md`와 `docs/naver-packet-map.md`에 따로 기록했다.

## 현재 코드에서 가장 중요한 변경점

가장 중요한 파일은 `src-tauri/src/naver_automation/packet_client.rs`다.

이 파일은 새로 추가된 Rust 패킷 클라이언트다. Chrome에서 쿠키를 가져오는 것 외에는 브라우저 페이지 안의 JavaScript에 등록 요청을 맡기지 않는다. 실제 글쓰기와 댓글 생성 요청은 `reqwest`가 수행한다.

새로 추가된 의존성은 `src-tauri/Cargo.toml`에 있다.

```toml
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }
```

`blocking`을 사용한 이유는 현재 CLI 흐름이 동기 실행 구조이기 때문이다. `json`은 글쓰기 JSON 요청에 필요하고, `rustls-tls`는 WSL Ubuntu 환경에서 별도 OpenSSL 설정 문제를 줄이기 위해 사용했다.

## Chrome DevTools를 아직 사용하는 이유

Chrome DevTools를 완전히 제거하지 않은 이유는 로그인 때문이다.

네이버 로그인은 2차 인증, 세션 쿠키, 브라우저 보안 정책이 함께 걸려 있다. 이 프로젝트는 ID/PW를 코드에 저장하거나 직접 로그인 API를 호출하지 않는다. 대신 사용자가 Chrome에서 직접 로그인한다. Rust는 로그인 완료 후 Chrome이 가진 쿠키를 읽어서 요청에 사용한다.

현재 Chrome DevTools를 사용하는 부분은 아래와 같다.

- Chrome 탭 연결
- 현재 URL 확인
- 패킷 함수가 선택한 토론방/게시글 URL로 화면 이동
- 네이버 로그인 쿠키 읽기
- 요청 성공 후 화면 새로고침

반대로 글쓰기 등록과 댓글 등록의 서버 요청은 Rust `reqwest`가 직접 수행한다.

## 로그인 쿠키를 읽는 함수

파일: `src-tauri/src/naver_automation/packet_client.rs`

함수: `CdpClient::build_naver_packet_client`

이 함수는 Chrome DevTools Protocol의 `Network.getCookies`를 호출한다. 대상 URL은 다음 네이버 도메인이다.

- `https://stock.naver.com`
- `https://m.stock.naver.com`
- `https://apis.naver.com`
- `https://static.nid.naver.com`

그 다음 `naver.com` 또는 `pstatic.net` 도메인의 쿠키를 모아서 HTTP `Cookie` 헤더 문자열로 만든다.

중요한 체크는 `NID_AUT`, `NID_SES`가 있는지 확인하는 부분이다. 이 두 쿠키는 로그인 세션 유지에 핵심인 값이다. 둘 중 하나라도 없으면 로그인되지 않은 상태로 보고 실행을 중단한다.

이 함수는 마지막에 `reqwest::blocking::Client`를 생성해서 `NaverPacketClient`로 반환한다.

## 글쓰기 등록 패킷 흐름

글쓰기 등록은 한 번의 요청으로 끝나지 않는다. 캡처와 실행 오류를 통해 `txId`가 필요하다는 점을 확인했다.

처음에는 `/front-api/discussion/add`에 임의의 `txId`를 넣어 요청했는데, 서버가 아래 오류를 반환했다.

```text
TX_ID_MISMATCH
The provided txId does not match the stored txId.
```

이 오류는 `txId`를 직접 만들면 안 되고, 먼저 서버가 발급한 `txId`를 받아야 한다는 뜻이다.

그래서 현재 글쓰기 등록은 두 단계다.

### 1단계. 글쓰기 form 패킷으로 txId 발급

함수: `NaverPacketClient::issue_post_tx_id`

요청:

```text
POST https://m.stock.naver.com/front-api/discussion/form?discussionType=<토론타입>&itemCode=<종목코드>
```

응답에서 읽는 값:

```text
result.txId
```

### 2단계. 글쓰기 add 패킷으로 실제 글 등록

함수: `NaverPacketClient::submit_post`

요청:

```text
POST https://m.stock.naver.com/front-api/discussion/add
Content-Type: application/json
```

요청 본문 주요 값:

- `title`: 사용자가 PowerShell/CLI에 입력한 제목
- `contentJson`: 네이버 스마트에디터 형식의 본문 JSON
- `discussionType`: 현재 URL에서 계산한 토론 타입
- `itemCode`: 현재 URL에서 추출한 종목 코드
- `txId`: 1단계 form 패킷에서 받은 `result.txId`
- `inflow`: `NFS-P-P`

성공 판단:

- 응답 JSON의 `isSuccess`가 `true`인지 확인한다.
- 성공하면 `result.id`를 글 ID로 읽는다.

이 구조 때문에 더 이상 `TX_ID_MISMATCH`를 피하기 위해 임의로 값을 맞추는 방식이 아니다. 서버가 발급한 `txId`를 받아 그대로 사용한다.

## 댓글 등록 패킷 흐름

댓글 등록도 한 번의 요청으로 끝나지 않는다. 먼저 댓글용 cbox token을 받아야 한다.

현재 댓글 등록은 두 단계다.

### 1단계. cbox token 발급

함수: `NaverPacketClient::issue_cbox_token`

요청:

```text
GET https://apis.naver.com/commentBox/cbox/web_naver_token_json.json?ticket=finance&templateId=community&pool=cbox12...
```

요청에 포함되는 주요 값:

- `objectId`: 현재 토론글 URL에서 추출한 게시글 ID
- `objectUrl`: 현재 토론글 URL
- `ticket`: `finance`
- `templateId`: `community`
- `pool`: `cbox12`

응답에서 읽는 값:

```text
result.cbox_token
```

### 2단계. 댓글 생성

함수: `NaverPacketClient::submit_comment`

요청:

```text
POST https://apis.naver.com/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=
Content-Type: application/x-www-form-urlencoded
```

요청 본문 주요 값:

- `objectId`: 현재 게시글 ID
- `objectUrl`: 현재 게시글 URL
- `contents`: 사용자가 CLI에 입력한 댓글 내용
- `commentType`: `txt`
- `validateBanWords`: `true`
- `cbox_token`: 1단계에서 받은 토큰

성공 판단:

- 응답 JSON의 `success`가 `true`인지 확인한다.
- 또는 `result.comment`, `result.commentList`가 있으면 생성 성공으로 판단한다.

## 글쓰기/댓글쓰기 CLI 흐름

파일: `src-tauri/src/bin/naver_discussion_cli.rs`

CLI를 실행하면 먼저 작업을 선택한다.

```text
작업 선택:
1. 글쓰기
2. 댓글쓰기
번호 입력:
```

`1`을 선택하면 제목과 본문을 입력받는다.

```text
제목 입력:
내용 입력:
여러 줄 입력 가능. 마지막 줄에 END 입력 후 Enter를 누르면 실행합니다.
```

`2`를 선택하면 댓글 본문만 입력받는다.

```text
댓글 내용 입력:
여러 줄 입력 가능. 마지막 줄에 END 입력 후 Enter를 누르면 실행합니다.
```

PowerShell과 WSL 사이에서 한글 입력이 깨지는 문제가 있어서 입력은 손실 허용 방식으로 읽도록 수정했다. 즉, 입력 스트림에 완전한 UTF-8이 아닌 바이트가 섞여도 프로그램이 바로 죽지 않도록 했다.

## 함수 분리 구조

현재 자동화는 한 파일에 모든 로직을 넣지 않고 기능별로 나누었다.

- `src-tauri/src/naver_automation.rs`
  - 전체 자동화 흐름을 조립한다.
  - 글쓰기와 댓글쓰기 분기를 결정한다.

- `src-tauri/src/naver_automation/devtools_connection.rs`
  - Chrome DevTools WebSocket 연결을 담당한다.

- `src-tauri/src/naver_automation/browser_flow.rs`
  - 페이지 이동, 새로고침, 공통 브라우저 조작을 담당한다.

- `src-tauri/src/naver_automation/discussion_room.rs`
  - 패킷 함수가 반환한 랜덤 종목/게시글 URL로 Chrome 화면을 이동시킨다.

- `src-tauri/src/naver_automation/post_form.rs`
  - 글쓰기 모달 열기, 프로필 설정, 글쓰기/댓글 등록 실행 흐름을 담당한다.
  - 등록 자체는 `packet_client.rs`에 위임한다.

- `src-tauri/src/naver_automation/packet_client.rs`
  - Chrome 쿠키를 읽어 Rust HTTP 클라이언트를 만든다.
  - 로그인 확인 getProfile, 랜덤 종목 목록, 랜덤 게시글 목록, 프로필 2222 설정, 글쓰기 form/add, 댓글 token/create 패킷을 직접 호출한다.

- `src-tauri/src/naver_automation/types.rs`
  - CLI와 Tauri에서 같이 쓰는 요청/응답 타입을 정의한다.

## 패킷 기반으로 추가 전환한 부분

추가 캡처 `naver_random_capture.pcapng`, `naver_capture_success.pcapng`와 `keylogfile.txt`를 사용해 아래 작업도 Rust 패킷 함수로 옮겼다.

- 로그인 확인: `static.nid.naver.com/getProfile`
- 랜덤 종목 선택: 토론급상승/상승/하락/거래량 API
- 랜덤 게시글 선택: `posts/by-item` API
- 프로필 소개 `2222` 설정: `status`, `form`, `introduction/validate`, `PUT users/<profileId>`

자동 제출 모드에서 Chrome/DOM이 맡는 역할은 아래로 제한된다.

- 사용자가 직접 로그인과 2차 인증을 끝낸 세션 제공
- Chrome DevTools로 로그인 쿠키 읽기
- Rust가 패킷 응답으로 선택한 URL로 화면 이동
- 등록 후 새로고침해서 결과 확인
- 수동 확인 모드에서 입력란 채우기와 등록 버튼 강조

## 사수에게 설명할 때 중요한 포인트

현재 구현은 "모든 브라우저 조작을 없앤 것"이 아니다. 브라우저는 로그인 세션과 화면 이동을 위해 계속 사용한다.

하지만 사수 피드백의 핵심이었던 "글쓰기/댓글 등록을 프론트에 넘기지 말고 Rust가 직접 요청하게 하라"는 부분은 반영했다. 실제 등록 요청은 Rust `reqwest`가 네이버 API로 직접 보낸다.

따라서 설명할 때는 아래처럼 말하면 된다.

```text
초기 구현은 Chrome DevTools로 페이지 안 fetch를 실행하는 방식이라 실제 HTTP 요청 주체가 브라우저였습니다.
피드백 반영 후에는 Chrome DevTools를 로그인 쿠키 추출과 화면 이동에만 쓰고, 글쓰기/댓글 등록 패킷은 Rust reqwest가 직접 보내도록 바꿨습니다.
추가 캡처 반영 후 getProfile 로그인 확인, 랜덤 종목 선택, 랜덤 게시글 선택, 프로필 소개 2222 설정도 Rust reqwest 패킷 함수로 옮겼습니다.
글쓰기는 form 패킷으로 txId를 발급받고 add 패킷에 넣습니다.
댓글은 cbox token 패킷을 먼저 호출하고 create 패킷에 token과 contents를 넣습니다.
```

## 현재 상태에서 주의할 점

이 자동화는 로그인된 네이버 세션 쿠키를 사용한다. 코드에 비밀번호를 저장하지는 않지만, 실행 중인 Chrome의 로그인 쿠키는 민감한 값이다.

Chrome은 `--remote-debugging-port` 옵션으로 실행되기 때문에 같은 PC의 로컬 프로세스가 브라우저를 조작할 수 있다. 실행 중에는 민감한 다른 사이트를 같은 Chrome 프로필에서 열지 않는 편이 좋다.

또한 네이버 서비스 정책이나 이용 제한 가능성은 별도로 확인해야 한다. 이 문서는 기술 구현 기록이지 서비스 정책 준수 보장을 의미하지 않는다.

## 실행 확인 방법

PowerShell에서 UTF-8을 설정한다.

```powershell
chcp 65001
[Console]::InputEncoding = [System.Text.UTF8Encoding]::new()
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()
$OutputEncoding = [System.Text.UTF8Encoding]::new()
```

Chrome을 디버깅 포트로 실행한다.

```powershell
taskkill /F /IM chrome.exe
& "C:\Program Files\Google\Chrome\Application\chrome.exe" --remote-debugging-port=9222 --user-data-dir="$env:TEMP\pstmacro-chrome-debug"
```

Chrome이 뜨면 네이버 로그인을 완료한다.

PowerShell에서 Chrome 포트를 확인한다.

```powershell
Invoke-WebRequest http://127.0.0.1:9222/json/version -UseBasicParsing
```

WSL에서 Windows Chrome 디버깅 포트가 보이는지 확인한다.

```powershell
wsl -d Ubuntu -- bash -lc "curl -s http://172.24.32.1:9223/json/version | head"
```

프로그램을 실행한다.

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro/src-tauri && cargo run --bin naver_discussion_cli -- --host 172.24.32.1 --port 9223'
```

현재 CLI는 성공하면 아래와 같은 정보를 출력한다.

```text
완료
로그인 확인: <닉네임>
선택 카테고리: ...
선택 순위: ...
선택 종목: ...
입력 대상: 글쓰기 또는 댓글
등록 실행: 완료
현재 URL: ...
```

## 이번 변경을 GitHub에 올릴 때 커밋 메시지 예시

```text
Rust reqwest 기반 패킷 요청으로 전환
```

PR 설명에는 아래 내용을 포함하면 된다.

```text
프론트 fetch 실행 방식이 아니라 Rust reqwest에서 네이버 글쓰기/댓글 등록 패킷을 직접 보내도록 수정했습니다.
Chrome DevTools는 로그인 세션 쿠키 추출과 화면 이동에만 사용합니다.
글쓰기는 form 패킷으로 txId를 받은 뒤 add 패킷을 호출하고, 댓글은 cbox token 패킷을 받은 뒤 create 패킷을 호출합니다.
```
