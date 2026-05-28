# 네이버 패킷 매핑 문서

이 문서는 Wireshark/tshark로 확인한 네이버 증권 패킷과 Rust 코드의 대응 관계를 설명한다.

## 패킷과 DOM의 차이

패킷은 브라우저와 서버가 주고받는 HTTP 요청/응답이다.

예:

```text
GET https://stock.naver.com/api/community/discussion/posts/by-item?...
POST https://m.stock.naver.com/front-api/discussion/add
```

DOM selector는 화면의 버튼, 입력창, div 같은 HTML 요소를 찾는 정보다.

예:

```text
#write-editor-modal button
//*[@id="write-editor-modal"]/div[2]/div[3]/button
```

따라서 `Ctrl+Shift+C`로 XPath를 보는 것은 패킷 분석이 아니다. 이번 구현은 Wireshark로 확인한 API 요청을 Rust 함수로 옮기고, Chrome은 로그인 세션과 결과 화면 확인에만 사용한다.

## 사용한 캡처

- `naver_random_capture.pcapng`: 토론급상승/상승/하락/거래량 클릭, 종목 이동, 프로필 소개 `2222` 입력 흐름
- `naver_capture_success.pcapng`: 프로필 생성 성공 흐름
- `keylogfile.txt`: TLS 1.3 복호화용 SSL key log

## 구현된 패킷 기반 함수

| 기능             | 확인한 패킷                                                                        | Rust 함수                                                     |
| ---------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| 로그인 확인      | `GET static.nid.naver.com/getProfile?svc=my&callback=...`                          | `NaverPacketClient::read_login_profile`                       |
| 랜덤 종목 선택   | `GET /api/community/discussion/rankings`, `GET /api/domestic/market/stock/default` | `NaverPacketClient::select_random_discussion_room`            |
| 랜덤 게시글 선택 | `GET /api/community/discussion/posts/by-item`                                      | `NaverPacketClient::select_random_discussion_post`            |
| 프로필 2222 설정 | `GET status`, `GET form`, `POST introduction/validate`, `PUT users/<profileId>`    | `NaverPacketClient::ensure_profile_intro_setup`               |
| 글쓰기 txId 발급 | `POST /front-api/discussion/form`                                                  | `NaverPacketClient::submit_post` 내부의 `issue_post_tx_id`    |
| 글쓰기 등록      | `POST /front-api/discussion/add`                                                   | `NaverPacketClient::submit_post`                              |
| 댓글 토큰 발급   | `GET /commentBox/cbox/web_naver_token_json.json`                                   | `NaverPacketClient::submit_comment` 내부의 `issue_cbox_token` |
| 댓글 등록        | `POST /commentBox/cbox/web_naver_create_json.json`                                 | `NaverPacketClient::submit_comment`                           |

## 로그인 확인 getProfile

패킷:

```text
:method: GET
:authority: static.nid.naver.com
:path: /getProfile?svc=my&callback=<jsonp callback>
```

응답에는 `rtn_cd`, `rtn_msg`, `nick_name`, `image_url`이 포함된다. `rtn_cd`가 `0`이면 로그인 성공으로 판단한다.

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::read_login_profile
```

## 랜덤 종목 선택

패킷:

```text
GET /api/community/discussion/rankings?nationType=KOR&page=1&size=10&postType=HOT
GET /api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=up&startIdx=0&pageSize=10
GET /api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=down&startIdx=0&pageSize=10
GET /api/domestic/market/stock/default?tradeType=KRX&marketType=ALL&orderType=quantTop&startIdx=0&pageSize=10
```

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::select_random_discussion_room
```

처리 방식:

1. 토론급상승/상승/하락/거래량 중 하나를 고른다.
2. Rust가 해당 API를 호출한다.
3. 응답 JSON에서 종목 코드와 종목명을 추출한다.
4. 선택된 종목 코드로 토론방 URL을 만든다.
5. Chrome은 생성된 URL로 이동만 한다.

## 랜덤 게시글 선택

패킷:

```text
GET /api/community/discussion/posts/by-item?discussionType=domesticStock&itemCode=<종목코드>&isHolderOnly=false&excludesItemNews=false&isItemNewsOnly=false&isCleanbotPassedOnly=true&pageSize=10
```

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::select_random_discussion_post
```

처리 방식:

1. 현재 종목 토론방 URL에서 `discussionType`, `itemCode`를 계산한다.
2. Rust가 `posts/by-item` API를 호출한다.
3. 응답 JSON에서 게시글 ID를 추출한다.
4. 선택된 게시글 ID로 상세 URL을 만든다.
5. Chrome은 생성된 게시글 URL로 이동만 한다.

## 프로필 소개 2222 설정

성공 캡처에서 확인한 흐름:

```text
GET  /api/community/profile/users/status
GET  /api/community/profile/users/form
POST /api/community/profile/users/introduction/validate
PUT  /api/community/profile/users/<profileId>
GET  /api/community/profile/users/status
```

`introduction/validate` 요청 본문:

```json
{
  "targetValue": "2222"
}
```

`PUT /users/<profileId>` 요청 본문:

```json
{
  "nickname": "<기존 nickname 또는 추천 nickname>",
  "introduction": "2222",
  "imageUrl": null,
  "danglingImages": []
}
```

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::ensure_profile_intro_setup
```

처리 방식:

1. `status`가 `existent`이면 이미 프로필이 있으므로 종료한다.
2. `status`가 `inactive`이면 응답의 `profileId`를 사용한다.
3. `form` API에서 기존 nickname과 imageUrl을 읽는다.
4. nickname이 비어 있으면 nickname 추천 API를 호출한다.
5. 소개 `2222`를 validate API로 검증한다.
6. 성공 캡처와 동일하게 `PUT /users/<profileId>`로 저장한다.
7. 다시 `status`를 호출해 `existent`가 되었는지 확인한다.

## 글쓰기 등록

패킷:

```text
POST /front-api/discussion/form?discussionType=<토론타입>&itemCode=<종목코드>
POST /front-api/discussion/add
```

`add` 요청에는 `form` 응답의 `result.txId`가 필요하다. 직접 만든 `txId`를 보내면 `TX_ID_MISMATCH`가 발생한다.

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::submit_post
```

## 댓글 등록

패킷:

```text
GET  /commentBox/cbox/web_naver_token_json.json?ticket=finance&templateId=community&pool=cbox12...
POST /commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=
```

코드:

```text
src-tauri/src/naver_automation/packet_client.rs
NaverPacketClient::submit_comment
```

## 남아 있는 브라우저 역할

자동 제출 경로에서 서버 요청은 Rust 패킷 함수가 수행한다. 브라우저는 아래 역할로 남아 있다.

- 네이버 로그인과 2차 인증을 사용자가 완료한 세션 제공
- Chrome DevTools로 쿠키 읽기
- Rust가 선택한 URL로 화면 이동
- 등록 후 새로고침해서 사용자가 결과 확인
- 수동 확인 모드에서 입력란 채우기와 버튼 강조

이 구조 때문에 “등록 패킷을 프론트 fetch로 넘긴다”는 방식은 제거되었다.
