# 실제 액션 기반 알림 피드 + 엑셀 입출력 — 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 알림 피드(activity + log-batches)의 가짜 seed를 제거하고 실제 액션이 피드를 채우게 하며, 알림·계정을 `.xlsx`로 내보내고 계정·게시글을 `.xlsx`로 가져온다.

**Architecture:** 모든 상태는 Rust `JsonStore<T>`에 있다. `time:String`(상대 문자열)을 `at:i64`(epoch ms)로 바꾸고 프론트에서 상대 시각을 포맷한다. 백엔드 커맨드 핸들러가 `activity::record(...)`로 활동을 기록하고, 포럼 게시는 `LogBatch`를 생성한다. 엑셀 입출력은 `rust_xlsxwriter`(쓰기)/`calamine`(읽기) + `tauri-plugin-dialog`(네이티브 경로 선택)로 Rust에서 처리한다.

**Tech Stack:** Rust(Tauri v2, ts-rs, serde), rust_xlsxwriter, calamine, tauri-plugin-dialog, React + Mantine + Vitest.

**스펙:** `docs/superpowers/specs/2026-06-04-real-notification-feed-and-excel-io-design.md` · **이슈:** #86 · **브랜치:** `feat/86`

**공통 명령**

- Rust 테스트: `cargo test --manifest-path src-tauri/Cargo.toml`
- 단일 Rust 테스트: `cargo test --manifest-path src-tauri/Cargo.toml <name> -- --nocapture`
- 바인딩 재생성: `pnpm gen:bindings`
- 프론트 테스트: `pnpm vitest run <path>`
- 전체 프론트: `pnpm vitest run`

---

## Phase A — 타임스탬프 기반

목표: `time:String`→`at:i64`, 상대시각 포맷 헬퍼, 알림 seed 제거. 완료 시 알림 피드는 **비어 있지만** 앱·테스트는 green.

### Task A1: 공유 `now_ms()` 유틸

**Files:**

- Create: `src-tauri/src/util.rs`
- Modify: `src-tauri/src/lib.rs` (모듈 선언 추가)

- [ ] **Step 1: 유틸 작성**

`src-tauri/src/util.rs`:

```rust
//! 작은 공유 유틸리티.
use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 시각을 epoch milliseconds로 반환. 시계 오류 시 0.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_ms_is_positive_and_post_2020() {
        // 2020-01-01 = 1_577_836_800_000 ms
        assert!(now_ms() > 1_577_836_800_000);
    }
}
```

- [ ] **Step 2: 모듈 등록**

`src-tauri/src/lib.rs`의 다른 `mod` 선언 옆에 추가:

```rust
mod util;
```

- [ ] **Step 3: 테스트**

Run: `cargo test --manifest-path src-tauri/Cargo.toml now_ms_is_positive`
Expected: PASS

- [ ] **Step 4: 커밋**

```bash
git add src-tauri/src/util.rs src-tauri/src/lib.rs
git commit -m "feat(util): add shared now_ms epoch-ms helper"
```

### Task A2: `ActivityItem.time` → `at`, seed 비우기

**Files:**

- Modify: `src-tauri/src/ipc/activity.rs`

- [ ] **Step 1: 테스트 먼저 수정**

`activity.rs`의 `#[cfg(test)] mod tests`를 아래로 교체:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_serializes_with_camelcase_at_and_lowercase_type() {
        let it = ActivityItem {
            id: "ac1".into(),
            r#type: ActivityType::Success,
            text: "테스트".into(),
            at: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&it).unwrap();
        assert!(json.contains("\"type\":\"success\""));
        assert!(json.contains("\"at\":1700000000000"));
        let back: ActivityItem = serde_json::from_str(&json).unwrap();
        assert_eq!(it, back);
    }

    #[test]
    fn seed_is_empty() {
        assert!(seed().is_empty());
    }
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::activity`
Expected: FAIL (필드 `at` 없음, `time` 사용 중)

- [ ] **Step 3: 구현**

`ActivityItem` 구조체에서 `pub time: String,` → `pub at: i64,`. `item(...)` 헬퍼와 `seed()`를 교체:

```rust
pub fn seed() -> Vec<ActivityItem> {
    Vec::new()
}
```

`fn item(...)` 헬퍼는 더 이상 쓰이지 않으면 삭제. (record는 Task B1에서 추가)

- [ ] **Step 4: 통과 확인 + 바인딩 재생성**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::activity
pnpm gen:bindings
```

Expected: 테스트 PASS, `src/shared/bindings/ActivityItem.ts`에 `at: number` 생성. (프론트 TS는 아직 깨질 수 있음 — Task A5~A7에서 수정)

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/activity.rs src/shared/bindings/ActivityItem.ts
git commit -m "feat(activity): replace relative time string with epoch-ms at, empty seed"
```

### Task A3: `LogBatch.time` → `at`, seed 비우기

**Files:**

- Modify: `src-tauri/src/ipc/log_batches.rs`

- [ ] **Step 1: 테스트 수정**

`log_batches.rs`의 테스트 모듈에서 `time:`을 쓰는 단언을 찾아 `at` 기반으로 바꾸고, seed 비움 테스트를 추가:

```rust
#[test]
fn seed_is_empty() {
    assert!(seed().is_empty());
}
```

기존에 seed 내용/`time` 문자열을 단언하던 테스트가 있으면 삭제하거나 위 테스트로 대체.

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::log_batches`
Expected: FAIL

- [ ] **Step 3: 구현**

`LogBatch` 구조체에서 `pub time: String,` → `pub at: i64,`. seed 빌더 함수들(`seed`, 보조 빌더)을 제거하고:

```rust
pub fn seed() -> Vec<LogBatch> {
    Vec::new()
}
```

보조 seed 빌더(forum destination 등)가 seed 전용이면 함께 삭제.

- [ ] **Step 4: 통과 + 바인딩**

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::log_batches
pnpm gen:bindings
```

Expected: PASS, `src/shared/bindings/LogBatch.ts`에 `at: number`.

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/log_batches.rs src/shared/bindings/LogBatch.ts
git commit -m "feat(log-batches): epoch-ms at field, empty seed"
```

### Task A4: 프론트 상대시각 헬퍼

**Files:**

- Modify: `src/shared/data/helpers.ts`
- Test: `src/shared/data/helpers.test.ts`

- [ ] **Step 1: 실패 테스트 추가**

`helpers.test.ts`에 추가:

```ts
import { describe, expect, it } from "vitest";

import { dayBucket, formatRelative } from "./helpers";

describe("formatRelative", () => {
  const now = 1_700_000_000_000;
  it("shows 방금 within a minute", () => {
    expect(formatRelative(now - 30_000, now)).toBe("방금");
  });
  it("shows minutes then hours", () => {
    expect(formatRelative(now - 5 * 60_000, now)).toBe("5분 전");
    expect(formatRelative(now - 3 * 3_600_000, now)).toBe("3시간 전");
  });
});

describe("dayBucket", () => {
  const now = new Date("2026-06-04T10:00:00").getTime();
  it("buckets today / yesterday / older", () => {
    expect(dayBucket(new Date("2026-06-04T08:00:00").getTime(), now)).toBe(
      "오늘",
    );
    expect(dayBucket(new Date("2026-06-03T23:00:00").getTime(), now)).toBe(
      "어제",
    );
    expect(dayBucket(new Date("2026-06-01T09:00:00").getTime(), now)).toBe(
      "이전",
    );
  });
});
```

- [ ] **Step 2: 실패 확인**

Run: `pnpm vitest run src/shared/data/helpers.test.ts`
Expected: FAIL (export 없음)

- [ ] **Step 3: 구현**

`helpers.ts`에 추가 (`now` 인자는 테스트용 기본값 `Date.now()`):

