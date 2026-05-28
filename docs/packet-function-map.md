# Packet Function Map

이 문서는 Wireshark/F12 Network에서 확인한 요청을 코드 함수로 옮긴 위치를 정리합니다.

## 로그인 상태 확인

Wireshark/F12에서 확인한 패킷:

- `:method`: `GET`
- `:authority`: `static.nid.naver.com`
- `:scheme`: `https`
- `:path`: `/getProfile?svc=my&callback=<jsonp callback>`
- 응답 타입: `application/x-javascript`
- 응답 예: `{"rtn_cd":"0","rtn_msg":"Success","nick_name":"...","image_url":"..."}`

코드 위치:

- `src-tauri/src/naver_automation/packet_profile.rs`
- 함수: `CdpClient::read_login_profile_from_packet`

역할:

1. 로그인된 Chrome 탭 안에서 같은 JSONP 요청을 실행합니다.
2. 브라우저가 이미 가진 `NID_AUT`, `NID_SES` 같은 쿠키를 직접 노출하지 않고 사용합니다.
3. 응답의 `rtn_cd`, `rtn_msg`, `nick_name`, `image_url`을 `NaverLoginProfile` 구조체로 변환합니다.
4. 로그인 확인 후 기존 자동화 흐름을 계속 진행합니다.

## 글쓰기 등록

Wireshark/F12에서 확인한 패킷:

- txId 발급:
  - `:method`: `POST`
  - `:authority`: `m.stock.naver.com`
  - `:path`: `/front-api/discussion/form?discussionType=...&itemCode=...`
  - 응답 본문 핵심값: `result.txId`
- 글 등록:
  - `:method`: `POST`
  - `:authority`: `m.stock.naver.com`
  - `:path`: `/front-api/discussion/add`
  - `content-type`: `application/json`

`/front-api/discussion/add` 요청의 `txId`는 `/front-api/discussion/form` 응답의 `result.txId`와 같아야 한다.

글 등록 요청 본문 핵심값:

- `:method`: `POST`
- `:authority`: `m.stock.naver.com`
- `:path`: `/front-api/discussion/add`
- `content-type`: `application/json`
- `title`
- `contentJson.document.version: 2.9.0`
- `contentJson.document.theme: default`
- `contentJson.document.language: ko-KR`
- `contentJson.document.components[0].@ctype: text`
- `contentJson.document.components[0].value[0].@ctype: paragraph`
- `contentJson.document.components[0].value[0].nodes[0].@ctype: textNode`
- `discussionType`
- `itemCode`
- `txId`
- `inflow: NFS-P-P`
- 응답 본문 핵심값:
  - `isSuccess: true`
  - `result.id`

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- `src-tauri/src/naver_automation/post_form.rs`
- 흐름 함수: `CdpClient::submit_post_and_refresh`
- 실제 HTTP 패킷 함수: `NaverPacketClient::submit_post`

역할:

1. 현재 URL에서 종목 코드와 토론 타입을 읽습니다.
2. Chrome DevTools에서 로그인 쿠키를 읽어 Rust HTTP 클라이언트에 넣습니다.
3. Rust `reqwest`로 form 패킷을 보내 `txId`를 받습니다.
4. Rust `reqwest`로 add 패킷을 보내 글을 등록합니다.
5. 성공하면 화면을 새로고침합니다.

## 댓글 등록

Wireshark/F12에서 확인한 패킷:

- 토큰 발급:
  - `:method`: `GET`
  - `:authority`: `apis.naver.com`
  - `:path`: `/commentBox/cbox/web_naver_token_json.json?...`
  - 응답 본문 핵심값: `result.cbox_token`
- 댓글 생성:
  - `:method`: `POST`
  - `:authority`: `apis.naver.com`
  - `:path`: `/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=`
  - `content-type`: `application/x-www-form-urlencoded`
  - 요청 본문 핵심값:
    - `objectId`
    - `objectUrl`
    - `contents`
    - `cbox_token`
    - `commentType: txt`
    - `validateBanWords: true`

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- `src-tauri/src/naver_automation/post_form.rs`
- 흐름 함수: `CdpClient::submit_comment_and_refresh`
- 실제 HTTP 패킷 함수: `NaverPacketClient::submit_comment`

역할:

1. 현재 게시글 URL에서 `objectId`를 읽습니다.
2. Chrome DevTools에서 로그인 쿠키를 읽어 Rust HTTP 클라이언트에 넣습니다.
3. Rust `reqwest`로 토큰 API를 호출해 `cbox_token`을 받습니다.
4. Rust `reqwest`로 댓글 생성 API에 `contents`와 `cbox_token`을 전송합니다.
5. 성공하면 화면을 새로고침합니다.

## DOM 자동화 흐름

패킷 기반 등록 함수 앞뒤로 필요한 화면 이동과 UI 준비는 DOM 자동화로 유지합니다.

- 네이버 증권 토론방 이동
- 오늘의 종목 토론 둘러보기에서 랜덤 카테고리/순위 선택
- 전체 토론글 보러가기 클릭
- 글쓰기 클릭
- 댓글 작성 전 랜덤 게시글 선택
- 프로필이 없을 때 글쓰기 > 설정하기 > 소개 `2222` > 완료 처리
- 수동 확인 모드에서 제목/본문 또는 댓글 입력
- 수동 확인 모드에서 등록 대상 표시

CLI 실행 모드는 `submit_after_fill=true`라서 패킷 기반 등록 함수까지 실행합니다.
Tauri UI에서는 수동 확인 모드를 유지할 수 있도록 구조를 나누어 두었습니다.

## F12와 Wireshark 비교 기준

F12 Network에서 같은 요청을 찾을 때는 URL만 보지 말고 아래 항목을 같이 비교합니다.

- Method가 `GET`인지
- Host가 `static.nid.naver.com`인지
- Path가 `/getProfile`인지
- Query에 `svc=my`, `callback=...`이 있는지
- Request Headers에 Cookie가 포함되는지
- Response에 `rtn_cd`, `rtn_msg`, `nick_name`, `image_url`이 있는지

`ctrl+shift+c`로 확인하는 XPath/CSS selector는 DOM 요소 찾기용이고, 패킷 분석과는 다른 작업입니다.
