# 0002. 패킷 기반 네이버 증권 토론 자동화 구조

날짜: 2026-05-27

## 상태

Accepted

## 배경

이 프로젝트는 기존 Python 자동화 코드를 Rust/Tauri/pnpm 환경으로 옮기면서, 네이버 증권 토론방에서 아래 작업을 수행해야 한다.

- 네이버 로그인 상태 확인
- 네이버 증권 토론방 이동
- 랜덤 토론방 선택
- 글쓰기 또는 댓글쓰기 선택
- 글쓰기인 경우 제목/본문 등록
- 댓글쓰기인 경우 랜덤 게시글 선택 후 댓글 등록

사수 요구사항은 단순히 DOM selector나 XPath만 보는 방식이 아니라, Chrome F12 Network와 Wireshark TLS 복호화 결과를 비교해서 실제 요청 패킷을 확인하고 그 패킷을 함수로 분리하는 것이다.

여기서 `Ctrl+Shift+C`, XPath, CSS selector는 화면 요소를 찾는 DOM 정보이고, 패킷은 HTTP 요청/응답 정보다. 패킷에는 method, authority, path, content-type, request body, response body가 포함된다.

## 확인한 패킷

### 로그인 상태 확인

- Method: `GET`
- Authority: `static.nid.naver.com`
- Path: `/getProfile?svc=my&callback=...`
- Response Type: `application/x-javascript`
- Response Body: `rtn_cd`, `rtn_msg`, `nick_name`, `image_url`

이 패킷은 로그인된 Chrome 세션에서 현재 사용자가 로그인되어 있는지 확인하는 데 사용한다.

### 글쓰기 등록

- Method: `POST`
- Authority: `m.stock.naver.com`
- Path: `/front-api/discussion/add`
- Content-Type: `application/json`
- Request Body 주요 값:
  - `title`
  - `contentJson`
  - `discussionType`
  - `itemCode`
  - `txId`
  - `inflow`
- Response Body 주요 값:
  - `isSuccess: true`
  - `result.id`

이 패킷은 네이버 증권 토론방에 새 글을 등록하는 데 사용한다.

### 댓글 등록

댓글 등록은 두 단계로 구성된다.

1. cbox token 발급
   - Method: `GET`
   - Authority: `apis.naver.com`
   - Path: `/commentBox/cbox/web_naver_token_json.json?...`
   - Response Body 주요 값: `result.cbox_token`

2. 댓글 생성
   - Method: `POST`
   - Authority: `apis.naver.com`
   - Path: `/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=`
   - Content-Type: `application/x-www-form-urlencoded`
   - Request Body 주요 값:
     - `objectId`
     - `objectUrl`
     - `contents`
     - `cbox_token`
     - `commentType: txt`

이 패킷은 특정 토론글에 댓글을 등록하는 데 사용한다.

## 결정

자동화 코드는 기능별로 파일과 함수를 분리한다.

- `naver_automation.rs`: 전체 자동화 흐름 실행
- `devtools_connection.rs`: Chrome DevTools 연결
- `browser_flow.rs`: 공통 브라우저 이동/약관/페이지 준비
- `packet_profile.rs`: getProfile 패킷 기반 로그인 확인
- `discussion_room.rs`: 토론방 이동 및 랜덤 종목/게시글 선택
- `post_form.rs`: 프로필 설정, 글쓰기 등록, 댓글 등록
- `types.rs`: 요청/응답 구조체
- `bin/naver_discussion_cli.rs`: PowerShell/WSL에서 실행하는 CLI

등록 기능은 가능한 범위에서 패킷 기반 함수로 구현한다.

- `submit_post_and_refresh`: `POST /front-api/discussion/add` 패킷 구조를 재현한다.
- `submit_comment_and_refresh`: cbox token 발급 후 `POST /web_naver_create_json.json` 패킷 구조를 재현한다.
- `read_login_profile_from_packet`: `GET /getProfile` 패킷 구조를 재현한다.

네이버 쿠키 값은 코드에 저장하지 않는다. 함수는 로그인된 Chrome 탭 안에서 `fetch`를 실행하므로, Chrome이 가진 현재 세션 쿠키를 그대로 사용한다.

## 현재 DOM 기반으로 남겨둔 부분

아래 작업은 현재 패킷이 충분히 확보되지 않았거나 화면 흐름 자체가 필요한 작업이라 DOM 기반으로 유지한다.

- 네이버 증권 토론 메인 이동
- 오늘의 종목 토론 둘러보기 영역에서 랜덤 종목 선택
- 전체 토론글 보러가기 클릭
- 글쓰기 모달 열기
- 댓글 작성을 위한 랜덤 게시글 열기
- 프로필이 없을 때 글쓰기 > 설정하기 > 소개 `2222` > 완료 처리

특히 프로필 소개 `2222` 저장 패킷은 현재 제공된 캡처에서 명확히 확인되지 않았다. 따라서 현재는 UI 자동화로 처리하고, 나중에 해당 저장 요청의 POST/PUT/PATCH 패킷을 확보하면 별도 함수로 교체할 수 있다.

## 결과

이 구조는 사수 요구사항인 "패킷 분석 후 함수화"를 코드에 반영한다. 동시에 모든 작업을 한 파일에 몰아넣지 않고, 기능별 Rust 모듈과 함수로 분리해 유지보수성을 확보한다.

CLI는 빠른 검증을 위해 `submit_after_fill=true`로 실행되어 실제 등록까지 수행한다. Tauri UI에서는 필요하면 수동 확인 모드를 유지할 수 있도록 구조를 남겨두었다.