```ts
/** epoch-ms를 "방금/N분 전/N시간 전/어제 HH:mm/M월 D일"로 포맷. */
export function formatRelative(at: number, now: number = Date.now()): string {
  const diff = now - at;
  if (diff < 60_000) return "방금";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}분 전`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}시간 전`;
  const d = new Date(at);
  if (dayBucket(at, now) === "어제") {
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    return `어제 ${hh}:${mm}`;
  }
  return `${d.getMonth() + 1}월 ${d.getDate()}일`;
}

/** epoch-ms를 캘린더 날짜 기준 오늘/어제/이전으로 분류. */
export function dayBucket(
  at: number,
  now: number = Date.now(),
): "오늘" | "어제" | "이전" {
  const startOf = (ms: number) => {
    const d = new Date(ms);
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  };
  const today = startOf(now);
  const day = 86_400_000;
  const atDay = startOf(at);
  if (atDay >= today) return "오늘";
  if (atDay >= today - day) return "어제";
  return "이전";
}
```

- [ ] **Step 4: 통과 확인**

Run: `pnpm vitest run src/shared/data/helpers.test.ts`
Expected: PASS

- [ ] **Step 5: 커밋**

```bash
git add src/shared/data/helpers.ts src/shared/data/helpers.test.ts
git commit -m "feat(helpers): formatRelative + dayBucket on epoch-ms"
```

### Task A5: `notifications.tsx` — at 기반 시각

**Files:**

- Modify: `src/features/notifications/notifications.tsx`

- [ ] **Step 1: 구현**

1. 상단 import에 추가: `import { batchStatus, dayBucket, formatRelative } from "@/shared/data/helpers";` (기존 `batchStatus` import 라인에 병합)
2. 파일 내부의 `function dayBucket(time: string) { ... }` (29~33행)을 **삭제**.
3. `SystemRow` 인터페이스: `time: string;` → `at: number;`
4. activity 매핑(236~248행 부근)에서 `time: a.time,` → `at: a.at,`
5. `Row` 유니온의 `time: string` → `at: number` (batch/system 양쪽). batch 매핑에서 `time: b.time` → `at: b.at`, system 매핑에서 `time: s.time` → `at: s.at`.
6. 그룹핑에서 `dayBucket(row.time)` → `dayBucket(row.at)`.
7. batch 행 시각 표시 `{batch.time.replace(/^(오늘|어제)\s/, "")}` → `{formatRelative(batch.at)}`.
8. system 행 시각 표시 `{row.s.time}` → `{formatRelative(row.s.at)}`.

- [ ] **Step 2: 타입체크**

Run: `pnpm tsc --noEmit` (또는 `pnpm vitest run src/features/notifications`)
Expected: notifications 관련 타입 에러 없음

- [ ] **Step 3: 커밋**

```bash
git add src/features/notifications/notifications.tsx
git commit -m "refactor(notifications): render time from epoch-ms at"
```

### Task A6: `dashboard.tsx` — at 기반 시각

**Files:**

- Modify: `src/features/dashboard/dashboard.tsx`

- [ ] **Step 1: 구현**

1. import에 `formatRelative` 추가 (`@/shared/data/helpers`).
2. log-batch → row 매핑(약 60행)에서 `time: b.time,` → `time: formatRelative(b.at),`
3. activity 직접 표시(약 228행) `{a.time}` → activity row가 `ActivityItem`이면 `{formatRelative(a.at)}`. (activity를 매핑하는 지점에서 `time` 필드를 만들면 거기서 `formatRelative(a.at)`로.)

> 주: dashboard가 activity/logBatch를 어떤 중간 타입으로 변환하는지에 맞춰 `time` 생성 지점을 `formatRelative(at)`로 바꾼다. 최종적으로 화면에 출력되는 문자열이 `formatRelative` 결과여야 한다.

- [ ] **Step 2: 타입체크**

Run: `pnpm tsc --noEmit`
Expected: dashboard 관련 에러 없음

- [ ] **Step 3: 커밋**

```bash
git add src/features/dashboard/dashboard.tsx
git commit -m "refactor(dashboard): render time from epoch-ms at"
```

### Task A7: mock 백엔드 seed `time`→`at`

**Files:**

- Modify: `src/test/ipc.ts`

- [ ] **Step 1: 구현**

`src/test/ipc.ts`에서 activity/log-batch seed의 모든 `time: "..."` 항목(320~796행 영역)을 `at: <epoch-ms>`로 교체. 결정적(deterministic) 값 사용 — 고정 기준 `BASE = 1_700_000_000_000`을 파일 상단 근처에 선언하고 상대 오프셋으로:

```ts
const NOW_BASE = 1_700_000_000_000;
// 예: "12분 전" → at: NOW_BASE - 12 * 60_000
//     "1시간 전" → at: NOW_BASE - 3_600_000
//     "어제"     → at: NOW_BASE - 26 * 3_600_000
```

각 seed 객체의 `time` 키를 `at` 숫자로 바꾼다 (activity 5건, log-batch 7건). 테스트가 `formatRelative`로 표시되는 문자열을 단언한다면 `now` 인자를 `NOW_BASE` 부근으로 주입하도록 테스트도 조정.

- [ ] **Step 2: 전체 프론트 테스트**

Run: `pnpm vitest run`
Expected: PASS (시각 문자열을 직접 단언하던 기존 테스트는 `formatRelative`/`dayBucket` 기준으로 수정)

- [ ] **Step 3: 커밋**

```bash
git add src/test/ipc.ts src/features
git commit -m "test(ipc-mock): migrate activity/log-batch seeds to epoch-ms at"
```

### Task A8: Phase A 체크포인트

- [ ] **Step 1: 전체 스위트**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm vitest run
```

Expected: 모두 PASS. 알림 화면은 빈 상태(데모 없음)지만 렌더/필터 정상.

---

## Phase B — 액션 recorder + 계측

목표: `record()` + 범용 `append_activity` 커맨드 + 백엔드 핸들러 계측 + 로그인 로깅 + 포럼 게시→LogBatch.

### Task B1: `activity::record()` + 보관 상한

**Files:**

- Modify: `src-tauri/src/ipc/activity.rs`

- [ ] **Step 1: 실패 테스트**

`activity.rs` 테스트 모듈에 추가:

```rust
#[test]
fn record_prepends_newest_first_and_caps_at_500() {
    let dir = std::env::temp_dir().join("pstmacro_activity_record_test");
    let _ = std::fs::remove_dir_all(&dir);
    let store = JsonStore::<ActivityItem>::load_or_seed(dir.join("a.json"), Vec::new());
    for i in 0..520 {
        record(&store, ActivityType::Info, format!("evt {i}"));
    }
    let items = store.snapshot();
    assert_eq!(items.len(), 500); // capped
    assert_eq!(items[0].text, "evt 519"); // newest first
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::activity::tests::record_prepends`
Expected: FAIL (`record` 미정의)

- [ ] **Step 3: 구현**

`activity.rs`에 추가:

```rust
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_ACTIVITY: usize = 500;

fn gen_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("ac-{}-{}", crate::util::now_ms(), n)
}

