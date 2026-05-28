# ABD. CSV 기반 네이버 증권 토론 batch UI 작업 정리

날짜: 2026-05-28

## 목적

기존 PowerShell CLI 입력 방식은 빠르게 검증하기에는 좋지만, 실제 사용자가 여러 제목, 여러 본문, 여러 댓글을 한 번에 가져와서 실행하기 어렵다.

이번 작업의 목적은 다음과 같다.

- CSV 파일에서 제목, 내용, 댓글내용을 가져온다.
- 사용자가 화면에서 종목을 선택한다.
- 사용자가 글쓰기 또는 댓글쓰기를 체크박스로 선택한다.
- 랜덤, 순차, 1개만 모드로 제목/내용/댓글내용을 선택한다.
- 3개 또는 5개 실행 개수를 고른다.
- 실제 등록은 Rust 패킷 기반 함수가 수행한다.
- 기존 CLI와 패킷 기반 등록 함수는 제거하지 않는다.

## 구현 파일

### Rust batch 실행 파일

```text
src-tauri/src/discussion_batch.rs
```

역할:

- CSV 파싱
- 제목, 내용, 댓글내용 헤더 처리
- 2행부터 실제 데이터 추출
- 빈 값 제거
- 네이버 증권 종목 API 조회
- 종목명과 종목코드 분리
- 랜덤, 순차, 1개만 선택 모드 검증
- 3개 또는 5개 실행 개수 검증
- 글쓰기/댓글쓰기 반복 실행
- 등록 후 다음 등록까지 1분 대기

### Tauri command 연결

```text
src-tauri/src/lib.rs
```

추가한 command:

- `parse_template_csv`
- `search_stocks`
- `run_naver_discussion_batch`

프론트 화면은 이 command들을 호출한다. 실제 CSV 파싱과 실행은 Rust에서 한다.

### 새 batch UI

```text
src/features/macro-editor/stock-batch-panel.tsx
```

역할:

- CSV 파일 가져오기 버튼
- 종목 검색 입력창
- 종목 목록 열기 버튼
- 종목 radio 선택 목록
- 선택된 종목 chip 표시
- 종목명, 종목코드, 링크 표시
- 글쓰기/댓글쓰기 체크박스
- 제목, 내용, 댓글내용 텍스트창
- 제목, 내용, 댓글내용 목록형 선택/수정 UI
- 랜덤, 순차, 1개만 선택 UI
- 3개, 5개 실행 개수 선택 UI
- 시크릿 Chrome 열기 버튼
- Chrome DevTools host/port 내부 자동 설정
- 설정 저장 버튼
- 실행 버튼

### 메인 화면 연결

```text
src/features/macro-editor/macro-editor-page.tsx
```

기존 아래쪽 저장/선택 패널은 새 batch UI와 혼동되어 화면에서 제거했다.

중요한 점:

- 관련 기존 컴포넌트 파일은 삭제하지 않았다.
- 현재 첫 화면에서는 새 batch UI만 보이게 했다.
- 기존 기능을 되돌려야 하면 컴포넌트를 다시 렌더링하면 된다.

### 스타일

```text
src/features/macro-editor/macro-editor.css
```

추가한 스타일:

- batch UI 박스
- 종목 검색 영역
- 종목 목록 grid
- 선택 종목 chip
- 종목 상세 영역
- 행동 미선택 빨간 테두리
- 제목/내용/댓글 텍스트창 grid
- 저장/실행 버튼 row

## 종목명이 숫자로만 보이던 문제

문제:

화면에 종목명이 `005930`, 종목코드도 `005930`처럼 보였다.

원인:

네이버 증권 API 응답 필드가 실제로는 아래처럼 소문자였다.

```json
{
  "itemname": "흥아해운",
  "itemcode": "003280"
}
```

기존 코드는 `itemName`, `itemCode` 같은 camelCase만 찾고 있었다.

수정:

`itemname`, `itemcode`, `stockname`, `stockcode` 같은 소문자 필드도 읽도록 수정했다.

결과:

종목 목록은 아래처럼 보여야 한다.

```text
삼성전자
005930
```

선택된 종목 chip은 아래처럼 보여야 한다.

```text
삼성전자 · 005930
```

## 제목 선택 방식

이전 화면에서는 아래쪽에 기존 제목 선택 패널이 같이 보여서 헷갈렸다.

이번 수정 후에는 텍스트창 기준으로만 사용한다.

CSV에서 제목이 여러 개 들어오면 텍스트창에는 아래처럼 보인다.

```text
이게되네 ㅋㅋㅋ
---
뭣
```

`---`는 여러 제목을 구분하는 구분선이다.

선택 방식은 텍스트창 아래 체크박스에서 고른다.

