# Packet Function Map

이 문서는 Wireshark/tshark로 복호화한 네이버 증권 요청을 Rust 함수로 옮긴 위치를 정리한다.

패킷 분석에 사용한 파일:

- `C:\Users\user\Desktop\naver_random_capture.pcapng`
- `C:\Users\user\Desktop\naver_capture_success.pcapng`
- `C:\Users\user\packet-123\keylogfile.txt`

주의: 쿠키 원문은 문서에 남기지 않는다. 코드도 비밀번호를 저장하지 않고, 사용자가 로그인한 Chrome 세션의 쿠키를 실행 시점에만 읽는다.

## 1. Rust 패킷 클라이언트 생성

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `CdpClient::build_naver_packet_client`

역할:

1. Chrome DevTools `Network.getCookies`로 로그인된 네이버 쿠키를 읽는다.
2. `NID_AUT`, `NID_SES`가 없으면 로그인되지 않은 상태로 보고 중단한다.
3. Rust `reqwest::blocking::Client`를 만든다.
4. 이후 네이버 API 요청은 브라우저 `fetch`가 아니라 Rust `reqwest`가 직접 보낸다.

## 2. 로그인 확인 getProfile

Wireshark에서 확인한 패킷:

```text
GET https://static.nid.naver.com/getProfile?svc=my&callback=<jsonp callback>
```

응답 형태:

```text
jsonp_callback({
  "rtn_cd": "0",
  "rtn_msg": "Success",
  "nick_name": "...",
  "image_url": "..."
});
```

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::read_login_profile`

역할:

1. 캡처에서 확인한 `static.nid.naver.com/getProfile` JSONP 요청을 Rust에서 직접 보낸다.
2. JSONP wrapper를 제거한다.
3. `rtn_cd == "0"`이면 로그인 성공으로 판단한다.
4. 닉네임과 프로필 이미지 URL을 `NaverLoginProfile`로 변환한다.

## 3. 랜덤 종목 선택

Wireshark에서 확인한 패킷:

```text
GET https://stock.naver.com/api/community/discussion/rankings?nationType=KOR&page=1&size=10&postType=HOT
GET https://stock.naver.com/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=10
GET https://stock.naver.com/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=10
GET https://stock.naver.com/api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=10
```

각 요청의 의미:

- `postType=HOT`: 토론급상승
- `orderType=up`: 상승
- `orderType=down`: 하락
- `orderType=quantTop`: 거래량

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::select_random_discussion_room`
- 보조 함수: `collect_stock_candidates`, `discussion_url_for`

역할:

1. 네 카테고리 중 하나를 시간값 기반으로 고른다.
2. 해당 카테고리 API를 Rust에서 직접 호출한다.
3. 응답 JSON에서 종목 코드와 종목명을 추출한다.
4. 후보 중 하나를 고른다.
5. 선택된 종목 코드를 토론방 URL로 변환한다.
6. Chrome 화면은 해당 URL로 이동만 한다.

즉, 랜덤 종목 선택 판단은 DOM 클릭이 아니라 API 응답 기반이다.

## 4. 랜덤 게시글 선택

Wireshark에서 확인한 패킷:

```text
GET https://stock.naver.com/api/community/discussion/posts/by-item?discussionType=domesticStock&itemCode=<종목코드>&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=true&pageSize=10
```

보조 fallback 요청:

