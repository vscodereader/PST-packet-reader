# GitHub 브랜치 생성 및 PR 작성 가이드

이 문서는 `beyondsoft-kr` GitHub 조직에 작업 내용을 올리고 Pull Request를 생성하는 절차를 정리한다.

## 1. GitHub에서 저장소 확인

브라우저에서 아래 주소로 이동한다.

[https://github.com/orgs/beyondsoft-kr/repositories](https://github.com/orgs/beyondsoft-kr/repositories)

저장소 목록에서 `pstmacro`를 찾는다.

## 2. 로컬에서 현재 저장소 위치로 이동

PowerShell에서 아래 명령을 실행한다.

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && pwd && git status'
```

`/home/csw/projects/pstmacro`가 나오면 맞는 위치다.

## 3. 최신 내용 받기

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git pull'
```

인증 문제가 생기면 GitHub 계정 권한 또는 토큰 설정이 필요하다.

## 4. 작업 브랜치 생성

브랜치 이름 예시:

```text
codex/packet-mapped-naver-discussion
```

명령:

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git checkout -b codex/packet-mapped-naver-discussion'
```

이미 같은 이름의 브랜치가 있다고 나오면 아래 명령으로 이동한다.

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git checkout codex/packet-mapped-naver-discussion'
```

## 5. 변경 파일 확인

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git status --short'
```

이번 작업에서 핵심 파일은 아래와 같다.

```text
src-tauri/src/naver_automation.rs
src-tauri/src/naver_automation/
src-tauri/src/bin/naver_discussion_cli.rs
docs/adr/0002-packet-mapped-naver-automation.md
docs/naver-discussion-automation-guide.md
docs/naver-packet-map.md
docs/packet-function-map.md
docs/github-pr-guide.md
```

## 6. 빌드 확인

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro/src-tauri && cargo fmt --all && cargo check'
```

성공 기준:

```text
Finished `dev` profile ...
```

## 7. 커밋 생성

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git add src-tauri/src docs README.md package.json pnpm-lock.yaml src/app src/test src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json && git commit -m "네이버 증권 토론 자동화 패킷 기반 함수화"'
```

이미 일부 파일이 다른 작업과 섞여 있으면 `git status --short`로 확인한 뒤 필요한 파일만 `git add` 해야 한다.

## 8. GitHub에 브랜치 push

```powershell
wsl -d Ubuntu -- bash -lc 'cd ~/projects/pstmacro && git push -u origin codex/packet-mapped-naver-discussion'
```

## 9. GitHub에서 PR 생성

브라우저에서 `pstmacro` 저장소로 이동한다.

1. 상단에 `Compare & pull request` 버튼이 보이면 클릭한다.
2. 버튼이 안 보이면 `Pull requests` 탭을 누른다.
3. `New pull request` 버튼을 누른다.
4. `compare` 브랜치에 `codex/packet-mapped-naver-discussion`을 선택한다.
5. `base` 브랜치는 팀에서 사용하는 기본 브랜치를 선택한다. 보통 `main` 또는 `develop`이다.
6. 제목과 내용을 아래 템플릿으로 입력한다.

## PR 제목

```text
네이버 증권 토론 자동화 패킷 기반 함수화
```

## PR 내용

````markdown
## 작업 배경

네이버 증권 토론 자동화 기능을 기존 Python 기반 흐름이 아니라 현재 프로젝트의 Rust/Tauri/pnpm 환경에 맞춰 구현했습니다.
또한 사수 요청에 따라 Chrome F12 Network와 Wireshark TLS 복호화 HTTP/2 내용을 기준으로 실제 요청 패킷을 확인하고, 확인된 패킷을 함수 단위로 분리했습니다.

## 주요 변경 사항

- 네이버 로그인 상태 확인을 `GET /getProfile?svc=my&callback=...` 패킷 기반 함수로 구현했습니다.
- 글쓰기 등록을 `POST /front-api/discussion/form`으로 `txId`를 받은 뒤 `POST https://m.stock.naver.com/front-api/discussion/add`를 호출하는 패킷 기반 함수로 구현했습니다.
- 댓글 등록을 cbox token 발급 요청과 `POST /commentBox/cbox/web_naver_create_json.json` 패킷 기반 함수로 구현했습니다.
- 실제 글쓰기/댓글 HTTP 요청은 Chrome JavaScript `fetch`가 아니라 Rust `reqwest` 클라이언트가 직접 전송합니다.
- CLI에서 `1. 글쓰기`, `2. 댓글쓰기`를 선택할 수 있게 했습니다.
- 글쓰기 선택 시 제목/본문을 입력받고 등록까지 실행합니다.
- 댓글쓰기 선택 시 댓글 내용을 입력받고 랜덤 게시글을 열어 댓글 등록까지 실행합니다.
- 프로필 생성이 필요한 경우 `글쓰기 > 설정하기 > 소개 2222 > 완료` 흐름을 처리합니다.
- 기능별로 Rust 모듈과 함수를 분리했습니다.
- 함수 바로 위에 역할 설명 주석을 추가했습니다.
- 패킷 분석 내용과 ADR 문서를 추가했습니다.

## 패킷 근거

- 글쓰기 등록:
  - `POST https://m.stock.naver.com/front-api/discussion/form?discussionType=...&itemCode=...`
  - 응답에서 `result.txId` 확인
  - `POST https://m.stock.naver.com/front-api/discussion/add`
  - `content-type: application/json`
  - 요청 본문에 `title`, `contentJson`, `discussionType`, `itemCode`, `txId`, `inflow` 포함

- 댓글 등록:
  - `GET https://apis.naver.com/commentBox/cbox/web_naver_token_json.json?...`
  - 응답에서 `result.cbox_token` 확인
  - `POST https://apis.naver.com/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=`
  - 요청 본문에 `objectId`, `objectUrl`, `contents`, `cbox_token` 포함

## 문서

- `docs/adr/0002-packet-mapped-naver-automation.md`
- `docs/naver-discussion-automation-guide.md`
- `docs/naver-packet-map.md`
- `docs/packet-function-map.md`
- `docs/github-pr-guide.md`

## 검증

```bash
cd ~/projects/pstmacro/src-tauri
cargo fmt --all
cargo check
```
````

`cargo check` 성공을 확인했습니다.

## 참고 사항

추가 Wireshark 캡처를 반영해 getProfile 로그인 확인, 랜덤 종목 선택, 랜덤 게시글 선택, 프로필 소개 `2222` 설정도 Rust `reqwest` 패킷 함수로 전환했습니다.
Chrome DevTools는 로그인 세션 쿠키 추출, 화면 이동, 등록 후 새로고침에 사용합니다.

```

## 10. PR 생성 버튼 클릭

내용을 확인한 뒤 `Create pull request` 버튼을 누른다.

팀에서 바로 리뷰하지 말고 초안으로 보라고 했다면 `Create draft pull request`를 선택한다.
```