/// 새 활동을 맨 앞에 추가하고 최신 500건만 유지(영속).
pub fn record(store: &JsonStore<ActivityItem>, ty: ActivityType, text: impl Into<String>) {
    let entry = ActivityItem {
        id: gen_id(),
        r#type: ty,
        text: text.into(),
        at: crate::util::now_ms(),
    };
    store.mutate(|mut items| {
        items.insert(0, entry.clone());
        items.truncate(MAX_ACTIVITY);
        items
    });
}
```

- [ ] **Step 4: 통과**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::activity`
Expected: PASS

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/activity.rs
git commit -m "feat(activity): record() prepend + 500-entry cap"
```

### Task B2: `append_activity` 커맨드 + IPC 노출 + mock

**Files:**

- Modify: `src-tauri/src/lib.rs` (커맨드 + register), `src/shared/ipc/index.ts`, `src/test/ipc.ts`

- [ ] **Step 1: 커맨드 작성**

`lib.rs`에 추가 (활동 타입 문자열을 받아 enum 매핑):

```rust
#[tauri::command]
fn append_activity(
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    kind: String,
    text: String,
) {
    use ipc::activity::ActivityType;
    let ty = match kind.as_str() {
        "success" => ActivityType::Success,
        "error" => ActivityType::Error,
        _ => ActivityType::Info,
    };
    ipc::activity::record(activity.inner(), ty, text);
}
```

`register_handlers`의 `generate_handler![...]`에 `append_activity,` 추가. (상단 `use crate::store::JsonStore;` 존재 확인 — 없으면 추가.)

- [ ] **Step 2: IPC 래퍼**

`src/shared/ipc/index.ts`의 `activity` 그룹을 확장:

```ts
activity: {
  list: () => call<ActivityItem[]>("list_activity"),
  append: (kind: "success" | "error" | "info", text: string) =>
    call<void>("append_activity", { kind, text }),
},
```

- [ ] **Step 3: mock 핸들러**

`src/test/ipc.ts` dispatch switch에 추가 (in-memory activity 배열 맨 앞에 prepend):

```ts
case "append_activity": {
  state.activity.unshift({
    id: `ac-${state.activity.length}`,
    type: args.kind as "success" | "error" | "info",
    text: args.text as string,
    at: NOW_BASE,
  });
  return undefined;
}
```

(`state.activity`의 실제 식별자/구조에 맞춰 조정.)

- [ ] **Step 4: 검증**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm vitest run
```

Expected: PASS

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/lib.rs src/shared/ipc/index.ts src/test/ipc.ts
git commit -m "feat(ipc): append_activity command + wrapper + mock"
```

### Task B3: 계정 커맨드 계측 (add/update/delete)

**Files:**

- Modify: `src-tauri/src/ipc/accounts.rs`

- [ ] **Step 1: 통합 테스트 (활동 누적 확인)**

`accounts.rs` 테스트 모듈에 — `record`가 임의 스토어에 쓰는지 확인하는 식의 헬퍼 테스트는 B1에서 커버됨. 여기서는 메시지 빌더를 순수 함수로 추출해 단언:

```rust
#[test]
fn account_event_messages() {
    assert_eq!(added_msg("invest_king7"), "계정 invest_king7 추가됨");
    assert_eq!(updated_msg("invest_king7"), "계정 invest_king7 수정됨");
    assert_eq!(deleted_msg(3), "계정 3건 삭제됨");
}
```

- [ ] **Step 2: 실패 확인**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::accounts::tests::account_event_messages`
Expected: FAIL

- [ ] **Step 3: 구현**

`accounts.rs`에 순수 메시지 빌더 추가:

```rust
pub fn added_msg(login_id: &str) -> String { format!("계정 {login_id} 추가됨") }
pub fn updated_msg(login_id: &str) -> String { format!("계정 {login_id} 수정됨") }
pub fn deleted_msg(n: usize) -> String { format!("계정 {n}건 삭제됨") }
```

커맨드 3개에 `activity` State 인자를 추가하고 mutate 후 record:

```rust
use crate::ipc::activity::{record, ActivityType};

#[tauri::command]
pub fn add_account(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    account: Account,
) -> Vec<Account> {
    let login = account.login_id.clone();
    let next = store.mutate(|accounts| apply_add(accounts, account));
    record(activity.inner(), ActivityType::Success, added_msg(&login));
    next
}

#[tauri::command]
pub fn update_account(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    account: Account,
) -> Vec<Account> {
    let login = account.login_id.clone();
    let next = store.mutate(|accounts| apply_update(accounts, account));
    record(activity.inner(), ActivityType::Info, updated_msg(&login));
    next
}

#[tauri::command]
pub fn delete_accounts(
    store: tauri::State<'_, JsonStore<Account>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    ids: Vec<String>,
) -> Vec<Account> {
    let n = ids.len();
    let next = store.mutate(|accounts| apply_delete(accounts, &ids));
    record(activity.inner(), ActivityType::Info, deleted_msg(n));
    next
}
```

- [ ] **Step 4: 통과**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::accounts`
Expected: PASS (Tauri는 State를 타입으로 주입하므로 invoke 호출부는 변경 불필요)

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/accounts.rs
git commit -m "feat(accounts): log add/update/delete to activity feed"
```

### Task B4: 게시글 커맨드 계측 (upsert 성공/실패, delete)

**Files:**

- Modify: `src-tauri/src/ipc/posts.rs`

- [ ] **Step 1: 메시지 빌더 테스트**

```rust
#[test]
fn post_event_messages() {
    assert_eq!(saved_msg("실적 정리"), "게시글 '실적 정리' 저장됨");
    assert_eq!(deleted_msg("실적 정리"), "게시글 '실적 정리' 삭제됨");
}
```

- [ ] **Step 2: 실패 확인** — `cargo test ... ipc::posts::tests::post_event_messages` → FAIL

- [ ] **Step 3: 구현**

```rust
use crate::ipc::activity::{record, ActivityType};

pub fn saved_msg(title: &str) -> String { format!("게시글 '{title}' 저장됨") }
pub fn deleted_msg(title: &str) -> String { format!("게시글 '{title}' 삭제됨") }
```

`upsert_post`/`delete_post`에 `activity` State 추가. upsert는 성공 경로에서 `ActivityType::Success`로 `saved_msg(&title)` 기록. (현재 구조상 upsert는 실패하지 않지만, 스펙의 "저장 실패" 로깅은 입력 검증 실패를 의미 — 제목이 공백이면 실패로 기록하고 저장 생략:)

```rust
#[tauri::command]
pub fn upsert_post(
    store: tauri::State<'_, JsonStore<LibraryPost>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    post: LibraryPost,
) -> Vec<LibraryPost> {
    if post.title.trim().is_empty() {
        record(activity.inner(), ActivityType::Error, "게시글 저장 실패 — 제목이 비어 있습니다");
        return store.snapshot();
    }
    let title = post.title.clone();
    let next = store.mutate(|posts| apply_upsert(posts, post));
    record(activity.inner(), ActivityType::Success, saved_msg(&title));
    next
}

#[tauri::command]
pub fn delete_post(
    store: tauri::State<'_, JsonStore<LibraryPost>>,
    activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,
    id: String,
) -> Vec<LibraryPost> {
    let title = store.snapshot().into_iter().find(|p| p.id == id).map(|p| p.title).unwrap_or_default();
    let next = store.mutate(|posts| apply_delete(posts, &id));
    record(activity.inner(), ActivityType::Info, deleted_msg(&title));
    next
}
```

- [ ] **Step 4: 통과** — `cargo test ... --lib ipc::posts` → PASS
- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/posts.rs
git commit -m "feat(posts): log save success/failure and delete to activity"
```

### Task B5: 큐 커맨드 계측

**Files:**

- Modify: `src-tauri/src/ipc/queue.rs`

- [ ] **Step 1: 구현 (단순 wiring — record 호출만 추가)**

`queue.rs` 상단에 `use crate::ipc::activity::{record, ActivityType};`. 각 커맨드에 `activity` State 추가 후:

- `add_queue_scheduled`: 성공(Ok) 직전 `record(activity.inner(), ActivityType::Info, format!("예약 추가됨 — {}", item.title));` (item.title은 move 전에 clone).
- `cancel_queue_scheduled`: `record(activity.inner(), ActivityType::Info, "예약 취소됨");`
- `cancel_queue_now`: `record(activity.inner(), ActivityType::Info, "진행 작업 취소됨");`
- `promote_queue_scheduled`: 승격 성공 시 `record(activity.inner(), ActivityType::Info, "예약을 즉시 게시로 전환");`

각 커맨드 시그니처에 `activity: tauri::State<'_, JsonStore<crate::ipc::activity::ActivityItem>>,` 추가.

