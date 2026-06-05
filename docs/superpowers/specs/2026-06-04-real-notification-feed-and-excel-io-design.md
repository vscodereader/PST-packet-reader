# 실제 액션 기반 알림 피드 + 엑셀 입출력

- **이슈:** #86
- **브랜치:** `feat/86` (master 기준)
- **작성일:** 2026-06-04

## 1. 배경 / 문제

알림(`notifications.tsx`) 화면과 대시보드 타임라인은 두 도메인에서 데이터를 받는다:

- `activity` (시스템 활동) — `ipc.activity.list()`
- `log-batches` (게시 배치별 결과) — `ipc.logBatches.list()`

현재 두 도메인 모두 **seed(가짜) 데이터만** 존재한다. 구조적 한계:

1. `activity.rs`는 주석에 _"read-only for the UI"_ 라고 명시돼 있고 **append 커맨드가 없다.** 따라서 로그인·계정 CRUD·크롤링 등 실제 액션이 활동 피드에 아무것도 남기지 못한다.
2. `run_forum_publish_now`(종목토론방 즉시 게시)는 결과를 publish 모달에 인라인 표시할 뿐 **`LogBatch`를 생성하지 않는다.** 그래서 "게시·댓글" 탭은 seed가 없으면 영원히 빈다.
3. `notifications.tsx`의 "내보내기" 버튼은 **가짜 토스트**만 띄운다. 엑셀 가져오기 기능은 없다.
4. `ActivityItem.time`, `LogBatch.time`이 `String`(상대 문자열 `"12분 전"`)이라 실제 타임스탬프 기반 정렬·표시가 불가능하다.

즉 "알림 seed를 지운다"는 요구는 **알림 피드를 실제 액션 기반으로 만든다**는 작업과 분리될 수 없다. 본 스펙은 이를 하나의 기능으로 묶어 4파트로 설계한다.

## 2. 목표 / 비목표

**목표**

- 알림(activity + log-batches)의 seed 데이터를 제거하고, 실제 사용자/시스템 액션이 피드를 채우게 한다.
- 누락된 액션을 모두 찾아 activity 또는 log-batches에 기록한다.
- 알림·계정을 `.xlsx`로 내보내고, 계정·게시글을 `.xlsx`로 가져온다 (Rust 백엔드 + 네이티브 다이얼로그).

**비목표**

- 알림 외 도메인(계정·게시글·종목·큐·카페·밴드)의 seed는 **유지**한다 (개발 편의).
- UI에 미연결된 `run_naver_discussion` / `run_naver_discussion_batch` 경로는 계측하지 않는다 (live 경로 `run_forum_publish_now`만).
- 알림 영구 보관/페이지네이션/검색 인덱싱은 범위 밖 (기존 클라이언트 필터 유지).

## 3. 결정 사항 (확정)

| 항목                 | 결정                                                                   |
| -------------------- | ---------------------------------------------------------------------- |
| 파일 형식            | `.xlsx`                                                                |
| 구현 레이어          | Rust 백엔드 + `tauri-plugin-dialog` 네이티브 다이얼로그                |
| seed 제거 범위       | `activity` + `log-batches` 만                                          |
| 가져오기 대상        | 계정, 게시글                                                           |
| 게시글 CRUD 로깅     | **함** (성공/실패 모두)                                                |
| 게시글 제목 중복 시  | `(1)`, `(2)` … 접미사 자동 추가 (복사하기 방식)                        |
| 계정 loginId 중복 시 | 기존 행 **업데이트**(import가 우선) — loginId는 식별자라 접미사 부적합 |
| 작업 브랜치          | `feat/86` (master 기준 신규)                                           |

## 4. 설계

### Part A — 타임스탬프 기반 (선행)

**데이터 모델 변경**

- `ActivityItem.time: String` → `at: i64` (epoch milliseconds, 로컬).
- `LogBatch.time: String` → `at: i64`.
- ts-rs 바인딩 재생성 (`src/shared/bindings/ActivityItem.ts`, `LogBatch.ts`).

**프론트 표시**

- `src/shared/data/helpers.ts`에 추가:
  - `formatRelative(at: number): string` — `방금` / `N분 전` / `N시간 전` / `어제 HH:mm` / `M월 D일` 규칙.
  - `dayBucket(at: number): "오늘" | "어제" | "이전"` — date 연산 기반 (기존 `notifications.tsx` 내 정규식 버전 대체).
- `notifications.tsx`: 내부 `dayBucket(string)` 제거, `helpers`의 `dayBucket(at)` / `formatRelative(at)` 사용. `SystemRow.time`, batch 시각 표시를 `at` 기반으로.
- `dashboard.tsx`: `a.time` / `b.time` 직접 출력(228, 60행 부근)을 `formatRelative(at)`로 교체.

**seed 제거**