- 랜덤: 여러 제목 중 하나를 실행 시점에 고른다.
- 순차: 첫 번째 실행은 첫 번째 제목, 두 번째 실행은 두 번째 제목을 사용한다.
- 1개만: 제목이 정확히 하나일 때만 사용할 수 있다.

내용과 댓글내용도 같은 방식이다.

## WSLg 리눅스 창에서 한글 입력이 안 되는 문제

현재 `pnpm tauri dev`는 WSL Ubuntu 안에서 Tauri 앱을 실행한다.

그래서 뜨는 창은 Windows Chrome 창이 아니라 WSLg 리눅스 GUI 창이다.

이 환경에서는 Windows 한글 IME가 그대로 붙지 않아 한글 입력이 안 되거나 영어만 입력될 수 있다.

이번 UI는 이 문제를 줄이기 위해 아래 방식으로 사용할 수 있다.

- 종목명 검색을 꼭 한글로 입력하지 않아도 된다.
- 종목 목록 열기 버튼을 눌러 마우스로 선택할 수 있다.
- 종목코드 숫자로도 검색할 수 있다.
- 제목, 내용, 댓글내용은 CSV UTF-8 파일에서 가져오면 직접 한글 타이핑이 줄어든다.

한글 직접 입력까지 완전히 해결하려면 WSLg 한글 IME 설정이 별도로 필요하다.

## 목록형 제목/내용/댓글 UI로 변경

기존 화면은 CSV 항목 여러 개를 아래처럼 하나의 텍스트창에 `---` 구분선으로 보여줬다.

```text
이게되네 ㅋㅋㅋ
---
뭣
```

이 방식은 개발자는 이해할 수 있지만 일반 사용자는 `---`가 무엇인지 헷갈릴 수 있다.

이번 수정에서는 `---` 구분선을 사용자에게 보여주지 않는다.

CSV에서 가져온 제목, 내용, 댓글내용은 각각 파일 목록처럼 표시한다.

예를 들어 CSV 제목 열에 아래 두 값이 있으면:

```text
이게되네 ㅋㅋㅋ
뭣
```

화면의 제목 목록에는 아래처럼 두 줄로 보인다.

```text
1  이게되네 ㅋㅋㅋ
2  뭣
```

목록 UI의 규칙은 다음과 같다.

- 클릭하면 해당 항목을 선택하거나 선택 해제한다.
- 선택된 항목만 랜덤, 순차, 1개만 실행 대상이 된다.
- 더블클릭하면 해당 항목을 수정할 수 있다.
- 수정 후 목록 안의 `저장`을 누르면 값이 바뀐다.
- 한 번에 보이는 항목은 최대 5개 높이로 제한한다.
- 항목이 많으면 목록 안에서 스크롤해서 본다.

## 글쓰기와 댓글쓰기 화면 분리

기존 화면은 글쓰기와 댓글쓰기 여부와 상관없이 제목, 내용, 댓글내용이 항상 한 화면에 같이 보였다.

이 방식은 “댓글은 어떤 글에 달리는가”를 이해하기 어렵게 만들었다.

이번 수정에서는 체크한 작업에 따라 필요한 영역만 보이게 했다.

### 글쓰기만 선택한 경우

아래 영역만 보인다.

```text
글쓰기 설정
- 제목 목록
- 내용 목록
- 제목 선택 방식: 랜덤 / 순차 / 1개만
- 내용 선택 방식: 랜덤 / 순차 / 1개만
- 작성 개수: 3개 / 5개
```

### 댓글쓰기만 선택한 경우

아래 영역만 보인다.

```text
댓글쓰기 설정
- 댓글 내용 목록
- 댓글 선택 방식: 랜덤 / 순차 / 1개만
- 작성 개수: 3개 / 5개
```

댓글쓰기만 실행할 때는 Rust가 선택한 종목의 토론글 목록 API를 호출해서 댓글을 달 게시글을 고른다.

### 글쓰기와 댓글쓰기를 같이 선택한 경우

아래 두 영역이 같이 보인다.

```text
글쓰기 설정
댓글쓰기 설정
```

실행 순서는 Rust에서 아래처럼 처리한다.

```text
1. 선택한 제목/내용으로 글쓰기 패킷 전송
2. 글쓰기 add 응답에서 작성 글 ID 확인
3. 작성 글 ID로 방금 작성한 글 URL 생성
4. 그 글 URL을 기준으로 댓글 토큰 패킷 요청
5. 댓글 생성 패킷 전송
6. 새로고침
```

즉 글쓰기와 댓글쓰기를 같이 선택하면 랜덤 게시글에 댓글을 다는 것이 아니라, 방금 작성한 글에 댓글을 단다.

이 로직은 프론트 DOM 클릭으로 처리하지 않고 Rust 패킷 함수에서 처리한다.