- [ ] **Step 2: 빌드/테스트**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::queue`
Expected: PASS (순수 로직 테스트 불변)

- [ ] **Step 3: 커밋**

```bash
git add src-tauri/src/ipc/queue.rs
git commit -m "feat(queue): log schedule/cancel/promote actions to activity"
```

### Task B6: 로그인 결과 로깅 (auth worker)

**Files:**

- Modify: `src-tauri/src/auth/queue.rs`

- [ ] **Step 1: 구현**

`worker_loop`에서 terminal `status`를 계산한 직후(약 138~152행, `existing.status = status.clone();` 부근), `app`을 통해 activity 스토어에 기록. `app: AppHandle<R>`가 이미 있으므로:

```rust
use crate::ipc::activity::{record, ActivityType, ActivityItem};
use crate::store::JsonStore;

// status / account_id / message 확정 후:
let activity = app.state::<JsonStore<ActivityItem>>();
let (ty, msg) = match status {
    QueueJobStatus::Success | QueueJobStatus::Expired => (
        ActivityType::Success,
        format!("계정 {account_id} 로그인 성공"),
    ),
    _ => (
        ActivityType::Error,
        format!("계정 {account_id} 로그인 실패 — {message}"),
    ),
};
record(activity.inner(), ty, msg);
```

`account_id`/`message`의 실제 변수명에 맞춰 사용. `use tauri::Manager;`가 필요하면 추가(`app.state` 사용 위해).

- [ ] **Step 2: 빌드**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 성공

- [ ] **Step 3: 통합 테스트로 확인 (선택)**

기존 IPC 통합 테스트 디렉터리에 worker가 activity를 남기는지 확인하는 테스트가 어려우면 빌드 확인으로 갈음.

- [ ] **Step 4: 커밋**

```bash
git add src-tauri/src/auth/queue.rs
git commit -m "feat(auth): log login success/failure to activity feed"
```

### Task B7: 포럼 게시 → LogBatch

**Files:**

- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 순수 변환 함수 + 테스트**

`lib.rs`에 결과→LogBatch 빌더를 순수 함수로 추가하고 테스트:

```rust
fn build_publish_batch(
    title: &str,
    run_post: bool,
    run_comment: bool,
    account_id: &str,
    at: i64,
    results: &[ForumPublishResult],
) -> ipc::log_batches::LogBatch {
    use ipc::accounts::PlatformId;
    use ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch};
    use ipc::posts::ModeValue;
    let kind = if run_post && run_comment {
        ModeValue::Both
    } else if run_comment {
        ModeValue::Comment
    } else {
        ModeValue::Post
    };
    let items = results
        .iter()
        .map(|r| BatchItem {
            platform: PlatformId::Forum,
            target: r.name.clone(),
            code: Some(r.code.clone()),
            board: None,
            login_id: account_id.to_owned(),
            status: if r.ok { BatchItemStatus::Success } else { BatchItemStatus::Fail },
            msg: r.message.clone(),
            trace: if r.ok { None } else { Some(r.message.clone()) },
        })
        .collect();
    LogBatch {
        id: format!("lb-{at}"),
        title: title.to_owned(),
        kind,
        at,
        state: None,
        items,
    }
}
```

테스트:

```rust
#[test]
fn build_publish_batch_maps_results_to_items() {
    let results = vec![
        ForumPublishResult { code: "005930".into(), name: "삼성전자".into(), ok: true, message: "게시 완료".into() },
        ForumPublishResult { code: "000660".into(), name: "SK하이닉스".into(), ok: false, message: "로그인 만료".into() },
    ];
    let b = build_publish_batch("실적 정리", true, false, "invest_king7", 1_700_000_000_000, &results);
    assert_eq!(b.items.len(), 2);
    assert_eq!(b.title, "실적 정리");
    assert!(matches!(b.kind, ipc::posts::ModeValue::Post));
    assert!(matches!(b.items[0].status, ipc::log_batches::BatchItemStatus::Success));
    assert_eq!(b.items[1].trace.as_deref(), Some("로그인 만료"));
}
```

- [ ] **Step 2: 실패 확인** — `cargo test ... build_publish_batch_maps` → FAIL
- [ ] **Step 3: 구현 — 빌더 추가 + 커맨드에서 저장**

`run_forum_publish_now`에서 결과 수신 후 LogBatch를 만들어 store에 prepend + activity 기록. `request`가 `spawn_blocking`으로 move되므로 필요한 필드를 먼저 clone:

```rust
#[tauri::command]
async fn run_forum_publish_now<R: Runtime>(
    app: tauri::AppHandle<R>,
    request: ForumPublishRequest,
) -> Result<Vec<ForumPublishResult>, String> {
    use tauri::Manager;
    let title = request.title.clone();
    let account_id = request.account_id.clone();
    let (run_post, run_comment) = (request.run_post, request.run_comment);
    let app_for_job = app.clone();
    let results = tauri::async_runtime::spawn_blocking(move || run_forum_publish(request, app_for_job))
        .await
        .map_err(|error| format!("게시 실행 스레드 오류: {error}"))?;

    let at = util::now_ms();
    let batch = build_publish_batch(&title, run_post, run_comment, &account_id, at, &results);
    let ok = results.iter().filter(|r| r.ok).count();
    let logs = app.state::<JsonStore<ipc::log_batches::LogBatch>>();
    logs.mutate(|mut v| { v.insert(0, batch); v });
    let activity = app.state::<JsonStore<ipc::activity::ActivityItem>>();
    ipc::activity::record(
        activity.inner(),
        if ok == results.len() { ipc::activity::ActivityType::Success } else { ipc::activity::ActivityType::Error },
        format!("'{title}' 게시 — {}곳 중 {ok}곳 성공", results.len()),
    );
    Ok(results)
}
```

필요한 `use crate::store::JsonStore;`가 lib.rs에 없으면 추가.

- [ ] **Step 4: 통과**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(forum): record publish results as a LogBatch + activity"
```

### Task B8: 크롤링 로깅 (프론트 → append_activity)

**Files:**

- Modify: `src/features/posts/stock-crawl-modal.tsx`
- Test: `src/features/posts/stock-crawl-modal.test.tsx`

- [ ] **Step 1: 실패 테스트**

크롤 완료(`phase === "done"`) 시 `ipc.activity.append("info", \`종목 N개 크롤링\`)`가 호출됨을 단언하는 테스트 추가 (`ipc.activity.append`를 vi.spyOn).

```ts
it("logs an activity when crawl completes", async () => {
  const spy = vi.spyOn(ipc.activity, "append");
  render(<StockCrawlModal /* 필요한 props */ />);
  // 크롤 완료까지 대기
  await screen.findByText(/가져오기|완료/);
  expect(spy).toHaveBeenCalledWith("info", expect.stringContaining("종목"));
});
```

- [ ] **Step 2: 실패 확인** — `pnpm vitest run src/features/posts/stock-crawl-modal.test.tsx` → FAIL
- [ ] **Step 3: 구현**

`stock-crawl-modal.tsx`에서 `ipc.stocks.list().then(setStocks)`로 크롤이 끝나 `phase`가 `"done"`이 될 때 한 번 `void ipc.activity.append("info", \`종목 ${list.length}개 크롤링\`);` 호출. 중복 로깅 방지 위해 최초 완료 시 1회만.

