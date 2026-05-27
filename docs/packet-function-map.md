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

- `:method`: `POST`
- `:authority`: `m.stock.naver.com`
- `:path`: `/front-api/discussion/add`
- `content-type`: `application/json`
- 요청 본문 핵심값:
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

- `src-tauri/src/naver_automation/post_form.rs`
- 함수: `CdpClient::submit_post_and_refresh`

역할:

1. 현재 URL에서 종목 코드와 토론 타입을 읽습니다.
2. 패킷에서 확인한 `contentJson` 구조로 글 제목/본문 요청을 만듭니다.
3. 로그인된 Chrome 탭 안에서 `fetch`를 실행해 브라우저 쿠키를 그대로 사용합니다.
4. 성공하면 화면을 새로고침합니다.

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

- `src-tauri/src/naver_automation/post_form.rs`
- 함수: `CdpClient::submit_comment_and_refresh`

역할:

1. 현재 게시글 URL에서 `objectId`를 읽습니다.
2. 패킷에서 확인한 토큰 API로 `cbox_token`을 받습니다.
3. 패킷에서 확인한 댓글 생성 API에 `contents`와 `cbox_token`을 전송합니다.
4. 성공하면 화면을 새로고침합니다.

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