## Chrome DevTools 관련 경고

개발 중 WSL에서 `pnpm tauri dev`를 실행하면 아래 경고가 보일 수 있다.

```text
libEGL warning
MESA: error: ZINK: failed to choose pdev
```

이 메시지는 WSLg 그래픽 드라이버 경고다.

Tauri 창이 뜨고 UI가 동작한다면 치명적인 오류는 아니다.

Windows 배포판 사용자는 WSL을 사용하지 않으므로 이 경고를 볼 가능성이 낮다.

## 시크릿 Chrome 실행

배포하거나 다른 사람이 실행할 때는 일반 Chrome 대신 시크릿 Chrome 전용 디버깅 세션을 사용한다.

이번 수정 후 일반 사용자 화면에는 아래 설정을 표시하지 않는다.

```text
Chrome DevTools host
Chrome DevTools 포트
Windows
WSL
```

이 값들은 개발자가 알면 되는 값이고, 일반 사용자가 직접 입력할 필요가 없다.

추가한 실행 파일:

```text
scripts/start-chrome-incognito-debug.bat
```

이 배치 파일은 아래 옵션으로 Chrome을 실행한다.

```text
--remote-debugging-port=9222
--user-data-dir=%TEMP%\pstmacro-chrome-incognito-debug
--incognito
--disable-quic
--no-first-run
--no-default-browser-check
```

일반 Chrome 프로필과 자동화용 Chrome 프로필을 분리하기 위해 `--user-data-dir`을 별도로 둔다. 사용자는 이 시크릿 창에서 네이버 로그인을 완료한 뒤 프로그램을 실행한다.

또한 Tauri 앱 안에 `시크릿 Chrome 열기` 버튼을 추가했다.

이 버튼은 Windows 배포판에서 아래 옵션으로 Chrome을 연다.

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

Windows 배포 앱에서 내부 기본 DevTools 값은 아래를 사용한다.

```text
host: 127.0.0.1
port: 9222
```

`127.0.0.1`은 사용자 본인 컴퓨터를 뜻한다.

즉 다른 사람 컴퓨터에 배포해도 그 사람의 컴퓨터 안에서 열린 시크릿 Chrome에 붙는다.

그래서 배포받은 사람마다 IP를 따로 알 필요가 없다.

`9222`는 Chrome이 자동화 명령을 받을 수 있게 여는 로컬 포트다.

프로그램이 같은 번호로 Chrome을 열고 같은 번호로 붙기 때문에 일반 사용자가 포트를 고를 필요가 없다.

WSL 개발 환경에서만 Windows Chrome에 연결해야 할 때는 내부적으로 아래 값을 사용한다.

```text
host: 172.24.32.1
port: 9223
```

이 값은 개발환경용 예외이므로 배포 UI에서는 숨겼다.

## 배포 시 남는 위험

아래 문제는 코드만으로 완전히 없앨 수 없다.

- 네이버 정책 또는 계정 제한 위험
- 네이버 API 응답 구조 변경
- 사용자가 로그인과 2차 인증을 완료하지 않은 상태
- Chrome 원격 디버깅 포트 노출
- WSLg 한글 입력기 문제
- Windows가 아닌 환경에서 Chrome 경로가 다른 문제

특히 원격 디버깅 포트는 로그인 쿠키를 읽을 수 있으므로 실행 중에는 자동화용 시크릿 Chrome에서 네이버만 사용하는 편이 안전하다.

## 테스트 결과

아래 검증을 통과했다.

```text
cargo check
cargo test discussion_batch
pnpm test -- --run
pnpm build
```

테스트에서 확인한 내용:

- CSV 헤더 제외
- CSV quoted field 파싱
- 1개만 모드 검증
- 네이버 API 소문자 필드 `itemname`, `itemcode`에서 종목명/종목코드 추출
- 새 batch UI 렌더링
- 기존 혼동 패널 미표시
- CSV 가져오기 상태 표시

## GitHub에 올릴 때 포함할 파일

```text
src-tauri/Cargo.toml
src-tauri/src/discussion_batch.rs
src-tauri/src/lib.rs
src-tauri/src/naver_automation.rs
src-tauri/src/naver_automation/discussion_room.rs
src-tauri/src/naver_automation/types.rs
src-tauri/src/bin/naver_discussion_cli.rs
src/features/macro-editor/stock-batch-panel.tsx
src/features/macro-editor/macro-editor-page.tsx
src/features/macro-editor/macro-editor-page.test.tsx
src/features/macro-editor/macro-editor.css
docs/adr/0003-discussion-batch-ui-and-rust-executor.md
docs/discussion-batch-ui-guide.md
docs/ABD-discussion-batch-ui-summary.md
```
