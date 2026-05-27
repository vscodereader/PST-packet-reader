# 네이버 패킷 매핑 문서

이 문서는 Wireshark와 Chrome F12 Network에서 확인한 HTTP 요청을 Rust 함수와 연결해 설명한다.

## 패킷과 DOM의 차이

패킷은 브라우저가 서버와 주고받는 HTTP 요청/응답이다.

예:

```text
POST https://m.stock.naver.com/front-api/discussion/add
content-type: application/json
```

DOM selector는 화면의 버튼, 입력창, div 같은 HTML 요소를 찾기 위한 정보다.

예:

```text
#write-editor-modal button
//*[@id="write-editor-modal"]/div[2]/div[3]/button
```

따라서 `Ctrl+Shift+C`로 XPath를 보는 것은 패킷 분석이 아니라 화면 요소 분석이다. 이번 구현에서는 등록 요청은 패킷 기반 함수로 만들고, 화면 이동이나 랜덤 클릭처럼 UI 흐름이 필요한 부분은 DOM 자동화로 유지한다.

## 구현된 패킷 기반 함수

| 기능 | 확인한 패킷 | Rust 함수 |
| --- | --- | --- |
| 로그인 확인 | `GET /getProfile?svc=my&callback=...` | `read_login_profile_from_packet` |
| 글쓰기 등록 | `POST /front-api/discussion/add` | `submit_post_and_refresh` |
| 댓글 토큰 발급 | `GET /commentBox/cbox/web_naver_token_json.json?...` | `submit_comment_and_refresh` |
| 댓글 등록 | `POST /commentBox/cbox/web_naver_create_json.json?...` | `submit_comment_and_refresh` |

## 로그인 확인 패킷

```text
:method: GET
:authority: static.nid.naver.com
:scheme: https
:path: /getProfile?svc=my&callback=<jsonp callback>
```

응답에는 아래 값이 포함된다.

```text
rtn_cd
rtn_msg
nick_name
image_url
```

코드 위치:

```text
src-tauri/src/naver_automation/packet_profile.rs
```

## 글쓰기 등록 패킷

```text
:method: POST
:authority: m.stock.naver.com
:path: /front-api/discussion/add
content-type: application/json
```

요청 본문 주요 값:

```text
title
contentJson
discussionType
itemCode
txId
inflow
```

응답 본문 주요 값:

```text
isSuccess: true
result.id
```

코드 위치:

```text
src-tauri/src/naver_automation/post_form.rs
```

## 댓글 등록 패킷

댓글 등록은 먼저 토큰을 받고, 그 토큰으로 댓글 생성 요청을 보낸다.

토큰 발급:

```text
:method: GET
:authority: apis.naver.com
:path: /commentBox/cbox/web_naver_token_json.json?...
```

응답:

```text
result.cbox_token
```

댓글 생성:

```text
:method: POST
:authority: apis.naver.com
:path: /commentBox/cbox/web_naver_create_json.json?ticket=finance&templateId=community&pool=cbox12&_cv=
content-type: application/x-www-form-urlencoded
```

요청 본문 주요 값:

```text
objectId
objectUrl
contents
cbox_token
commentType=txt
validateBanWords=true
```

코드 위치:

```text
src-tauri/src/naver_automation/post_form.rs
```

## 패킷 증빙 파일

제공된 `naver_http2_detail.txt`에서 핵심 패킷 구간을 분리해 아래 파일로 저장했다.

```text
C:\Users\user\Desktop\post_packet.txt
C:\Users\user\Desktop\comment_packet.txt
C:\Users\user\Desktop\profile_packet.txt
```

## 아직 DOM 기반인 부분

아래 작업은 현재 패킷 기반으로 완전히 대체하지 않았다.

- 네이버 증권 토론 메인 이동
- 랜덤 토론방 선택
- 전체 토론글 보러가기 클릭
- 글쓰기 모달 열기
- 댓글 작성을 위한 랜덤 게시글 열기
- 프로필 소개 `2222` 설정

프로필 소개 저장 패킷은 현재 캡처에서 명확히 확인되지 않았다. 나중에 이 요청이 잡히면 `setup_profile_if_needed` 내부를 패킷 기반 함수로 교체할 수 있다.