- [ ] **Step 4: 통과** — `pnpm vitest run src/features/posts/stock-crawl-modal.test.tsx` → PASS
- [ ] **Step 5: 커밋**

```bash
git add src/features/posts/stock-crawl-modal.tsx src/features/posts/stock-crawl-modal.test.tsx
git commit -m "feat(crawl): log stock crawl completion to activity"
```

### Task B9: Phase B 체크포인트 + 통합 테스트

**Files:**

- Modify: 기존 IPC 통합 테스트 (`src-tauri/tests/` 하위)

- [ ] **Step 1: 통합 테스트 추가**

`add_account` invoke 후 `list_activity`가 1건 증가하고 텍스트가 `추가됨`을 포함하는지 단언하는 테스트를 기존 통합 테스트 파일에 추가 (Tauri mock runtime + manage_stores 사용 패턴 그대로).

- [ ] **Step 2: 전체 스위트**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm vitest run
```

Expected: PASS

- [ ] **Step 3: 커밋**

```bash
git add -A
git commit -m "test(ipc): assert mutations append to activity feed"
```

---

## Phase C — 엑셀 내보내기 (알림 + 계정)

### Task C1: 의존성 + dialog 플러그인

**Files:**

- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`, `package.json`

- [ ] **Step 1: Rust 의존성**

`src-tauri/Cargo.toml` `[dependencies]`에 추가:

```toml
rust_xlsxwriter = "0.79"
calamine = "0.26"
tauri-plugin-dialog = "2"
```

- [ ] **Step 2: 플러그인 init**

`lib.rs`의 `run()`에서 빌더 체인에 `.plugin(tauri_plugin_dialog::init())` 추가 (`register_handlers(...)` 전후 적절한 위치).

- [ ] **Step 3: capabilities**

`src-tauri/capabilities/default.json`의 `permissions` 배열에 추가:

```json
"dialog:allow-save",
"dialog:allow-open"
```

- [ ] **Step 4: 프론트 플러그인**

```bash
pnpm add @tauri-apps/plugin-dialog
```

- [ ] **Step 5: 빌드 확인**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 성공 (의존성 다운로드/컴파일)

- [ ] **Step 6: 커밋**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/capabilities/default.json package.json pnpm-lock.yaml
git commit -m "build: add xlsx (rust_xlsxwriter/calamine) + dialog plugin deps"
```

### Task C2: `export_accounts_xlsx`

**Files:**

- Create: `src-tauri/src/ipc/excel.rs`
- Modify: `src-tauri/src/ipc/mod.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: 실패 테스트**

`excel.rs` 작성 시작 + 테스트 (임시 파일에 쓰고 calamine으로 재읽기 round-trip):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::accounts::{Account, AccountStatus, PlatformId};

    fn acct(login: &str) -> Account {
        Account {
            id: login.into(), platform: PlatformId::Forum, login_id: login.into(),
            pw: "pw123".into(), status: AccountStatus::Active, last: "—".into(),
            tags: vec!["반도체".into(), "대형주".into()],
        }
    }

    #[test]
    fn accounts_roundtrip_through_xlsx() {
        let dir = std::env::temp_dir().join("pstmacro_xlsx_acct");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("acc.xlsx");
        write_accounts_xlsx(path.to_str().unwrap(), &[acct("invest_king7")]).unwrap();

        use calamine::{open_workbook, Reader, Xlsx};
        let mut wb: Xlsx<_> = open_workbook(&path).unwrap();
        let range = wb.worksheet_range("계정").unwrap();
        let rows: Vec<_> = range.rows().collect();
        assert_eq!(rows[0][0].to_string(), "loginId");
        assert_eq!(rows[1][0].to_string(), "invest_king7");
        assert_eq!(rows[1][1].to_string(), "pw123"); // pw 포함
        assert_eq!(rows[1][2].to_string(), "forum");
        assert!(rows[1][4].to_string().contains("반도체")); // tags
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test ... write_accounts` / `accounts_roundtrip` → FAIL
- [ ] **Step 3: 구현**

`excel.rs`:

```rust
//! 엑셀(.xlsx) 입출력 — Rust에서 워크북 생성(rust_xlsxwriter)/파싱(calamine).
use rust_xlsxwriter::Workbook;

use crate::ipc::accounts::Account;

fn platform_str(p: &crate::ipc::accounts::PlatformId) -> &'static str {
    use crate::ipc::accounts::PlatformId::*;
    match p { Forum => "forum", Naver => "naver", Band => "band", Instagram => "instagram", Threads => "threads" }
}
fn status_str(s: &crate::ipc::accounts::AccountStatus) -> &'static str {
    use crate::ipc::accounts::AccountStatus::*;
    match s { New => "new", Active => "active", Error => "error" }
}

/// 계정 목록을 "계정" 시트로 기록. pw 포함(백업/재가져오기 대칭).
pub fn write_accounts_xlsx(path: &str, accounts: &[Account]) -> Result<(), String> {
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet().set_name("계정").map_err(|e| e.to_string())?;
    let headers = ["loginId", "pw", "platform", "status", "tags", "last"];
    for (c, h) in headers.iter().enumerate() {
        sheet.write_string(0, c as u16, *h).map_err(|e| e.to_string())?;
    }
    for (r, a) in accounts.iter().enumerate() {
        let row = (r + 1) as u32;
        sheet.write_string(row, 0, &a.login_id).map_err(|e| e.to_string())?;
        sheet.write_string(row, 1, &a.pw).map_err(|e| e.to_string())?;
        sheet.write_string(row, 2, platform_str(&a.platform)).map_err(|e| e.to_string())?;
        sheet.write_string(row, 3, status_str(&a.status)).map_err(|e| e.to_string())?;
        sheet.write_string(row, 4, &a.tags.join(",")).map_err(|e| e.to_string())?;
        sheet.write_string(row, 5, &a.last).map_err(|e| e.to_string())?;
    }
    wb.save(path).map_err(|e| e.to_string())
}
```

`mod.rs`에 `pub mod excel;` 추가. `lib.rs`에 커맨드 + register:

```rust
#[tauri::command]
fn export_accounts_xlsx(
    store: tauri::State<'_, JsonStore<ipc::accounts::Account>>,
    path: String,
) -> Result<(), String> {
    ipc::excel::write_accounts_xlsx(&path, &store.snapshot())
}
```

`generate_handler![...]`에 `export_accounts_xlsx,` 추가.

- [ ] **Step 4: 통과** — `cargo test ... accounts_roundtrip_through_xlsx` → PASS
- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/excel.rs src-tauri/src/ipc/mod.rs src-tauri/src/lib.rs
git commit -m "feat(excel): export accounts to xlsx (pw included)"
```

### Task C3: `export_activity_xlsx`

**Files:**