- `activity.rs::seed()` → `Vec::new()`.
- `log_batches.rs::seed()` → `Vec::new()`.
- `manage_stores`는 변경 없음 (빈 스토어로 시작; 파일 없으면 빈 배열 persist).
- 관련 테스트 갱신: `activity.rs`의 `seed_covers_every_activity_type` 등 제거/대체, `notifications.test.tsx`·`dashboard` 테스트의 시각 가정 수정.

### Part B — 액션 recorder + 계측

**recorder API (Rust)**

- `activity.rs`에 추가:

  ```rust
  pub fn record(store: &JsonStore<ActivityItem>, ty: ActivityType, text: impl Into<String>);
  ```

  - 새 `ActivityItem { id: uuid-ish, type, text, at: now_ms() }`를 **맨 앞에** prepend, 최대 보관 건수(예: 500) 초과 시 오래된 것 truncate.

- 범용 커맨드 `append_activity(ty, text)` — 프론트/worker 종료 등 백엔드-only 핸들러가 아닌 흐름에서 호출.
- 시각 유틸 `now_ms()` (epoch ms) 공통 헬퍼.

**activity로 기록할 액션 (누락 액션 목록)**

| 액션             | 기록 위치                 | 예시 문구                             |
| ---------------- | ------------------------- | ------------------------------------- |
| 로그인 성공      | auth queue worker 종료 시 | `계정 {loginId} 로그인 성공`          |
| 로그인 실패      | auth queue worker 종료 시 | `계정 {loginId} 로그인 실패 — {사유}` |
| 계정 추가        | `add_account`             | `계정 {loginId} 추가됨`               |
| 계정 수정        | `update_account`          | `계정 {loginId} 수정됨`               |
| 계정 삭제        | `delete_accounts`         | `계정 {n}건 삭제됨`                   |
| 게시글 저장 성공 | `upsert_post`             | `게시글 '{title}' 저장됨`             |
| 게시글 저장 실패 | `upsert_post` (오류 경로) | `게시글 저장 실패 — {사유}`           |
| 게시글 삭제      | `delete_post`             | `게시글 '{title}' 삭제됨`             |
| 예약 추가        | `add_queue_scheduled`     | `예약 추가됨 — {title}`               |
| 예약 취소        | `cancel_queue_scheduled`  | `예약 취소됨`                         |
| 지금 게시 취소   | `cancel_queue_now`        | `진행 작업 취소됨`                    |
| 즉시 게시 전환   | `promote_queue_scheduled` | `예약을 즉시 게시로 전환`             |
| 종목 크롤링      | `search_stocks`/crawl     | `종목 {n}개 크롤링`                   |
| 계정 가져오기    | `import_accounts_xlsx`    | `엑셀에서 계정 {n}건 가져옴`          |
| 게시글 가져오기  | `import_posts_xlsx`       | `엑셀에서 게시글 {n}건 가져옴`        |

> 백엔드 커맨드 핸들러는 해당 `JsonStore<ActivityItem>`를 추가 `State` 인자로 받아 in-handler에서 `record(...)` 호출. 로그인은 worker가 `State`를 안 받으므로 **activity 스토어 `Arc`를 worker 컨텍스트에 주입**하거나, terminal 상태 도달 시 프론트 `pollLogin`이 `append_activity`를 호출 — 둘 중 worker 주입을 1순위로 시도하고, 플러밍이 과하면 프론트 호출로 대체(스펙 구현 단계에서 확정).

**log-batches로 기록할 액션**

- `run_forum_publish_now`가 종목별 결과(`ForumPublishResult`)를 모아 **`LogBatch` 1건 생성**:
  - `title` = 게시 제목, `kind` = 요청의 post/comment/both, `at` = now, `items` = 종목별 `BatchItem`(platform=forum, target=종목명, code, login_id, status=success/fail, msg, trace=실패 시).
  - 커맨드가 `JsonStore<LogBatch>`를 `State`로 받아 결과 반환 직전에 prepend.
- 진행 중 표시(`state: running`)는 동기 호출 구조상 생략(완료 후 1건 append). 향후 비동기화 시 확장.

### Part C — 엑셀 내보내기 (알림 + 계정)

**의존성**

- `rust_xlsxwriter` (워크북 생성, 순수 Rust).
- `tauri-plugin-dialog` + capabilities `dialog:allow-save` / `dialog:allow-open`.
- 프론트 `@tauri-apps/plugin-dialog`.

**커맨드**

- `export_activity_xlsx(path: String) -> Result<(), String>`
  - 시트 ①「게시 배치」: batch별 flatten — 시각 / 제목 / 종류 / 플랫폼 / 대상 / 코드 / 계정 / 상태 / 메시지.
  - 시트 ②「시스템 활동」: 시각 / 유형 / 내용.
