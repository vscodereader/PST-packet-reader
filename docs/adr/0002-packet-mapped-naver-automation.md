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

글쓰기 등록은 두 단계로 구성된다.

1. 글쓰기 form 생성
   - Method: `POST`
   - Authority: `m.stock.naver.com`
   - Path: `/front-api/discussion/form?discussionType=...&itemCode=...`
   - Response Body 주요 값: `result.txId`

2. 글 생성
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

`/front-api/discussion/add`에 임의의 `txId`를 보내면 `TX_ID_MISMATCH` 오류가 발생한다. 따라서 먼저 `/front-api/discussion/form` 응답의 `result.txId`를 받은 뒤, 그 값을 `/front-api/discussion/add` 요청에 그대로 사용한다.

이 패킷은 네이버 증권 토론방에 새 글을 등록하는 데 사용한다.

### 랜덤 종목 선택

랜덤 종목 선택은 네이버 증권 토론 메인에서 실제로 호출되는 목록 API를 사용한다.

- 토론급상승
  - Method: `GET`
  - Authority: `stock.naver.com`
  - Path: `/api/community/discussion/rankings?nationType=KOR&page=1&size=10&postType=HOT`
- 상승
  - Method: `GET`
  - Authority: `stock.naver.com`
  - Path: `/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=10`
- 하락
  - Method: `GET`
  - Authority: `stock.naver.com`
  - Path: `/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=10`
- 거래량
  - Method: `GET`
  - Authority: `stock.naver.com`
  - Path: `/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=10`

이 패킷 응답에서 종목 코드와 종목명을 추출해 랜덤 종목 토론방 URL을 만든다.

### 랜덤 게시글 선택

댓글 작성 대상 게시글은 종목 토론글 목록 API를 사용한다.

- Method: `GET`
- Authority: `stock.naver.com`
- Path: `/api/community/discussion/posts/by-item?discussionType=...&itemCode=...&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=true&pageSize=10`

이 패킷 응답에서 게시글 ID를 추출해 랜덤 게시글 상세 URL을 만든다.

### 프로필 소개 2222 설정

프로필이 없는 계정은 댓글 작성 전에 프로필 소개 설정이 필요하다. 성공 캡처에서 아래 순서를 확인했다.

1. 프로필 상태 확인
   - Method: `GET`
   - Authority: `stock.naver.com`
   - Path: `/api/community/profile/users/status`
   - Response Body 주요 값: `profileId`, `status`

2. 프로필 form 조회
   - Method: `GET`
   - Authority: `stock.naver.com`
   - Path: `/api/community/profile/users/form`

3. 소개 검증
   - Method: `POST`
   - Authority: `stock.naver.com`
   - Path: `/api/community/profile/users/introduction/validate`
   - Request Body: `{ "targetValue": "2222" }`

4. 프로필 저장
   - Method: `PUT`
   - Authority: `stock.naver.com`
   - Path: `/api/community/profile/users/<profileId>`
   - Request Body 주요 값: `nickname`, `introduction: "2222"`, `imageUrl`, `danglingImages`

5. 프로필 상태 재확인
   - Method: `GET`
   - Authority: `stock.naver.com`
   - Path: `/api/community/profile/users/status`
   - Response Body 주요 값: `status: "existent"`

이 패킷은 프로필 소개를 `2222`로 설정하는 데 사용한다.

### 기존에 확인한 add 패킷

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
- `discussion_room.rs`: 패킷 API가 반환한 URL로 토론방/게시글 화면 이동
- `packet_client.rs`: Chrome 쿠키를 기반으로 Rust `reqwest`가 실제 네이버 HTTP 패킷 요청 수행
- `post_form.rs`: 프로필 설정, 글쓰기 등록 호출, 댓글 등록 호출
- `types.rs`: 요청/응답 구조체
- `bin/naver_discussion_cli.rs`: PowerShell/WSL에서 실행하는 CLI

등록 기능은 가능한 범위에서 패킷 기반 함수로 구현한다.

- `submit_post_and_refresh`: `POST /front-api/discussion/add` 패킷 구조를 재현한다.
- `submit_comment_and_refresh`: cbox token 발급 후 `POST /web_naver_create_json.json` 패킷 구조를 재현한다.
- `read_login_profile`: `GET /getProfile` 패킷 구조를 재현한다.
- `select_random_discussion_room`: 랭킹/시세 목록 패킷으로 랜덤 종목을 선택한다.
- `select_random_discussion_post`: 토론글 목록 패킷으로 랜덤 게시글을 선택한다.
- `ensure_profile_intro_setup`: 프로필 status/form/validate/PUT 패킷으로 소개 `2222`를 설정한다.

네이버 쿠키 값은 코드에 저장하지 않는다. Rust는 Chrome DevTools의 `Network.getCookies`로 현재 로그인 세션 쿠키를 읽고, 실제 HTTP 요청은 Rust `reqwest` 클라이언트가 직접 보낸다. 따라서 패킷 요청 주체는 브라우저 프론트 런타임이 아니라 Rust 백엔드다.

## 현재 DOM 기반으로 남겨둔 부분

자동 제출 경로에서 핵심 데이터 요청은 패킷 기반 함수로 전환했다. 아래 작업은 화면 표시와 사용자의 확인을 위한 브라우저 역할로 유지한다.

- 사용자가 직접 로그인과 2차 인증을 끝낸 Chrome 세션 제공
- DevTools로 로그인 쿠키 읽기
- 패킷 함수가 선택한 토론방/게시글 URL로 Chrome 화면 이동
- 글쓰기/댓글 등록 후 화면 새로고침
- `submit_after_fill=false` 수동 확인 모드에서 입력란 채우기와 등록 버튼 강조

## 결과

이 구조는 사수 요구사항인 "패킷 분석 후 함수화"를 코드에 반영한다. 동시에 모든 작업을 한 파일에 몰아넣지 않고, 기능별 Rust 모듈과 함수로 분리해 유지보수성을 확보한다.

CLI는 빠른 검증을 위해 `submit_after_fill=true`로 실행되어 실제 등록까지 수행한다. Tauri UI에서는 필요하면 수동 확인 모드를 유지할 수 있도록 구조를 남겨두었다.
