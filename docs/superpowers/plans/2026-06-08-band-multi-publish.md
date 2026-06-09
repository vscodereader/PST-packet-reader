# 밴드 멀티 게시 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 게시 모달 밴드 섹션에서 여러 링크를 한 줄씩 저장해 실제 밴드명을 드롭다운(사수 UI)에 누적하고, 다중 선택한 밴드들에 (계정×밴드) 전부 게시한다.

**Architecture:** 프론트 전용 변경. 백엔드(`band_publish`/`band_resolve_name`)는 무수정. 프론트가 (계정×밴드) 루프로 `ipc.band.publish`를 호출. 단일 `bandLinkSaved`/`bandNameSaved` 상태를 `resolvedBands[]`+`selectedBands[]`로 대체.

**Tech Stack:** React 19, Mantine, vitest, `@/test/ipc` 인메모리 목.

---

## File Structure

- Modify: `src/features/posts/publish-modal.tsx` — 밴드 섹션 UI/상태/잡 구성.
- Modify: `src/features/posts/publish-modal.test.tsx` — 멀티 밴드 테스트.
- 백엔드/목: 변경 없음(`band_resolve_name`·`band_publish` 목 기존 존재).

---

### Task 1: 멀티 밴드 통합 테스트 (실패 먼저)

**Files:**

- Test: `src/features/posts/publish-modal.test.tsx`

- [ ] **Step 1: 실패 테스트 작성** — 기존 밴드 테스트("requires a saved band link...")를 멀티 밴드 흐름으로 교체.

```tsx
it("accumulates bands from links, multi-selects, and publishes to each", async () => {
  renderPublish();
  await userEvent.click(await screen.findByText("value_invest")); // a7 band

  // 링크1 저장 → 드롭다운에 "데일밴드" 누적
  const linkInput = screen.getByLabelText("밴드 링크");
  await userEvent.type(linkInput, "https://band.us/band/103043410");
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  expect(await screen.findByText("데일밴드")).toBeInTheDocument();

  // 링크2 저장 → "밴드 999" 누적
  await userEvent.clear(linkInput);
  await userEvent.type(linkInput, "https://band.us/band/999");
  await userEvent.click(screen.getByRole("button", { name: "저장" }));
  await screen.findByText("밴드 999");

  // 드롭다운에서 두 밴드 선택 → 칩 누적
  await pickOption(0, "데일밴드"); // 밴드 선택 드롭다운(첫 listbox)
  await pickOption(0, "밴드 999");

  // 게시 → 선택한 각 밴드에 band_publish 호출
  const publishBtn = await screen.findByRole("button", {
    name: /^게시 \(\d+\)/,
  });
  await waitFor(() => expect(publishBtn).toBeEnabled());
  await userEvent.click(publishBtn);
  await waitFor(() => {
    const links = ipcBackend.mock.calls
      .filter((c) => c[0] === "band_publish")
      .map((c) => (c[1] as { bandLink: string }).bandLink);
    expect(links).toContain("https://band.us/band/103043410");
    expect(links).toContain("https://band.us/band/999");
  });
});
```

- [ ] **Step 2: 실패 확인** — Run: `pnpm exec vitest run src/features/posts/publish-modal.test.tsx -t "accumulates bands"`. Expected: FAIL("밴드 링크" 저장이 드롭다운에 누적 안 됨 / pickOption 없음).

---

### Task 2: 상태 교체 — resolvedBands / selectedBands

**Files:**

- Modify: `src/features/posts/publish-modal.tsx`

- [ ] **Step 1: 상태 정의 교체** — `bandNameSaved`/`bandResolving`/`bandLink`/`bandLinkSaved` 영역을 아래로.