- `export_accounts_xlsx(path: String) -> Result<(), String>`
  - 컬럼: loginId / pw / platform / status / tags(쉼표 결합) / last. (**pw 포함** — 백업/재가져오기 round-trip 지원. 가져오기 필수 컬럼과 대칭.)

**프론트 흐름**

- `save({ defaultPath, filters: [{ name: "Excel", extensions: ["xlsx"] }] })` → 경로 → `invoke`.
- `notifications.tsx`: 가짜 토스트 제거, `export_activity_xlsx` 호출 + 성공/취소 토스트.
- `accounts.tsx`: 헤더에 "내보내기" 버튼 추가 → `export_accounts_xlsx`.

### Part D — 엑셀 가져오기 (계정 + 게시글)

**의존성**

- `calamine` (xlsx 읽기, 순수 Rust).

**커맨드**

- `import_accounts_xlsx(path: String) -> Result<ImportSummary, String>`
  - 필수 컬럼: `loginId`, `pw`, `platform`. 선택: `tags`.
  - 행 검증: loginId/pw 비어있으면 skip+집계, platform 미인식이면 skip. 중복 loginId → 기존 행 업데이트.
  - 성공 행을 accounts 스토어에 병합, activity 기록.
- `import_posts_xlsx(path: String) -> Result<ImportSummary, String>`
  - 필수 컬럼: `title`, `body`. 선택: `kind`(기본 post).
  - 제목 중복 시 `제목 (1)`, `제목 (2)` … 접미사로 유일화.
  - posts 스토어 병합, activity 기록.
- `ImportSummary { imported: u32, skipped: u32, errors: Vec<String> }` (ts-rs export).

**프론트 흐름**

- `open({ multiple: false, filters: [...] })` → 경로 → `invoke` → 반환 리스트로 화면 state 갱신.
- `accounts.tsx` / `posts.tsx`: "가져오기" 버튼 + 결과 요약 토스트(`{imported}건 가져옴, {skipped}건 건너뜀`).

## 5. IPC 표면 변경 요약

신규 커맨드 (lib.rs `register_handlers` 등록 + `src/shared/ipc/index.ts` 노출 + `src/test/ipc.ts` 모킹):

- `append_activity`
- `export_activity_xlsx`, `export_accounts_xlsx`
- `import_accounts_xlsx`, `import_posts_xlsx`

> `src/test/ipc.ts`(인메모리 백엔드)와 실제 커맨드 셋의 동기화를 반드시 유지한다 (기존 리뷰에서 지적된 divergence 패턴 재발 방지). 신규 커맨드는 mock에도 대응 핸들러를 추가한다.

## 6. 테스트 전략

- **Rust 단위:** `record()` prepend/cap, `import_*`의 컬럼 검증·중복 접미사·skip 집계, `export_*`의 워크북 생성(임시 파일 → calamine으로 재읽기 round-trip), publish→LogBatch 변환.
- **프론트(vitest):** `formatRelative`/`dayBucket` 경계값(방금/어제/이전), 가져오기 결과 토스트, 내보내기 버튼이 dialog+invoke를 호출(모킹).
- **신규 `src/**/_.tsx`파일은 인접`_.test.tsx`필수** (CI`test-required`).
- 커버리지 70% 게이트 유지.

## 7. 구현 순서 (phase)

1. **Part A** — `at` 필드 + `formatRelative` + seed 제거 + 화면/테스트 수정. (피드는 비지만 깨지지 않음)
2. **Part B** — recorder + 백엔드 핸들러 계측 + 로그인 로깅 + publish→LogBatch.
3. **Part C** — 내보내기(deps, dialog 플러그인, 커맨드 2개, 버튼).
4. **Part D** — 가져오기(calamine, 커맨드 2개, 검증, 버튼, 결과 UI).

각 phase는 독립 커밋. Part A 완료 시점에 앱이 동작(빈 피드)해야 한다.

## 8. 리스크 / 유의점

- **로그인 로깅 플러밍:** worker가 `State`를 안 받음 → activity 스토어 `Arc` 주입이 1순위, 과하면 프론트 `pollLogin` 종료 시 `append_activity` 폴백.
- **타임스탬프 마이그레이션:** 기존 디스크의 `activity.json`/`log-batches.json`에 `time:String`이 남아 있으면 역직렬화 실패 가능 → seed 제거와 함께 구 파일은 무시/덮어쓰기되도록 `#[serde(default)]` 또는 로드 실패 시 빈 배열 fallback(현 `read_json`이 이미 `None`→seed 처리). 개발 머신의 구 파일은 삭제 안내.
- **xlsx 한글:** `rust_xlsxwriter`/`calamine` 모두 UTF-8 처리 OK.
- **`src/test/ipc.ts` 동기화:** 신규 커맨드 누락 시 테스트가 `unhandled command`로 실패 — 체크리스트에 포함.
