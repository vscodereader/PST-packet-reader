# 밴드 멀티 게시 — 설계 (사수 드롭다운 복원 + 확장)

날짜: 2026-06-08 · 이슈 #150 · 브랜치 feat/150

## 배경 / 문제

게시 모달의 밴드 섹션에는 사수가 만든 밴드 선택 **드롭다운(`<Select>`)** 이 있었다.
그 드롭다운의 데이터가 가짜 시드(`가치투자모임/단타클럽/주식스터디 BAND`)라 혼동을 줬다.
직전 작업에서 **더미 데이터만** 지워야 했는데 **드롭다운 UI 자체를 제거**해 버린 것이 잘못이다.
사수 UI는 보존하고, 그 위에 사용자 기능을 **추가**하는 것이 원칙이다.

## 목표 (사용자 시나리오)

1. **가입할 밴드 링크**를 한 줄 입력하고 **저장** → 그 링크의 실제 밴드명을 조회.
2. 사수의 **드롭다운**에 조회된 실제 밴드명들이 **누적**된다(여러 링크를 한 줄씩 저장해 쌓음).
3. 드롭다운에서 게시할 밴드를 **여러 개 선택** → 선택한 밴드들이 아래 **칩**으로 표시(삭제 가능).
4. **게시** → **선택한 각 밴드 계정 × 선택한 각 밴드** 조합 전부에 가입 + 글(+댓글).

## 비목표 (YAGNI)

- 백엔드 멀티 밴드 전용 커맨드는 만들지 않는다(기존 `band_publish`를 프론트가 반복 호출).
- 예약(schedule) 게시에서 밴드는 기존대로 plan에서 제외(엔진 미연결) — 멀티 밴드도 즉시 게시(now)에만.
- 초대 전용 단축링크(`band.us/n/...`)는 미지원(기존과 동일).

## 아키텍처

### 백엔드 (Rust) — 변경 없음

- `band_post::band_publish(account_id, band_link, title, content, comment)` — 1계정+1링크 게시. 이미 검증됨.
- `band_post::resolve_band_name(account_id, band_link)` — 링크의 밴드명 조회. 이미 존재.
- IPC `band_publish`, `band_resolve_name` 그대로 사용.
- 멀티는 **프론트가 (계정×밴드) 루프로 `band_publish`를 호출**해 구성한다. 검증된 백엔드는 무수정.

### 프론트 (publish-modal.tsx) — 밴드 섹션 재구성

DestinationPicker의 밴드 섹션을 위→아래 순서로:

1. **링크 입력 + 저장 버튼** (기존 유지)
   - 저장 시: 선택된 첫 밴드 계정 loginId로 `ipc.band.resolveName(loginId, link)` 호출.
   - 결과를 `resolvedBands` 목록에 추가: `{ bandNo, name, link }`. `bandNo` 기준 **중복 제거**.
   - 조회 중에는 로딩 표시. 조회 실패(미로그인 등)면 `name = 링크` 폴백으로 추가(게시는 가능).
2. **사수의 드롭다운 `<Select>` 복원**
   - `data = resolvedBands.map(b => b.name)` (시드 아님, 누적된 실제 밴드명).
   - 항목 선택 시 `selectedBands`에 추가(이미 있으면 무시). 선택 후 드롭다운 값은 비움(연속 선택).
   - `resolvedBands`가 비면 "링크를 저장하면 밴드가 여기 표시됩니다" 안내.
3. **"게시할 밴드" 칩 영역**
   - `selectedBands`를 삭제 가능한 칩(Badge + x)으로 표시.
   - 하나도 없으면 "게시할 밴드를 선택하세요" 안내.

### 상태 (PublishModalInner)

- `resolvedBands: { bandNo: string; name: string; link: string }[]` — 저장으로 누적.
- `selectedBands: string[]` — 선택된 bandNo 목록.
- `bandLink: string`(현재 입력), `bandResolving: boolean`.
- 기존 단일 `bandLinkSaved`/`bandNameSaved`/`band`/`setBand`/`bands` 제거(이 목록으로 대체).

### 게시 잡 모델 (jobs)

- 밴드 잡 = **선택된 밴드 계정들 × `selectedBands`**.
- 각 잡: `{ platform: "band", loginId: 계정, targetName: 밴드명, bandNo }`.
- `runNow`의 bandWork: 각 잡마다 `ipc.band.publish({ accountId: loginId, bandLink: 해당밴드링크, title, content, comment })`.
  - 결과 행 라벨 = 게시 응답의 실제 밴드명(`out.bandName`) 또는 잡의 밴드명.

### 게시 가능 조건 (canPublish)

- `bandReady` = 밴드 미선택 OR 예약(schedule) OR `selectedBands.length > 0`.
- (즉시 게시에서 밴드를 골랐으면 selectedBands가 1개 이상이어야 게시 버튼 활성.)

## 에러 처리

- (계정×밴드) 각 게시는 독립 Promise. 하나 실패해도 나머지 진행, 결과 행에 성공/실패 개별 표시(기존 패턴).
- resolveName 실패: 토스트 없이 링크 폴백으로 목록 추가(게시 시도 가능). 단 콘솔/로그엔 남김.

## 테스트 (vitest)

- 링크 저장 → `resolvedBands`에 밴드명 누적(중복 저장은 1개), 드롭다운에 표시.
- 드롭다운 선택 → 칩 추가, x로 제거.
- 게시 → 선택한 (계정×밴드) 수만큼 `band_publish` 호출, 각 args의 bandLink/accountId 검증.
- 밴드 미선택 시 게시 버튼 비활성.
- 기존 목(`band_resolve_name`, `band_publish`) 재사용.

## 영향 범위 (파일)

- `src/features/posts/publish-modal.tsx` — 밴드 섹션 UI/상태/잡 구성(수정).
- `src/features/posts/publish-modal.test.tsx` — 멀티 밴드 테스트(수정/추가).
- 백엔드 Rust: **변경 없음**.