```tsx
// 저장으로 누적된 밴드 목록(링크별 실제 밴드명)과 게시할 밴드 선택(bandNo).
type ResolvedBand = { bandNo: string; name: string; link: string };
const [resolvedBands, setResolvedBands] = useState<ResolvedBand[]>([]);
const [selectedBands, setSelectedBands] = useState<string[]>([]); // bandNo[]
const [bandLink, setBandLink] = useState("");
const [bandResolving, setBandResolving] = useState(false);
```

- [ ] **Step 2: saveBandLink 누적 구현** — band_no를 링크에서 파싱, resolveName 조회, 중복(bandNo) 제거 후 추가.

```tsx
const bandNoFromLink = (link: string): string => {
  const t = link.trim();
  if (/^\d+$/.test(t)) return t;
  const m = t.match(/\/band\/(\d+)/);
  return m ? m[1]! : t;
};

const saveBandLink = () => {
  const link = bandLink.trim();
  if (!link) return;
  const bandNo = bandNoFromLink(link);
  const bandAcct = selected
    .map((id) => accounts.find((a) => a.id === id))
    .find((a): a is Account => !!a && a.platform === "band");
  setBandLink("");
  setBandResolving(true);
  const add = (name: string) =>
    setResolvedBands((prev) =>
      prev.some((b) => b.bandNo === bandNo)
        ? prev
        : [...prev, { bandNo, name, link }],
    );
  if (!bandAcct) {
    add(link);
    setBandResolving(false);
    return;
  }
  ipc.band
    .resolveName(bandAcct.loginId, link)
    .then((name) => add(name))
    .catch(() => add(link))
    .finally(() => setBandResolving(false));
};
```

- [ ] **Step 3: 빌드 확인** — Run: `pnpm exec tsc --noEmit`. Expected: 일부 미사용/누락 에러(다음 태스크에서 UI/잡 수정).

---

### Task 3: 밴드 섹션 UI — 드롭다운(사수) 복원 + 칩

**Files:**

- Modify: `src/features/posts/publish-modal.tsx` (DestinationPicker 밴드 섹션 + props)

- [ ] **Step 1: DestinationPicker props 교체** — `bandLinkSaved`/`bandNameSaved` 대신 `resolvedBands`/`selectedBands`/`onSelectBand`/`onRemoveBand` 전달. `bandLink`/`setBandLink`/`bandResolving`/`onSaveBandLink` 유지.

- [ ] **Step 2: 밴드 섹션 JSX** — 링크 입력+저장(유지), 그 아래 Select(드롭다운, data=resolvedBands 이름), 그 아래 선택 칩.

```tsx
<Stack gap={8} p={10}>
  <Group gap={8} align="flex-end" wrap="nowrap">
    <TextInput
      style={{ flex: 1 }}
      label="가입할 밴드 링크"
      placeholder="https://band.us/band/103043410"
      value={bandLink}
      onChange={(e) => setBandLink(e.currentTarget.value)}
      leftSection={<Icon.link size={14} />}
      aria-label="밴드 링크"
    />
    <Button
      variant="light"
      color="band"
      onClick={onSaveBandLink}
      disabled={!bandLink.trim() || bandResolving}
    >
      저장
    </Button>
  </Group>
  {bandResolving && (
    <Group gap={6}>
      <Loader size="xs" />
      <Text fz={12} c="dimmed">
        밴드 정보를 확인하는 중…
      </Text>
    </Group>
  )}
  {/* 사수의 드롭다운: 저장으로 누적된 실제 밴드명 목록 */}
  <Select
    placeholder={
      resolvedBands.length
        ? "게시할 밴드 선택"
        : "링크를 저장하면 밴드가 표시됩니다"
    }
    data={resolvedBands.map((b) => b.name)}
    value={null}
    disabled={resolvedBands.length === 0}
    onChange={(name) => {
      const b = resolvedBands.find((x) => x.name === name);
      if (b) onSelectBand(b.bandNo);
    }}
  />
  {selectedBands.length > 0 ? (
    <Group gap={6}>
      {selectedBands.map((no) => {
        const b = resolvedBands.find((x) => x.bandNo === no);
        return (
          <Badge
            key={no}
            color="band"
            variant="light"
            rightSection={
              <ActionIcon
                size={14}
                variant="transparent"
                color="band"
                onClick={() => onRemoveBand(no)}
              >
                <Icon.x size={10} />
              </ActionIcon>
            }
          >
            {b?.name ?? no}
          </Badge>
        );
      })}
    </Group>
  ) : (
    <Text fz={12} c="orange.7">
      게시할 밴드를 선택하세요.
    </Text>
  )}
</Stack>
```