- Modify: `src-tauri/src/ipc/excel.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: 실패 테스트**

`excel.rs` 테스트에 추가 — 두 시트("게시 배치", "시스템 활동")가 생성되고 헤더/행이 맞는지 round-trip 단언. log-batch 1건(items 2개) + activity 1건 입력.

```rust
#[test]
fn activity_log_roundtrip_two_sheets() {
    use crate::ipc::activity::{ActivityItem, ActivityType};
    use crate::ipc::log_batches::{BatchItem, BatchItemStatus, LogBatch};
    use crate::ipc::posts::ModeValue;
    use crate::ipc::accounts::PlatformId;
    let dir = std::env::temp_dir().join("pstmacro_xlsx_act");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("act.xlsx");
    let batch = LogBatch {
        id: "lb1".into(), title: "실적 정리".into(), kind: ModeValue::Post,
        at: 1_700_000_000_000, state: None,
        items: vec![BatchItem { platform: PlatformId::Forum, target: "삼성전자".into(),
            code: Some("005930".into()), board: None, login_id: "invest_king7".into(),
            status: BatchItemStatus::Success, msg: "게시 완료".into(), trace: None }],
    };
    let act = ActivityItem { id: "ac1".into(), r#type: ActivityType::Info, text: "종목 12개 크롤링".into(), at: 1_700_000_000_000 };
    write_activity_xlsx(path.to_str().unwrap(), &[batch], &[act]).unwrap();

    use calamine::{open_workbook, Reader, Xlsx};
    let mut wb: Xlsx<_> = open_workbook(&path).unwrap();
    assert!(wb.worksheet_range("게시 배치").is_ok());
    let sys = wb.worksheet_range("시스템 활동").unwrap();
    let rows: Vec<_> = sys.rows().collect();
    assert_eq!(rows[1][2].to_string(), "종목 12개 크롤링");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: 실패 확인** → FAIL
- [ ] **Step 3: 구현**

`excel.rs`에 추가 (`at`는 사람이 읽는 문자열로: `ms_to_local_string(at)` 보조 — 간단히 `chrono` 없이 `format!`로 ISO 비슷하게, 또는 epoch 그대로 숫자). 간단히 epoch ms를 `YYYY-MM-DD HH:MM` 로컬 문자열로 변환하는 보조를 직접 구현하거나, 우선 epoch 숫자 문자열로 기록(테스트는 텍스트 컬럼만 단언):

```rust
use crate::ipc::activity::ActivityItem;
use crate::ipc::log_batches::LogBatch;

fn activity_type_str(t: &crate::ipc::activity::ActivityType) -> &'static str {
    use crate::ipc::activity::ActivityType::*;
    match t { Success => "성공", Error => "실패", Info => "정보" }
}
fn item_status_str(s: &crate::ipc::log_batches::BatchItemStatus) -> &'static str {
    use crate::ipc::log_batches::BatchItemStatus::*;
    match s { Success => "성공", Fail => "실패", Running => "처리중", Waiting => "대기" }
}

pub fn write_activity_xlsx(path: &str, batches: &[LogBatch], activity: &[ActivityItem]) -> Result<(), String> {
    let mut wb = Workbook::new();
    // 시트 ① 게시 배치 (flatten)
    let s1 = wb.add_worksheet().set_name("게시 배치").map_err(|e| e.to_string())?;
    let h1 = ["시각(ms)", "제목", "플랫폼", "대상", "코드", "계정", "상태", "메시지"];
    for (c, h) in h1.iter().enumerate() { s1.write_string(0, c as u16, *h).map_err(|e| e.to_string())?; }
    let mut row = 1u32;
    for b in batches {
        for it in &b.items {
            s1.write_number(row, 0, b.at as f64).map_err(|e| e.to_string())?;
            s1.write_string(row, 1, &b.title).map_err(|e| e.to_string())?;
            s1.write_string(row, 2, platform_str(&it.platform)).map_err(|e| e.to_string())?;
            s1.write_string(row, 3, &it.target).map_err(|e| e.to_string())?;
            s1.write_string(row, 4, it.code.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
            s1.write_string(row, 5, &it.login_id).map_err(|e| e.to_string())?;
            s1.write_string(row, 6, item_status_str(&it.status)).map_err(|e| e.to_string())?;
            s1.write_string(row, 7, &it.msg).map_err(|e| e.to_string())?;
            row += 1;
        }
    }
    // 시트 ② 시스템 활동
    let s2 = wb.add_worksheet().set_name("시스템 활동").map_err(|e| e.to_string())?;
    let h2 = ["시각(ms)", "유형", "내용"];
    for (c, h) in h2.iter().enumerate() { s2.write_string(0, c as u16, *h).map_err(|e| e.to_string())?; }
    for (r, a) in activity.iter().enumerate() {
        let rr = (r + 1) as u32;
        s2.write_number(rr, 0, a.at as f64).map_err(|e| e.to_string())?;
        s2.write_string(rr, 1, activity_type_str(&a.r#type)).map_err(|e| e.to_string())?;
        s2.write_string(rr, 2, &a.text).map_err(|e| e.to_string())?;
    }
    wb.save(path).map_err(|e| e.to_string())
}
```

`platform_str`는 C2에서 정의됨(같은 파일). lib.rs 커맨드 + register:

```rust
#[tauri::command]
fn export_activity_xlsx(
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    logs: tauri::State<'_, JsonStore<ipc::log_batches::LogBatch>>,
    path: String,
) -> Result<(), String> {
    ipc::excel::write_activity_xlsx(&path, &logs.snapshot(), &activity.snapshot())
}
```

`generate_handler![...]`에 `export_activity_xlsx,` 추가.

- [ ] **Step 4: 통과** → PASS
- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/excel.rs src-tauri/src/lib.rs
git commit -m "feat(excel): export activity + log batches to two-sheet xlsx"
```

### Task C4: 프론트 IPC 래퍼 + mock

**Files:**

- Modify: `src/shared/ipc/index.ts`, `src/test/ipc.ts`

- [ ] **Step 1: 래퍼 추가**

`index.ts`에 `excel` 그룹 추가:

```ts
excel: {
  exportAccounts: (path: string) => call<void>("export_accounts_xlsx", { path }),
  exportActivity: (path: string) => call<void>("export_activity_xlsx", { path }),
},
```

- [ ] **Step 2: mock 핸들러**

`src/test/ipc.ts` dispatch에 추가 (실제 파일 쓰기 없이 성공 반환):

```ts
case "export_accounts_xlsx":
case "export_activity_xlsx":
  return undefined;
```

- [ ] **Step 3: 테스트** — `pnpm vitest run` → PASS
- [ ] **Step 4: 커밋**

```bash
git add src/shared/ipc/index.ts src/test/ipc.ts
git commit -m "feat(ipc): excel export wrappers + mock"
```

### Task C5: 내보내기 버튼 (알림 + 계정)

**Files:**

- Modify: `src/features/notifications/notifications.tsx`, `src/features/accounts/accounts.tsx`
- Test: 각 `*.test.tsx`

- [ ] **Step 1: 공용 save 헬퍼 + 실패 테스트**

`notifications.test.tsx`: "내보내기" 클릭 시 dialog `save`가 호출되고 경로가 있으면 `ipc.excel.exportActivity(path)`가 불리는지 단언 (`@tauri-apps/plugin-dialog`의 `save`를 vi.mock).

- [ ] **Step 2: 실패 확인** → FAIL
- [ ] **Step 3: 구현**

`notifications.tsx`의 "내보내기" 버튼 onClick을 교체:

```ts
import { save } from "@tauri-apps/plugin-dialog";
// ...
onClick={async () => {
  const path = await save({ defaultPath: "알림.xlsx", filters: [{ name: "Excel", extensions: ["xlsx"] }] });
  if (!path) return; // 취소
  await ipc.excel.exportActivity(path);
  notifications.show({ message: "알림 내역을 엑셀로 내보냈어요", color: "green" });
}}
```

`accounts.tsx` 헤더 Group에 "내보내기" 버튼 추가 → `save({ defaultPath: "계정.xlsx", ... })` → `ipc.excel.exportAccounts(path)` → 토스트. (notifications의 기존 버튼 스타일 `variant="default"` + `Icon.download` 재사용.)

- [ ] **Step 4: 통과** — `pnpm vitest run src/features/notifications src/features/accounts` → PASS
- [ ] **Step 5: 커밋**

```bash
git add src/features/notifications/notifications.tsx src/features/accounts/accounts.tsx src/features/notifications/notifications.test.tsx src/features/accounts/accounts.test.tsx
git commit -m "feat(ui): real xlsx export for notifications and accounts"
```

### Task C6: Phase C 체크포인트

- [ ] **Step 1:** `cargo test --manifest-path src-tauri/Cargo.toml` + `pnpm vitest run` → 모두 PASS

---

## Phase D — 엑셀 가져오기 (계정 + 게시글)

### Task D1: `ImportSummary` 타입 + 바인딩

**Files:**

- Modify: `src-tauri/src/ipc/excel.rs`

- [ ] **Step 1: 타입 정의 + 테스트**

```rust
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../src/shared/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    #[test]
    fn summary_camelcase() {
        let s = ImportSummary { imported: 3, skipped: 1, errors: vec!["bad row".into()] };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"imported\":3"));
    }
}
```

- [ ] **Step 2: 통과 + 바인딩**

```bash
cargo test --manifest-path src-tauri/Cargo.toml summary_camelcase
pnpm gen:bindings
```

Expected: PASS, `src/shared/bindings/ImportSummary.ts` 생성.

- [ ] **Step 3: 커밋**

```bash
git add src-tauri/src/ipc/excel.rs src/shared/bindings/ImportSummary.ts
git commit -m "feat(excel): ImportSummary type + bindings"
```

### Task D2: `import_accounts_xlsx`

**Files:**

- Modify: `src-tauri/src/ipc/excel.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: 실패 테스트 (round-trip: write→import)**

