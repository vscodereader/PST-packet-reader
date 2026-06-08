# 알림 원문 보기 + 글관리 배지 draft 제외 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 알림에서 게시 항목을 펼치면 그때 올린 제목·본문·댓글 원문을 보여주고, 글관리 사이드바 배지가 임시저장(draft)을 제외한 개수를 표시한다.

**Architecture:** 게시 경로(`run_forum_publish_now`)가 만드는 `LogBatch`에 `body`/`comment` 옵션 필드를 추가해 게시 시점 원문을 스냅샷 저장한다. 프론트는 그 필드를 알림 drill-down에 렌더한다. 배지는 글관리 화면과 동일한 draft-제외 필터를 쓴다.

**Tech Stack:** Rust(serde, ts-rs), React/TypeScript(Mantine), Vitest, cargo test.

Spec: `docs/superpowers/specs/2026-06-05-notification-original-content-design.md` (issue #112)

---

## File Structure

- `src-tauri/src/ipc/log_batches.rs` — `LogBatch` 구조체에 `body`/`comment` 추가 + 직렬화 테스트
- `src-tauri/src/lib.rs` — `build_publish_batch`가 원문 스냅샷, `run_forum_publish_now`가 요청 원문 전달
- `src/shared/bindings/LogBatch.ts` — ts-rs 재생성(자동)
- `src/shared/data/types.ts` — 수기 `LogBatch` 인터페이스에 `body?`/`comment?`
- `src/app/app-shell.tsx` — 배지 draft 제외
- `src/features/notifications/notifications.tsx` — BatchRow 확장에 원문 블록
- `src/test/ipc.ts` — LogBatch mock 픽스처에 `body`/`comment` 추가(렌더 테스트용)

---

### Task 1: LogBatch — body/comment 필드 추가

**Files:**

- Modify: `src-tauri/src/ipc/log_batches.rs:55-65` (struct), `:85-99` (test)

- [ ] **Step 1: 실패 테스트 작성** — `log_batches.rs`의 `mod tests`에 추가:

```rust
    #[test]
    fn log_batch_roundtrips_body_and_comment() {
        let b = LogBatch {
            id: "b2".into(),
            title: "제목".into(),
            body: Some("<p>본문</p>".into()),
            comment: Some("좋네요".into()),
            kind: ModeValue::Both,
            at: 1_700_000_000_000,
            state: None,
            items: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"body\":\"<p>본문</p>\""));
        assert!(json.contains("\"comment\":\"좋네요\""));
        assert_eq!(b, serde_json::from_str::<LogBatch>(&json).unwrap());
    }

    #[test]
    fn log_batch_omits_none_body_comment() {
        let b = LogBatch {
            id: "b3".into(),
            title: "제목".into(),
            body: None,
            comment: None,
            kind: ModeValue::Post,
            at: 1,
            state: None,
            items: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(!json.contains("body"));
        assert!(!json.contains("comment"));
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib log_batch`
      Expected: 컴파일 에러 (`LogBatch` has no field `body`) + 기존 `log_batch_serializes_at_as_number`도 컴파일 실패(필드 누락).

- [ ] **Step 3: 구조체에 필드 추가** — `log_batches.rs`의 `LogBatch`를 다음으로(기존 `title` 다음 줄, `kind` 앞에 삽입):

```rust
pub struct LogBatch {
    pub id: String,
    pub title: String,
    /// 게시 본문 원문(스냅샷). 본문 없는 게시면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    /// 게시 댓글 원문(스냅샷). 댓글 없는 게시면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub comment: Option<String>,
    pub kind: ModeValue,
    #[ts(type = "number")]
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<BatchState>,
    pub items: Vec<BatchItem>,
}
```

- [ ] **Step 4: 기존 테스트도 새 필드로 갱신** — `log_batch_serializes_at_as_number`의 구조체 리터럴에 `body: None, comment: None,`를 `title` 다음에 추가.

- [ ] **Step 5: 통과 확인** — Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib log_batch`
      Expected: PASS (3개 테스트).

- [ ] **Step 6: 커밋**

```bash
git add src-tauri/src/ipc/log_batches.rs
git commit -m "feat(notifications): add body/comment snapshot fields to LogBatch"
```

---

### Task 2: 게시 시 원문 스냅샷 (build_publish_batch)

**Files:**

- Modify: `src-tauri/src/lib.rs:109-159` (build_publish_batch), `:167-188` (run_forum_publish_now 호출부)

- [ ] **Step 1: 실패 테스트 작성** — `lib.rs`의 `#[cfg(test)] mod tests`에 추가(`build_publish_batch`는 같은 크레이트의 private fn이라 직접 호출 가능):

```rust
    #[test]
    fn build_publish_batch_snapshots_nonempty_body_and_comment() {
        let results = vec![ForumPublishResult {
            code: "005930".into(),
            name: "삼성전자".into(),
            ok: true,
            message: "게시 완료".into(),
        }];
        let b = super::build_publish_batch(
            "제목", true, true, "acct", "본문내용", "댓글내용", 1, &results,
        );
        assert_eq!(b.body.as_deref(), Some("본문내용"));
        assert_eq!(b.comment.as_deref(), Some("댓글내용"));
    }

    #[test]
    fn build_publish_batch_omits_empty_and_unused_text() {
        let results = vec![];
        // run_comment=false → comment 무시; body 빈 문자열 → None
        let b = super::build_publish_batch("제목", true, false, "acct", "", "안쓴댓글", 1, &results);
        assert_eq!(b.body, None);
        assert_eq!(b.comment, None);
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib build_publish_batch`
      Expected: 컴파일 에러(인자 개수 불일치 / `body` 필드 없음).

- [ ] **Step 3: build_publish_batch 시그니처+본문 수정** — `lib.rs:109`의 함수 시그니처에 `body`/`comment` 인자를 `account_id` 다음에 추가하고, 반환 리터럴에 스냅샷을 넣는다:

```rust
fn build_publish_batch(
    title: &str,
    run_post: bool,
    run_comment: bool,
    account_id: &str,
    body: &str,
    comment: &str,
    at: i64,
    results: &[ForumPublishResult],
) -> ipc::log_batches::LogBatch {
```

그리고 함수 끝의 `LogBatch { ... }` 리터럴(`:151`)을 다음으로:

```rust
    LogBatch {
        id: format!("lb-{at}-{seq}"),
        title: title.to_owned(),
        // 게시 시점 원문 스냅샷: 실제 게시한 것만 남긴다.
        body: if run_post && !body.is_empty() {
            Some(body.to_owned())
        } else {
            None
        },
        comment: if run_comment && !comment.is_empty() {
            Some(comment.to_owned())
        } else {
            None
        },
        kind,
        at,
        state: None,
        items,
    }
```

- [ ] **Step 4: 호출부에서 원문 전달** — `run_forum_publish_now`에서 `request`가 spawn_blocking으로 move되기 전에 원문을 클론한다. `lib.rs:167-169`의 `let title = request.title.clone();` 블록에 두 줄 추가:

```rust
    let title = request.title.clone();
    let body = request.body.clone();
    let comment = request.comment.clone();
    let account_id = request.account_id.clone();
```

그리고 `:188`의 호출을 다음으로:

```rust
    let batch = build_publish_batch(
        &title, run_post, run_comment, &account_id, &body, &comment, at, &results,
    );
```

- [ ] **Step 5: 통과 확인** — Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib build_publish_batch`
      Expected: PASS (2개).

- [ ] **Step 6: 전체 컴파일·테스트** — Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
      Expected: PASS (0 failed).

- [ ] **Step 7: 커밋**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(notifications): snapshot post body/comment into LogBatch on publish"
```

---

### Task 3: 바인딩 재생성 + 수기 타입 갱신

**Files:**

- Regenerate: `src/shared/bindings/LogBatch.ts`
- Modify: `src/shared/data/types.ts:146-153`

- [ ] **Step 1: ts-rs 바인딩 재생성** — Run: `pnpm gen:bindings`
      Expected: `src/shared/bindings/LogBatch.ts`에 `body?: string, comment?: string` 추가됨. 확인: `grep -n "body\|comment" src/shared/bindings/LogBatch.ts`

- [ ] **Step 2: 수기 LogBatch 인터페이스 갱신** — `src/shared/data/types.ts`의 `LogBatch`를:

```ts
export interface LogBatch {
  id: string;
  title: string;
  body?: string;
  comment?: string;
  kind: ModeValue;
  at: number;
  state?: "running";
  items: BatchItem[];
}
```

- [ ] **Step 3: 타입체크** — Run: `pnpm typecheck`
      Expected: 에러 없음.

- [ ] **Step 4: 커밋**

```bash
git add src/shared/bindings/LogBatch.ts src/shared/data/types.ts
git commit -m "chore(bindings): regenerate LogBatch with body/comment"
```

---

### Task 4: 글관리 배지 draft 제외

**Files:**

- Modify: `src/app/app-shell.tsx:106`
- Test: `src/app/app-shell.test.tsx`

- [ ] **Step 1: 실패 테스트 작성** — `app-shell.test.tsx`의 `describe("MacroApp", …)` 안에 추가(상단에 `import type { LibraryPost } from "@/shared/bindings/LibraryPost";` 필요 시 추가):

```tsx
it("글 관리 badge counts only non-draft posts", async () => {
  const { invoke } = await import("@/test/ipc");
  const posts = await invoke<{ status: string }[]>("list_posts");
  const expected = posts.filter((p) => p.status !== "draft").length;
  // 시드에 draft가 있어야 의미 있는 검증
  expect(expected).toBeLessThan(posts.length);
  renderApp();
  const btn = navButton(/글 관리/);
  expect(await within(btn).findByText(String(expected))).toBeInTheDocument();
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm test -- app-shell`
      Expected: FAIL — 배지가 `posts.length`(draft 포함, 예: 5)라서 `expected`(예: 3)와 불일치.

- [ ] **Step 3: 배지 수정** — `app-shell.tsx:106`의 `posts: posts.length,`를:

```ts
        // 글관리 화면(posts.tsx)이 draft를 숨기므로 배지도 동일 기준으로 센다.
        posts: posts.filter((p) => p.status !== "draft").length,
```

- [ ] **Step 4: 통과 확인** — Run: `pnpm test -- app-shell`
      Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add src/app/app-shell.tsx src/app/app-shell.test.tsx
git commit -m "fix(ui): 글 관리 badge excludes draft posts to match the list"
```

---

### Task 5: 알림 drill-down에 원문 렌더

**Files:**

- Modify: `src/features/notifications/notifications.tsx:205`
- Modify: `src/test/ipc.ts` (LogBatch mock 픽스처 한 개에 body/comment 추가)
- Test: `src/features/notifications/notifications.test.tsx`

- [ ] **Step 1: mock 픽스처에 원문 추가** — `src/test/ipc.ts`에서 제목이 `"5월 이벤트 결과 발표"`인 LogBatch 픽스처 객체에 `body`/`comment` 필드를 추가한다(`title` 다음 줄):

```ts
    body: "<p>5월 이벤트 결과를 정리했습니다. 많은 참여 감사드립니다.</p>",
    comment: "이벤트 참여 감사합니다 🙌",
```

- [ ] **Step 2: 실패 테스트 작성** — `notifications.test.tsx`에 추가:

```tsx
it("shows the written body and comment when a batch is expanded", async () => {
  renderLog();
  await userEvent.click(await screen.findByText("5월 이벤트 결과 발표"));
  expect(
    screen.getByText(/5월 이벤트 결과를 정리했습니다/),
  ).toBeInTheDocument();
  expect(screen.getByText(/이벤트 참여 감사합니다/)).toBeInTheDocument();
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm test -- notifications`
      Expected: FAIL — 원문이 렌더되지 않아 텍스트를 못 찾음.

- [ ] **Step 4: BatchRow 확장에 원문 블록 추가** — `notifications.tsx:205`의 `{expanded && batch.items.map(...)}` **앞에** 원문 블록을 삽입:

```tsx
{
  expanded && (
    <Box
      px={16}
      py={12}
      bg="gray.0"
      style={{ borderTop: "1px solid var(--mantine-color-gray-2)" }}
    >
      <Text fz={11.5} c="dimmed" mb={3}>
        작성 내용
      </Text>
      <Text
        fz={13}
        fw={700}
        mb={batch.body || batch.comment ? 6 : 0}
        style={{ whiteSpace: "pre-wrap" }}
      >
        {batch.title}
      </Text>
      {batch.body && (
        <Text
          fz={12.5}
          mb={batch.comment ? 8 : 0}
          style={{ whiteSpace: "pre-wrap" }}
        >
          {batch.body}
        </Text>
      )}
      {batch.comment && (
        <>
          <Text fz={11.5} c="dimmed" mb={2}>
            댓글
          </Text>
          <Text fz={12.5} style={{ whiteSpace: "pre-wrap" }}>
            {batch.comment}
          </Text>
        </>
      )}
    </Box>
  );
}
{
  expanded && batch.items.map((item, i) => <SubLog key={i} item={item} />);
}
```

- [ ] **Step 5: 통과 확인** — Run: `pnpm test -- notifications`
      Expected: PASS (기존 expand 테스트 포함 전부).

- [ ] **Step 6: 커밋**

```bash
git add src/features/notifications/notifications.tsx src/test/ipc.ts src/features/notifications/notifications.test.tsx
git commit -m "feat(notifications): show written title/body/comment in batch drill-down"
```

---

### Task 6: 최종 검증 + PR

- [ ] **Step 1: 전체 테스트** — Run: `pnpm test` (vitest) + `cargo test --manifest-path src-tauri/Cargo.toml --lib`
      Expected: 둘 다 0 failed.

- [ ] **Step 2: 포맷/린트** — Run: `pnpm format && pnpm lint && (cd src-tauri && cargo fmt)`

- [ ] **Step 3: 푸시 + PR** — Run:

```bash
git push -u origin feat/112
gh pr create --base master --head feat/112 \
  --title "feat(notifications): 알림 원문 보기 + 글관리 배지 draft 제외" \
  --body "Closes #112 ..."
```

(PR 본문에 `Closes #112` 필수 — feat 브랜치 규칙. pallas-dev 리뷰어 REST API로 지정.)

---

## Self-Review

- **Spec coverage:** ① 원문 보기 = Task 1/2/5 ✅ ② 배지 draft 제외 = Task 4 ✅ ③ 스냅샷 방식 Ⓐ = Task 2 ✅ ④ TDD = 각 Task 실패테스트 먼저 ✅ ⑤ 하위호환(None 생략) = Task 1 Step 3 serde ✅.
- **No placeholders:** 모든 코드 블록 실제 코드.
- **Type consistency:** `body`/`comment`가 Rust(`Option<String>`)·바인딩(`?: string`)·수기타입(`?: string`)·렌더(`batch.body`)에서 일관.
- **Edge cases:** 빈 body/comment → None → 렌더 생략(Task5 조건부); 구버전 배치(원문 없음) → 제목만.