- [ ] **Step 3: 부모에서 핸들러 전달** — `onSelectBand={(no)=>setSelectedBands(s=>s.includes(no)?s:[...s,no])}`, `onRemoveBand={(no)=>setSelectedBands(s=>s.filter(x=>x!==no))}`, `onSaveBandLink={saveBandLink}`.

---

### Task 4: 잡 구성(계정×밴드) + canPublish + bandWork

**Files:**

- Modify: `src/features/posts/publish-modal.tsx`

- [ ] **Step 1: 밴드 잡 = 계정 × selectedBands** — 기존 `a.platform === "band"` 분기를 selectedBands 루프로.

```tsx
} else if (a.platform === "band") {
  selectedBands.forEach((no) => {
    const b = resolvedBands.find((x) => x.bandNo === no);
    if (!b) return;
    jobs.push({ key: `${aid}-${no}`, platform: "band", loginId: a.loginId,
      targetName: b.name, board: "전체글", status: a.status, bandLink: b.link });
  });
}
```

(PublishJob에 `bandLink?: string` 추가 필요 시 `src/shared/data/types.ts` 확인 — 없으면 잡에 담지 말고 bandWork에서 resolvedBands로 링크 조회.)

- [ ] **Step 2: bandReady** — `selPlatforms.includes("band") ? (when==="schedule" || selectedBands.length>0) : true` 형태로 canPublish에 반영.

- [ ] **Step 3: bandWork** — 각 밴드 잡마다 그 밴드 링크로 publish.

```tsx
const bandWork = Promise.all(
  bandJobs.map((j) => {
    const link = resolvedBands.find((b) => b.name === j.targetName)?.link ?? "";
    return ipc.band
      .publish({
        accountId: j.loginId,
        bandLink: link,
        title: doc.title,
        content: htmlToText(doc.body ?? ""),
        ...(bandComment ? { comment: bandComment } : {}),
      })
      .then((out) => ({
        ...j,
        ok: true,
        targetName: out.bandName ?? j.targetName,
        msg: out.commented ? "글·댓글 게시 완료" : "글 게시 완료",
      }))
      .catch((err: unknown) => ({ ...j, ok: false, msg: errText(err) }));
  }),
);
```

- [ ] **Step 4: tsc + 대상 테스트** — Run: `pnpm exec tsc --noEmit` (0), `pnpm exec vitest run src/features/posts/publish-modal.test.tsx`. 기존 밴드 테스트("picks a per-account...")가 드롭다운 변화로 깨지면 selectedBands 흐름에 맞게 수정.

---

### Task 5: 전체 검증 + 커밋

- [ ] **Step 1:** Run: `pnpm exec tsc --noEmit` → 0; `pnpm exec vitest run` → all pass; `pnpm exec eslint <변경파일>` → 0.
- [ ] **Step 2: 커밋**

```bash
git add src/features/posts/publish-modal.tsx src/features/posts/publish-modal.test.tsx
git commit -m "feat(band-post): 멀티 밴드 게시 — 사수 드롭다운 복원 + 다중선택 칩

여러 링크를 한 줄씩 저장해 실제 밴드명을 사수 드롭다운에 누적, 다중 선택한 밴드들에
(계정×밴드) 전부 가입+게시. 백엔드 무수정(band_publish 프론트 루프). Refs #150"
```

- [ ] **Step 3:** `git push origin feat/150`.