```rust
#[test]
fn import_accounts_validates_and_merges() {
    let dir = std::env::temp_dir().join("pstmacro_imp_acct");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("in.xlsx");
    // 헤더 + 2 valid + 1 invalid(빈 pw)
    {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let s = wb.add_worksheet().set_name("계정").unwrap();
        for (c, h) in ["loginId", "pw", "platform", "tags"].iter().enumerate() {
            s.write_string(0, c as u16, *h).unwrap();
        }
        s.write_string(1, 0, "new_user").unwrap(); s.write_string(1, 1, "pw1").unwrap(); s.write_string(1, 2, "forum").unwrap(); s.write_string(1, 3, "반도체,대형주").unwrap();
        s.write_string(2, 0, "no_pw").unwrap(); s.write_string(2, 2, "naver").unwrap(); // pw 없음 → skip
        wb.save(&path).unwrap();
    }
    let existing = vec![];
    let (next, summary) = import_accounts(path.to_str().unwrap(), existing).unwrap();
    assert_eq!(summary.imported, 1);
    assert_eq!(summary.skipped, 1);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].login_id, "new_user");
    assert_eq!(next[0].tags, vec!["반도체".to_string(), "대형주".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: 실패 확인** → FAIL
- [ ] **Step 3: 구현 — 순수 `import_accounts` + 커맨드**

`excel.rs`:

```rust
use calamine::{open_workbook, Data, Reader, Xlsx};
use crate::ipc::accounts::{Account, AccountStatus, PlatformId};

fn parse_platform(s: &str) -> Option<PlatformId> {
    match s.trim().to_lowercase().as_str() {
        "forum" => Some(PlatformId::Forum), "naver" => Some(PlatformId::Naver),
        "band" => Some(PlatformId::Band), "instagram" => Some(PlatformId::Instagram),
        "threads" => Some(PlatformId::Threads), _ => None,
    }
}

fn header_index(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
}