```text
GET https://stock.naver.com/api/community/discussion/posts/by-item?discussionType=<토론타입>&itemCode=<종목코드>&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=false&pageSize=30
```

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::select_random_discussion_post`
- 보조 함수: `collect_post_candidates`, `discussion_target_from_url`, `discussion_url_for`

역할:

1. 현재 종목 토론방 URL에서 `discussionType`, `itemCode`를 계산한다.
2. 캡처에서 확인한 `posts/by-item` API를 Rust에서 직접 호출한다.
3. 응답 JSON에서 게시글 ID 후보를 추출한다.
4. 후보 중 하나를 고른다.
5. 게시글 상세 URL을 만든다.
6. Chrome 화면은 해당 게시글 URL로 이동만 한다.

즉, 댓글 대상 게시글 선택도 화면에서 글을 클릭하는 방식이 아니라 API 응답 기반이다.

## 5. 프로필 소개 2222 설정

성공 캡처에서 확인한 패킷 흐름:

```text
GET  https://stock.naver.com/api/community/profile/users/status
GET  https://stock.naver.com/api/community/profile/users/form
POST https://stock.naver.com/api/community/profile/users/introduction/validate
PUT  https://stock.naver.com/api/community/profile/users/<profileId>
GET  https://stock.naver.com/api/community/profile/users/status
```

소개 검증 요청 본문:

```json
{
  "targetValue": "2222"
}
```

프로필 저장 요청 본문:

```json
{
  "nickname": "<기존 nickname 또는 추천 nickname>",
  "introduction": "2222",
  "imageUrl": null,
  "danglingImages": []
}
```

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::ensure_profile_intro_setup`
- 보조 함수: `recommend_profile_nickname`, `validate_profile_introduction`

역할:

1. `status` API로 프로필 상태를 확인한다.
2. 이미 `existent`이면 아무 작업도 하지 않는다.
3. 프로필이 없으면 `profileId`를 읽는다.
4. `form` API로 기존 nickname/imageUrl을 읽는다.
5. nickname이 없으면 추천 nickname API를 호출한다.
6. 소개 `2222`를 validate API로 검증한다.
7. 성공 캡처와 동일하게 `PUT /api/community/profile/users/<profileId>`로 저장한다.
8. 다시 status API를 호출해서 `existent`가 되었는지 확인한다.

즉, 프로필 생성/소개 설정도 더 이상 글쓰기 팝업 DOM 입력에 의존하지 않는다.

## 6. 글쓰기 등록

Wireshark/F12에서 확인한 패킷:

```text
POST https://m.stock.naver.com/front-api/discussion/form?discussionType=<토론타입>&itemCode=<종목코드>
POST https://m.stock.naver.com/front-api/discussion/add
```

중요한 점:

- `/front-api/discussion/add`의 `txId`는 직접 만들면 안 된다.
- 먼저 `/front-api/discussion/form`에서 받은 `result.txId`를 그대로 넣어야 한다.
- 그렇지 않으면 `TX_ID_MISMATCH` 오류가 발생한다.

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::submit_post`
- 보조 함수: `issue_post_tx_id`, `build_post_payload`
- 흐름 함수: `CdpClient::submit_post_and_refresh`

## 7. 댓글 등록

Wireshark/F12에서 확인한 패킷:

```text
GET  https://apis.naver.com/commentBox/cbox/web_naver_token_json.json?ticket=finance&templateId=community&pool=cbox12...
POST https://apis.naver.com/commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=
```

코드 위치:

- `src-tauri/src/naver_automation/packet_client.rs`
- 함수: `NaverPacketClient::submit_comment`
- 보조 함수: `issue_cbox_token`, `build_comment_form`
- 흐름 함수: `CdpClient::submit_comment_and_refresh`

## 현재 Chrome/DOM이 맡는 역할

자동 제출 경로에서 핵심 데이터 요청은 Rust 패킷 함수로 옮겼다. Chrome/DOM은 아래 용도로만 남아 있다.

- 사용자가 직접 로그인과 2차 인증을 끝낸 세션 제공
- 로그인 쿠키를 DevTools로 읽기
- 선택된 토론방/게시글 URL로 화면 이동
- 등록 성공 후 새로고침해서 결과 확인
- 수동 확인 모드에서 입력란 채우기와 버튼 강조

CLI 기본 실행은 `submit_after_fill=true`라서 글/댓글 등록은 Rust 패킷 함수로 수행한다.
