# 알림에서 게시 원문 보기 + 글관리 배지 draft 제외 — 설계

- Date: 2026-06-05
- Issue: #112
- Status: Draft (사용자 검토 대기)

## Context

PR #104(실제 액션 기반 알림 피드)가 머지되어, 종목토론방 게시(`run_forum_publish_now`)가
`LogBatch`를 만들어 알림 화면에 기록된다. 그러나 두 가지가 남았다.

1. **알림 drill-down에 원문이 없다.** 알림에서 게시 항목을 펼치면(`notifications.tsx`의
   BatchRow/SubLog) 대상 종목·코드·계정·결과 메시지·실패 trace만 보인다. 사용자가
   **실제로 작성한 제목·본문·댓글**은 어디에도 안 보인다. (`BatchItem`에 원문이 없고,
   본문/댓글은 게시 요청에만 있다가 사라진다.) 사수도 "임시 구현, 수정 필요"라고 확인.
2. **글관리 사이드바 배지가 과다 카운트.** 배지는 `ipc.posts.list().length`
   (`app-shell.tsx:106`)로 **임시저장(draft) 포함 전체**를 세는데, 글관리 화면은
   draft를 제외(`posts.tsx:75`, `status !== "draft"`)한다. 그래서 화면엔 1개인데 배지엔
   2가 뜬다(draft 1개가 배지에만 잡힘).

## Goals

1. 알림에서 게시 항목을 펼치면 **그 게시에 올린 제목·본문·댓글 원문**을 보여준다.
2. 글관리 배지가 화면과 동일하게 **draft를 제외**한 개수를 표시한다.

## Non-Goals (YAGNI)

- ADB 카드에 디바이스 일련번호 표시 — 안 함(현재 "연결됨/미연결"로 충분).
- Chrome 진단 변경 — 안 함(이미 실제 설치 버전 표시).
- 알림 외 새 화면/네비게이션 추가 — 안 함(기존 BatchRow 확장만 사용).
- 게시 외 경로의 원문 스냅샷 — 종목토론방 게시(`run_forum_publish_now`)만 대상.

## Design

### 접근: Ⓐ 스냅샷 (게시 시점 원문을 알림 기록에 박아둠)

대안 Ⓑ(글관리 Posts 참조)는 글을 수정·삭제하면 "그때 올린 내용"이 어긋난다. 목적이
"내가 그때 무엇을 올렸는지 확인"이므로, 게시 순간의 원문을 그대로 보존하는 스냅샷을
택한다.

### 1. 데이터 모델 — `LogBatch`에 원문 필드 추가

`src-tauri/src/ipc/log_batches.rs:55` `LogBatch`에 옵션 필드 추가:

- `body: Option<String>` — 게시 본문 원문
- `comment: Option<String>` — 게시 댓글 원문(댓글 안 달았으면 None)

(`title`은 이미 존재.) `#[serde(default, skip_serializing_if = "Option::is_none")]`로
기존 직렬화·하위호환 유지. ts-rs 바인딩(`src/shared/bindings/LogBatch.ts`)과 수기
타입(`src/shared/data/types.ts`)도 `body?`/`comment?`로 갱신.

### 2. 캡처 — 게시 시 원문 스냅샷

`src-tauri/src/lib.rs:116`의 LogBatch 빌더(`run_forum_publish_now`에서 호출)가
`ForumPublishRequest`의 `title`/`body`/`comment`를 LogBatch에 그대로 담는다. 게시
당시 값이라 이후 글 수정·삭제와 무관하게 보존된다.

### 3. 렌더 — 알림 drill-down에 원문 표시

`src/features/notifications/notifications.tsx`의 BatchRow 확장 영역에서, 기존 대상별
SubLog **위에** "작성 내용" 블록을 추가한다:

- **제목**(굵게) — `batch.title`
- **본문** — `batch.body` (있을 때만)
- **댓글** — `batch.comment` (있을 때만)

빈 값은 줄을 생략한다. 줄바꿈 보존(`white-space: pre-wrap`). 기존 대상별 상태/메시지
목록은 그대로 그 아래 유지.

### 4. 배지 — draft 제외

`src/app/app-shell.tsx:106`:
`posts: posts.length` → `posts: posts.filter((p) => p.status !== "draft").length`
(글관리 화면 `posts.tsx:75`와 동일 기준). 다른 배지는 변경하지 않는다.

## Data Flow

```
게시(run_forum_publish_now)
  → ForumPublishRequest{title, body, comment, stocks…}
  → LogBatch{title, body, comment, kind, at, items[…]}  (스냅샷)
  → JsonStore<LogBatch> 저장
  → 알림 화면 list_log_batches → BatchRow 펼침 → 원문(제목·본문·댓글) + 대상별 상태
```

## Edge Cases

- 본문/댓글이 빈 게시 → 해당 줄 생략(렌더에서 falsy 체크).
- #104 이전에 생성된 기존 LogBatch(원문 없음) → `body`/`comment`가 None이라 원문 블록은
  제목만 표시(하위호환 깨지지 않음).
- 긴 본문 → 스크롤/줄바꿈 허용, 별도 트렁케이트는 하지 않음(YAGNI).

## Testing (TDD — 실패 테스트 먼저)

1. **배지**: `list_posts`에 draft 포함 시 배지 수가 draft를 제외함(프론트 단위 테스트,
   `app-shell.test.tsx`에 카운트 단언 추가 — 현재 미보호).
2. **LogBatch 직렬화**: `body`/`comment` 라운드트립 + 빈 값일 때 생략(Rust 테스트,
   `log_batches.rs`).
3. **스냅샷 캡처**: 게시 요청의 title/body/comment가 생성된 LogBatch에 담김(Rust 테스트,
   빌더 함수를 순수 함수로 분리해 검증).
4. **렌더**: 본문/댓글이 있는 배치는 원문 블록 표시, 없으면 생략(`notifications.test.tsx`).

## 작업 분할

- 본 작업은 issue #112, 브랜치 `feat/112`. 배지 수정(작은 fix)은 CLAUDE.md 규칙상 in-flight
  feat 브랜치에 함께 커밋한다(별도 PR 안 만듦).