/// 첫 시트를 읽어 (loginId,pw,platform[,tags]) 행을 계정으로 변환·병합.
/// 중복 loginId는 기존 행 업데이트. 반환: (병합된 목록, 요약).
pub fn import_accounts(path: &str, mut existing: Vec<Account>) -> Result<(Vec<Account>, ImportSummary), String> {
    let mut wb: Xlsx<_> = open_workbook(path).map_err(|e| e.to_string())?;
    let range = wb.worksheet_range_at(0).ok_or("시트를 찾을 수 없습니다")?.map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers: Vec<String> = rows.next().map(|r| r.iter().map(|c| c.to_string()).collect()).unwrap_or_default();
    let (Some(i_login), Some(i_pw), Some(i_plat)) =
        (header_index(&headers, "loginId"), header_index(&headers, "pw"), header_index(&headers, "platform"))
        else { return Err("필수 컬럼(loginId/pw/platform)이 없습니다".into()); };
    let i_tags = header_index(&headers, "tags");

    let mut summary = ImportSummary { imported: 0, skipped: 0, errors: vec![] };
    let cell = |r: &[Data], i: usize| r.get(i).map(|c| c.to_string()).unwrap_or_default();

    for (n, r) in rows.enumerate() {
        let login = cell(r, i_login).trim().to_owned();
        let pw = cell(r, i_pw).trim().to_owned();
        let plat = parse_platform(&cell(r, i_plat));
        if login.is_empty() || pw.is_empty() || plat.is_none() {
            summary.skipped += 1;
            summary.errors.push(format!("{}행: loginId/pw/platform 누락 또는 오류", n + 2));
            continue;
        }
        let tags: Vec<String> = i_tags
            .map(|i| cell(r, i).split(',').map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default();
        let acct = Account {
            id: login.clone(), platform: plat.unwrap(), login_id: login.clone(),
            pw, status: AccountStatus::New, last: "—".into(), tags,
        };
        match existing.iter_mut().find(|a| a.login_id == login) {
            Some(a) => *a = acct,        // 중복 → 업데이트
            None => existing.push(acct), // 신규 → 추가
        }
        summary.imported += 1;
    }
    Ok((existing, summary))
}
```

lib.rs 커맨드 + register:

```rust
#[tauri::command]
fn import_accounts_xlsx(
    store: tauri::State<'_, JsonStore<ipc::accounts::Account>>,
    activity: tauri::State<'_, JsonStore<ipc::activity::ActivityItem>>,
    path: String,
) -> Result<ipc::excel::ImportSummary, String> {
    let (next, summary) = ipc::excel::import_accounts(&path, store.snapshot())?;
    store.mutate(|_| next.clone());
    ipc::activity::record(activity.inner(), ipc::activity::ActivityType::Info,
        format!("엑셀에서 계정 {}건 가져옴", summary.imported));
    Ok(summary)
}
```

`generate_handler![...]`에 `import_accounts_xlsx,` 추가.

- [ ] **Step 4: 통과** → PASS
- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/excel.rs src-tauri/src/lib.rs
git commit -m "feat(excel): import accounts from xlsx (validate/skip/merge)"
```

### Task D3: `import_posts_xlsx` + 제목 중복 접미사

**Files:**

- Modify: `src-tauri/src/ipc/excel.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: 실패 테스트 (접미사 포함)**

```rust
#[test]
fn unique_title_appends_suffix() {
    let taken = vec!["실적 정리".to_string(), "실적 정리 (1)".to_string()];
    assert_eq!(unique_title("실적 정리", &taken), "실적 정리 (2)");
    assert_eq!(unique_title("새 글", &taken), "새 글");
}

#[test]
fn import_posts_dedupes_titles() {
    let dir = std::env::temp_dir().join("pstmacro_imp_post");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("p.xlsx");
    {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let s = wb.add_worksheet().set_name("게시글").unwrap();
        for (c, h) in ["title", "body", "kind"].iter().enumerate() { s.write_string(0, c as u16, *h).unwrap(); }
        s.write_string(1, 0, "실적 정리").unwrap(); s.write_string(1, 1, "본문").unwrap(); s.write_string(1, 2, "post").unwrap();
        wb.save(&path).unwrap();
    }
    use crate::ipc::posts::{LibraryPost, ModeValue, PostStatus};
    let existing = vec![LibraryPost {
        id: "l1".into(), title: "실적 정리".into(), kind: ModeValue::Post, updated: "—".into(),
        words: 1, status: PostStatus::Draft, excerpt: "x".into(),
        body: None, comments: None, comment_target: None, comment_url: None, comment_count: None,
    }];
    let (next, summary) = import_posts(path.to_str().unwrap(), existing).unwrap();
    assert_eq!(summary.imported, 1);
    assert!(next.iter().any(|p| p.title == "실적 정리 (1)"));
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: 실패 확인** → FAIL
- [ ] **Step 3: 구현**

`excel.rs`:

```rust
use crate::ipc::posts::{LibraryPost, ModeValue, PostStatus};

/// `taken`에 없는 제목이면 그대로, 있으면 " (1)", " (2)" … 접미사.
pub fn unique_title(title: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == title) { return title.to_owned(); }
    let mut n = 1;
    loop {
        let cand = format!("{title} ({n})");
        if !taken.iter().any(|t| t == &cand) { return cand; }
        n += 1;
    }
}

fn parse_kind(s: &str) -> ModeValue {
    match s.trim().to_lowercase().as_str() {
        "comment" => ModeValue::Comment, "both" => ModeValue::Both, _ => ModeValue::Post,
    }
}

pub fn import_posts(path: &str, mut existing: Vec<LibraryPost>) -> Result<(Vec<LibraryPost>, ImportSummary), String> {
    let mut wb: Xlsx<_> = open_workbook(path).map_err(|e| e.to_string())?;
    let range = wb.worksheet_range_at(0).ok_or("시트를 찾을 수 없습니다")?.map_err(|e| e.to_string())?;
    let mut rows = range.rows();
    let headers: Vec<String> = rows.next().map(|r| r.iter().map(|c| c.to_string()).collect()).unwrap_or_default();
    let (Some(i_title), Some(i_body)) = (header_index(&headers, "title"), header_index(&headers, "body"))
        else { return Err("필수 컬럼(title/body)이 없습니다".into()); };
    let i_kind = header_index(&headers, "kind");

    let mut summary = ImportSummary { imported: 0, skipped: 0, errors: vec![] };
    let cell = |r: &[Data], i: usize| r.get(i).map(|c| c.to_string()).unwrap_or_default();

    for (n, r) in rows.enumerate() {
        let title_raw = cell(r, i_title).trim().to_owned();
        let body = cell(r, i_body);
        if title_raw.is_empty() || body.trim().is_empty() {
            summary.skipped += 1;
            summary.errors.push(format!("{}행: title/body 누락", n + 2));
            continue;
        }
        let taken: Vec<String> = existing.iter().map(|p| p.title.clone()).collect();
        let title = unique_title(&title_raw, &taken);
        let kind = i_kind.map(|i| parse_kind(&cell(r, i))).unwrap_or(ModeValue::Post);
        let id = format!("imp-{}", crate::util::now_ms() + n as i64);
        let excerpt: String = body.chars().take(60).collect();
        existing.insert(0, LibraryPost {
            id, title, kind, updated: "방금 전".into(), words: body.chars().count() as u32,
            status: PostStatus::Draft, excerpt, body: Some(body),
            comments: None, comment_target: None, comment_url: None, comment_count: None,
        });
        summary.imported += 1;
    }
    Ok((existing, summary))
}
```

lib.rs 커맨드 + register (`import_posts_xlsx`, activity 기록 `엑셀에서 게시글 {n}건 가져옴`), 패턴은 D2와 동일.

- [ ] **Step 4: 통과** → PASS
- [ ] **Step 5: 커밋**

```bash
git add src-tauri/src/ipc/excel.rs src-tauri/src/lib.rs
git commit -m "feat(excel): import posts from xlsx with (n) title de-dup"
```

### Task D4: 가져오기 버튼 (계정 + 게시글)

**Files:**

- Modify: `src/shared/ipc/index.ts`, `src/test/ipc.ts`, `src/features/accounts/accounts.tsx`, `src/features/posts/posts.tsx`
- Test: 각 `*.test.tsx`

- [ ] **Step 1: IPC 래퍼 + mock + 실패 테스트**

`index.ts` `excel` 그룹에 추가:

```ts
importAccounts: (path: string) => call<ImportSummary>("import_accounts_xlsx", { path }),
importPosts: (path: string) => call<ImportSummary>("import_posts_xlsx", { path }),
```

(`import type { ImportSummary } from "@/shared/bindings/ImportSummary";` 추가)

`src/test/ipc.ts` dispatch에 canned summary:

```ts
case "import_accounts_xlsx":
case "import_posts_xlsx":
  return { imported: 2, skipped: 0, errors: [] };
```

accounts.test: "가져오기" 클릭 → dialog `open` 모킹 → `ipc.excel.importAccounts(path)` 호출 + 결과 토스트 단언.

- [ ] **Step 2: 실패 확인** → FAIL
- [ ] **Step 3: 구현**

`accounts.tsx`/`posts.tsx` 헤더에 "가져오기" 버튼 추가:

```ts
import { open } from "@tauri-apps/plugin-dialog";
// onClick:
const path = await open({
  multiple: false,
  filters: [{ name: "Excel", extensions: ["xlsx"] }],
});
if (typeof path !== "string") return;
const summary = await ipc.excel.importAccounts(path); // posts는 importPosts
// 목록 갱신: 계정/게시글 list 재조회
setRows(await ipc.accounts.list()); // posts는 setPosts(await ipc.posts.list())
notifications.show({
  message: `${summary.imported}건 가져옴${summary.skipped ? `, ${summary.skipped}건 건너뜀` : ""}`,
  color: "green",
});
```

- [ ] **Step 4: 통과** — `pnpm vitest run src/features/accounts src/features/posts` → PASS
- [ ] **Step 5: 커밋**

```bash
git add src/shared/ipc/index.ts src/test/ipc.ts src/features/accounts src/features/posts
git commit -m "feat(ui): xlsx import buttons for accounts and posts"
```

### Task D5: 최종 체크포인트

- [ ] **Step 1: 전체 스위트 + 커버리지**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
pnpm vitest run
pnpm test:coverage
```

Expected: 모두 PASS, 커버리지 70% 게이트 통과.

- [ ] **Step 2: 신규 파일 인접 테스트 확인**

추가된 `src/**/*.tsx`가 모두 인접 `*.test.tsx`를 가지는지(`test-required` CI) 점검. `src/shared/ipc/index.ts` 등은 제외 대상.

- [ ] **Step 3: PR**

```bash
git push -u origin feat/86
gh pr create --title "feat(notifications): 실제 액션 기반 알림 피드 + 엑셀 입출력" \
  --body "Closes #86

- 알림 seed 제거, time:String → at:i64
- 액션 recorder + 계정/게시글/큐/로그인/크롤/게시 계측
- 엑셀 내보내기(알림·계정) / 가져오기(계정·게시글)"
```

---

## Self-Review (작성자 점검 결과)

- **스펙 커버리지:** Part A(A1–A8), Part B(B1–B9), Part C(C1–C6), Part D(D1–D5) — 스펙의 4파트·15개 누락 액션·신규 커맨드 5개 모두 태스크 존재. ✅
- **타입 일관성:** `record(&JsonStore<ActivityItem>, ActivityType, impl Into<String>)`, `ImportSummary{imported,skipped,errors}`, `build_publish_batch(...)`, `import_accounts/import_posts → (Vec<T>, ImportSummary)`, `unique_title(&str,&[String])` — 사용처 전반 일치. ✅
- **누락 위험(구현 시 확인):** ① `src/test/ipc.ts`의 실제 state 식별자/구조에 맞춰 mock 조정 ② dashboard activity 변환 지점의 정확한 위치 ③ auth worker의 `account_id`/`message` 실제 변수명 ④ `worksheet_range_at`/`Data` calamine 0.26 API 시그니처(버전 고정 후 확인). 각 태스크에 주석으로 명시함.
