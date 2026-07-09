# 설계서 — 종목토론방 "특정 게시글" 여러 댓글 게시 + "나눠서 게시"(댓글 분배)

Date: 2026-07-09
Status: Proposed (구현 전 리뷰용)

## 배경 / 문제

글 관리 화면에서 **종목토론방(forum) "특정 게시글"** 모드로:

1. 게시글 **링크 여러 개**를 넣고
2. 그 밑에 **댓글 여러 개**를 쓰고
3. **저장**한 뒤
4. **게시하기** → 계정 여러 개 선택 → **지금 바로 게시 / 예약 게시**

로 게시한다.

**버그**: 댓글을 여러 개 써도 **맨 위(첫) 댓글 하나만** 각 링크에 게시된다.

- 원인: `src-tauri/src/ipc/queue_runner.rs:1265 plan_to_forum_requests()`,
  `1270`: `let comment = plan.comments.first().cloned().unwrap_or_default();`
  → 댓글 풀에서 **첫 항목만** 뽑아 `ForumPublishRequest.comment`(단일 문자열,
  `discussion_batch.rs:85`)에 싣는다. url_reqs(특정글, `1291`)·regular(`1309`) 둘 다 동일.
- 참고: `run_forum_publish`(`discussion_batch.rs:219`)는 요청 1건당 `comment` 1개를
  `comment_url` 글에 단다. 즉 "요청 1건 = 댓글 1개" 구조.

## 요구 동작

### A) 정상 게시(지금 바로 / 예약) — 전체 댓글

각 **(선택 계정 × 링크)** 마다 **작성한 모든 댓글**을 단다.

- 예) 링크 3 · 댓글 6 · 계정 4 → 링크당 (4계정 × 6댓글)=24, × 3링크 = **72개**.
- 예) 링크 N · 계정 1 · 댓글 여러 개 → 그 계정이 각 링크에 **모든 댓글**을 단다.

### B) 신규 "나눠서 게시"(댓글 1:1 분배)

- **버튼**: 게시 설정 창에서 "지금 바로 게시 / 예약 게시"와 **동일 UI**의 3번째 버튼.
- **노출 조건**: **특정 게시글 + 링크 + 댓글 작성 + 저장** 맥락에서만 보인다.
  (그 외 — 최신/인기 댓글, 랜덤 종목 게시 등 — 에서는 숨김.)
- **활성 조건**: **작성 댓글 수 == 선택 계정 수**.
  - 불일치(댓글 > 계정 또는 계정 > 댓글)면 **비활성** + "나눠서 게시" 글씨 아래
    **회색 글씨**로 `댓글 : N개   계정 : M개` 표시.
- **동작**: 각 **링크**마다 댓글을 계정에 **1:1 무작위·겹침 없이** 배정 → 각 계정이 자기
  몫 댓글 1개를 그 링크에 단다.
  - 예) 댓글 4(안녕하세요/반갑습니다/저두요/가시죠!) · 계정 A B C D → 각 링크에서
    임의 1:1 매칭(누가 어떤 댓글일지는 랜덤, 단 겹치지 않음). 링크마다 반복.

## 재사용 원칙 (없는 것만 신규)

| 필요 | 재사용할 기존 자산 | 위치 |
| --- | --- | --- |
| 댓글 1:1 무작위·겹침없는 분배 | `distribute_comments` + `mulberry32` + `shuffle` + `seed_from_clock` | `src-tauri/src/naver_cafe/distribute.rs` (이미 `queue_runner.rs:34`에서 import) |
| 즉시/예약 큐 적재 | `dispatchPublish(jobs, when)` | `src/features/posts/publish-modal.tsx:1748` |
| 계정별 1큐 분배 적재 | `dispatchSplitNow(jobs)` | `publish-modal.tsx:1838` |
| 종토 특정글 잡 생성 | 기존 `commentUrl` 잡 빌드 | `publish-modal.tsx:1288` |
| 종토 요청 실행 엔진 | `run_forum_publish` (요청당 댓글 1개) | `discussion_batch.rs:219` |
| 계정 체크박스 UI | `AccountRow` | `publish-modal.tsx` |

**무손상**: 기존 "나눠서 게시"(= **종목**을 계정에 분배, `canDistribute`
`publish-modal.tsx:1543` / `distributeForumJobs:1551` / `dispatchSplitNow`)는 **종목 잡
(`code` 있고 `commentUrl` 없음)** 만 다루고 특정글 잡(`commentUrl`)은 명시적으로 제외한다
(`1536`,`1558`). 새 댓글 분배는 **특정글 맥락에서만** 동작하므로 두 기능은 배타적 — 기존
종목 분배·카페·블로그·밴드·좋아요는 건드리지 않는다.

## 구현 지점 (초안)

### 백엔드 — `queue_runner.rs plan_to_forum_requests`

특정글(`comment_url`) 경로에서 **댓글 1개 → 모든 댓글**로 확장. "요청 1건 = 댓글 1개"
엔진을 그대로 쓰되 **요청을 여러 개 emit** 한다(엔진 무변경, 재사용).

- **정상(A)**: 각 (계정 × 링크)마다 `plan.comments` 전체를 순회해 요청 N개 생성.
- **분배(B)**: 링크마다 `distribute_comments(계정수, plan.comments, rng)`로 계정별 1개
  배정 → (계정, 링크, 배정댓글) 요청 생성. `rng`는 `seed_from_clock`(카페와 동일).
- 정상/분배 선택은 plan에 실어 전달(예: `plan.forum_comment_distribute: bool`,
  기존 plan 스키마에 필드 추가). 없으면 정상(A) = 하위호환.

### 프론트 — `publish-modal.tsx`

- 특정글+댓글+저장 맥락 판별: `commentTargetMode === "url"` && forum 대상 &&
  `doc.commentUrls`/`comments` 존재(기존 상태 재사용).
- **"나눠서 게시" 버튼**(3번째): 기존 지금바로/예약 버튼과 동일 스타일. 위 맥락에서만 렌더.
  - 활성: `comments.length === 선택 forum 계정 수`.
  - 비활성 시 버튼 아래 회색 `댓글 : N개   계정 : M개`.
  - onClick: 정상 dispatch 재사용하되 plan에 `forum_comment_distribute=true` 세팅.

## 열린 질문 (구현 전 확인)

1. **분배 재무작위화 범위**: 링크마다 매칭을 다시 셔플할지(권장), 전 링크 동일 매칭일지.
   기본안 = **링크마다 재셔플**(각 링크 독립 1:1).
2. 정상(A) 모드에서 **댓글 게시 순서**: 작성 순서대로 vs 무작위. 기본안 = **작성 순서**.
3. 결과/진행률 표시: 요청이 댓글 수만큼 늘어나므로 완료 로그 건수도 늘어난다(정상 동작).

## 검증 계획

- 백엔드: `plan_to_forum_requests` 단위 테스트 — (링크·댓글·계정) 조합별 요청 개수/내용,
  분배 시 계정별 1개·겹침 없음(시드 주입 결정성, `distribute.rs` 테스트 패턴 재사용).
- 프론트: 버튼 노출/활성 조건, 회색 카운트 표시, onClick이 분배 플래그로 dispatch 호출.
- E2E(Windows): 특정글 3링크·6댓글·4계정 정상=72건, 4링크·4댓글·4계정 나눠서=링크당 1:1.
