import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Group,
  Loader,
  Modal,
  Radio,
  SegmentedControl,
  Select,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { useEffect, useState } from "react";

import { KIND, STATUS_ACCOUNT } from "@/shared/data/config";
import {
  acctPlatforms,
  hasToken,
  resolveTemplate,
} from "@/shared/data/helpers";
import type {
  Account,
  Band,
  Cafe,
  GoFn,
  LibraryPost,
  PlatformId,
  PublishJob,
  PublishResult,
  Stock,
} from "@/shared/data/types";
import { listAccounts } from "@/shared/ipc/accounts";
import { listBands } from "@/shared/ipc/bands";
import { listCafes } from "@/shared/ipc/cafes";
import { listStocks } from "@/shared/ipc/stocks";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo, PlatformPill } from "@/shared/ui/platform-logo";

import { PreviewModal } from "./preview-modal";
import { StockCrawlModal } from "./stock-crawl-modal";

export interface PublishModalProps {
  open: boolean;
  doc: LibraryPost | null;
  onClose: () => void;
  go: GoFn;
}

function AccountRow({
  a,
  selected,
  onToggle,
}: {
  a: Account;
  selected: boolean;
  onToggle: (id: string) => void;
}) {
  const st = STATUS_ACCOUNT[a.status] ?? { t: a.status, c: "gray" };
  const disabled = a.status === "error";
  return (
    <Group
      gap={9}
      px={10}
      py={7}
      wrap="nowrap"
      onClick={() => !disabled && onToggle(a.id)}
      style={{
        borderRadius: "var(--mantine-radius-sm)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
        background: selected
          ? "var(--mantine-color-blue-light)"
          : "transparent",
      }}
    >
      <Checkbox checked={selected} readOnly size="sm" disabled={disabled} />
      <PlatformLogo id={a.platform} size={24} />
      <Text fz={13} fw={700} ff="monospace" style={{ flexShrink: 0 }}>
        {a.loginId}
      </Text>
      <Group
        gap={4}
        wrap="nowrap"
        style={{ flex: 1, minWidth: 0, overflow: "hidden" }}
      >
        {(a.tags ?? []).map((t) => (
          <Badge key={t} size="xs" variant="default" radius="xl">
            # {t}
          </Badge>
        ))}
      </Group>
      {a.status !== "active" && (
        <Badge size="sm" color={st.c} variant="light">
          {st.t}
        </Badge>
      )}
    </Group>
  );
}

function DestinationPicker({
  selPlatforms,
  stockCodes,
  openStockModal,
  removeStock,
  cafe,
  setCafe,
  cafeBoard,
  setCafeBoard,
  band,
  setBand,
  cafes,
  bands,
  stocks,
}: {
  selPlatforms: PlatformId[];
  stockCodes: string[];
  openStockModal: () => void;
  removeStock: (code: string) => void;
  cafe: string;
  setCafe: (v: string) => void;
  cafeBoard: string;
  setCafeBoard: (v: string) => void;
  band: string;
  setBand: (v: string) => void;
  cafes: Cafe[];
  bands: Band[];
  stocks: Stock[];
}) {
  const cafeObj = cafes.find((c) => c.name === cafe) ?? cafes[0];
  const card = {
    border: "1px solid var(--mantine-color-gray-2)",
    borderRadius: "var(--mantine-radius-md)",
    overflow: "hidden",
  };
  const head = {
    background: "var(--mantine-color-gray-0)",
    borderBottom: "1px solid var(--mantine-color-gray-2)",
  };
  return (
    <Stack gap={10}>
      {selPlatforms.includes("forum") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} wrap="nowrap" style={head}>
            <PlatformLogo id="forum" size={22} />
            <Text fz={13} fw={700} style={{ flex: 1 }}>
              종목토론방
            </Text>
            <Button
              size="compact-xs"
              radius="xl"
              variant="light"
              color="forum"
              leftSection={<Icon.globe size={13} />}
              onClick={openStockModal}
            >
              종목 선택
            </Button>
          </Group>
          <Box p={10}>
            {stockCodes.length === 0 ? (
              <Text fz={12} c="gray.5" px={2} py={4}>
                크롤링으로 게시할 종목토론방을 선택하세요.
              </Text>
            ) : (
              <Group gap={6}>
                {stockCodes.map((code) => (
                  <Group
                    key={code}
                    gap={6}
                    h={28}
                    pl={10}
                    pr={6}
                    wrap="nowrap"
                    style={{
                      borderRadius: 999,
                      background: "var(--mantine-color-forum-light)",
                    }}
                  >
                    <Text fz={12} fw={700} c="forum">
                      {stocks.find((s) => s.code === code)?.name ?? code}
                    </Text>
                    <ActionIcon
                      size={17}
                      radius="xl"
                      variant="transparent"
                      color="forum"
                      onClick={() => removeStock(code)}
                    >
                      <Icon.x size={11} />
                    </ActionIcon>
                  </Group>
                ))}
              </Group>
            )}
          </Box>
        </Box>
      )}
      {selPlatforms.includes("naver") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="naver" size={22} />
            <Text fz={13} fw={700}>
              네이버 카페
            </Text>
          </Group>
          <Group gap={8} p={10} grow>
            <Select
              value={cafe}
              data={cafes.map((c) => c.name)}
              onChange={(v) => {
                if (!v) return;
                setCafe(v);
                const c = cafes.find((x) => x.name === v);
                setCafeBoard(c?.boards[0] ?? "");
              }}
            />
            <Select
              value={cafeBoard}
              data={cafeObj?.boards ?? []}
              onChange={(v) => setCafeBoard(v ?? "")}
              style={{ maxWidth: 130 }}
            />
          </Group>
        </Box>
      )}
      {selPlatforms.includes("band") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="band" size={22} />
            <Text fz={13} fw={700}>
              밴드
            </Text>
          </Group>
          <Box p={10}>
            <Select
              value={band}
              data={bands.map((b) => b.name)}
              onChange={(v) => setBand(v ?? "")}
            />
          </Box>
        </Box>
      )}
    </Stack>
  );
}

function PublishFlow({
  state,
  mode,
  when,
  date,
  time,
  count,
  onClose,
  go,
}: {
  state: null | "running" | PublishResult[];
  mode: LibraryPost["kind"];
  when: "now" | "schedule";
  date: string;
  time: string;
  count: number;
  onClose: () => void;
  go: GoFn;
}) {
  if (!state) return null;
  const running = state === "running";
  const results = Array.isArray(state) ? state : [];
  const okCount = results.filter((r) => r.ok).length;
  const allOk = results.length > 0 && okCount === results.length;
  const actionWord =
    mode === "comment" ? "댓글" : mode === "both" ? "글·댓글" : "글";

  return (
    <Modal
      opened
      onClose={running ? () => {} : onClose}
      withCloseButton={false}
      size={480}
      radius="lg"
      centered
    >
      <Stack align="center" gap={0} py={14} px={8}>
        {running ? (
          <>
            <ThemeIcon
              size={64}
              radius="xl"
              variant="light"
              color="blue"
              mb={18}
            >
              <Loader size="md" />
            </ThemeIcon>
            <Text fz={19} fw={800} mb={6}>
              {when === "schedule"
                ? `${actionWord} 예약하는 중…`
                : `${actionWord} 게시하는 중…`}
            </Text>
            <Text fz={14} c="dimmed">
              선택한 {count}곳에 차례로 업로드하고 있어요.
            </Text>
          </>
        ) : (
          <>
            <ThemeIcon
              size={64}
              radius="xl"
              variant="light"
              color={allOk ? "green" : "yellow"}
              mb={16}
            >
              {allOk ? (
                <Icon.checkCircle size={36} />
              ) : (
                <Icon.alert size={34} />
              )}
            </ThemeIcon>
            <Text fz={20} fw={800} mb={6}>
              {allOk
                ? when === "schedule"
                  ? "예약 완료!"
                  : "게시 완료!"
                : `${results.length}곳 중 ${okCount}곳 성공`}
            </Text>
            <Text fz={14} c="dimmed" mb={20} ta="center">
              {when === "schedule"
                ? `${date} ${time}에 자동 ${actionWord} 게시됩니다`
                : allOk
                  ? `모든 위치에 정상 ${actionWord} 게시되었어요`
                  : "일부 위치는 다시 시도해 주세요"}
            </Text>
            <Stack
              gap={8}
              w="100%"
              mb={22}
              style={{ maxHeight: 280, overflowY: "auto" }}
            >
              {results.map((r) => (
                <Group
                  key={r.key}
                  gap={11}
                  px={13}
                  py={11}
                  wrap="nowrap"
                  style={{
                    borderRadius: "var(--mantine-radius-md)",
                    border: "1px solid var(--mantine-color-gray-2)",
                    background: "var(--mantine-color-gray-0)",
                  }}
                >
                  <PlatformLogo id={r.platform} size={30} />
                  <Box style={{ flex: 1, minWidth: 0 }}>
                    <Group gap={6} wrap="nowrap">
                      <Text fz={13.5} fw={700} truncate>
                        {r.targetName}
                      </Text>
                      {r.code && (
                        <Badge size="xs" color="forum" variant="light">
                          {r.code}
                        </Badge>
                      )}
                    </Group>
                    <Text fz={11.5} c={r.ok ? "dimmed" : "red"}>
                      {r.loginId} · {r.msg}
                    </Text>
                  </Box>
                  {r.ok ? (
                    <ThemeIcon variant="transparent" color="green">
                      <Icon.checkCircle size={22} />
                    </ThemeIcon>
                  ) : (
                    <Button size="compact-xs" variant="light" color="red">
                      재시도
                    </Button>
                  )}
                </Group>
              ))}
            </Stack>
            <Group gap={9} grow w="100%">
              <Button size="sm" variant="default" onClick={onClose}>
                계속 작성
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  onClose();
                  go(when === "schedule" ? "queue" : "log");
                }}
              >
                {when === "schedule" ? "큐 보기" : "알림 보기"}
              </Button>
            </Group>
          </>
        )}
      </Stack>
    </Modal>
  );
}

function PublishModalInner({ open, doc, onClose, go }: PublishModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [stocks, setStocks] = useState<Stock[]>([]);
  const [cafes, setCafes] = useState<Cafe[]>([]);
  const [bands, setBands] = useState<Band[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [stockCodes, setStockCodes] = useState<string[]>(["005930"]);
  const [stockModal, setStockModal] = useState(false);
  const [cafe, setCafe] = useState("");
  const [cafeBoard, setCafeBoard] = useState("");
  const [band, setBand] = useState("");
  const [when, setWhen] = useState<"now" | "schedule">("now");
  const [date, setDate] = useState("2026-05-29");
  const [time, setTime] = useState("18:00");
  const [acctFilter, setAcctFilter] = useState<"all" | PlatformId>("all");
  const [linkOverride, setLinkOverride] = useState("");
  const [showPreview, setShowPreview] = useState(false);
  const [flow, setFlow] = useState<null | "running" | PublishResult[]>(null);

  useEffect(() => {
    void listAccounts().then((a) => {
      setAccounts(a);
      const firstUsable = a.find((x) => x.status !== "error");
      setSelected((s) => (s.length || !firstUsable ? s : [firstUsable.id]));
    });
    void listStocks().then(setStocks);
    void listCafes().then((c) => {
      setCafes(c);
      setCafe((cur) => cur || (c[0]?.name ?? ""));
      setCafeBoard((cur) => cur || (c[0]?.boards[0] ?? ""));
    });
    void listBands().then((b) => {
      setBands(b);
      setBand((cur) => cur || (b[0]?.name ?? ""));
    });
  }, []);

  if (!doc) {
    return <Modal opened={false} onClose={onClose} />;
  }

  const mode = doc.kind;
  const toggle = (id: string) =>
    setSelected((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  const selPlatforms = acctPlatforms(selected, accounts);

  const acctFilters = [
    { value: "all", label: "전체" },
    { value: "forum", label: "종목토론방" },
    { value: "naver", label: "네이버 카페" },
    { value: "band", label: "밴드" },
  ];
  const visibleAccts = accounts.filter(
    (a) => acctFilter === "all" || a.platform === acctFilter,
  );
  const visUsable = visibleAccts
    .filter((a) => a.status !== "error")
    .map((a) => a.id);
  const allVisibleOn =
    visUsable.length > 0 && visUsable.every((id) => selected.includes(id));
  const selectAllVisible = () =>
    setSelected((s) =>
      allVisibleOn
        ? s.filter((id) => !visUsable.includes(id))
        : [...new Set([...s, ...visUsable])],
    );

  const jobs: PublishJob[] = [];
  selected.forEach((aid) => {
    const a = accounts.find((x) => x.id === aid);
    if (!a) return;
    if (a.platform === "forum") {
      stockCodes.forEach((code) =>
        jobs.push({
          key: aid + "-" + code,
          platform: "forum",
          loginId: a.loginId,
          targetName: stocks.find((x) => x.code === code)?.name ?? code,
          code,
          board: "종목토론방",
          status: a.status,
        }),
      );
    } else if (a.platform === "naver") {
      jobs.push({
        key: aid,
        platform: "naver",
        loginId: a.loginId,
        targetName: cafe,
        board: cafeBoard,
        status: a.status,
      });
    } else if (a.platform === "band") {
      jobs.push({
        key: aid,
        platform: "band",
        loginId: a.loginId,
        targetName: band,
        board: "전체글",
        status: a.status,
      });
    }
  });
  const targetsOk = !selPlatforms.includes("forum") || stockCodes.length > 0;
  const canPublish = selected.length > 0 && targetsOk && jobs.length > 0;

  const doPublish = () => {
    setFlow("running");
    const action =
      mode === "comment" ? "댓글" : mode === "both" ? "글+댓글" : "글";
    window.setTimeout(() => {
      setFlow(
        jobs.map((j) => {
          const ok = Math.random() > 0.1;
          return {
            ...j,
            ok,
            msg: ok
              ? when === "schedule"
                ? `${action} 예약 완료`
                : `${action} 게시 완료`
              : "게시 실패 — 잠시 후 재시도",
          };
        }),
      );
    }, 2000);
  };

  const kd = KIND[mode] ?? { t: mode, c: "gray" };
  const allText = [doc.title, doc.body ?? "", ...(doc.comments ?? [])].join(
    " ",
  );
  const hasNameTok = hasToken(allText, "stock");
  const hasCodeTok = hasToken(allText, "code");
  const hasLinkTok = hasToken(allText, "link");
  const showTokens = hasNameTok || hasCodeTok || hasLinkTok;
  const exampleText =
    doc.title || (doc.comments ?? []).find(Boolean) || doc.body || "";

  const tokenChip = (label: string) => (
    <Text
      fz={11.5}
      fw={800}
      ff="monospace"
      c="blue"
      px={8}
      py={2}
      style={{
        background: "var(--mantine-color-body)",
        border: "1px solid var(--mantine-color-blue-filled)",
        borderRadius: 5,
      }}
    >
      {label}
    </Text>
  );

  const timing: {
    v: "now" | "schedule";
    t: string;
    s: string;
    ic: React.ReactNode;
  }[] = [
    {
      v: "now",
      t: "지금 바로 게시",
      s: "대기열에 추가돼 즉시 처리",
      ic: <Icon.bolt size={16} />,
    },
    {
      v: "schedule",
      t: "예약 게시",
      s: "원하는 시간에 자동 업로드",
      ic: <Icon.calendar size={16} />,
    },
  ];

  return (
    <Modal
      opened={open}
      onClose={onClose}
      withCloseButton={false}
      padding={0}
      size={560}
      radius="lg"
      styles={{
        body: { display: "flex", flexDirection: "column", maxHeight: "85vh" },
      }}
    >
      {/* header */}
      <Group
        px={18}
        py={16}
        gap={12}
        wrap="nowrap"
        style={{
          flexShrink: 0,
          borderBottom: "1px solid var(--mantine-color-gray-2)",
        }}
      >
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Text fz={15.5} fw={800}>
            게시 설정
          </Text>
          <Group gap={7} mt={4} wrap="nowrap">
            <Badge size="sm" color={kd.c} variant="light">
              {kd.t}
            </Badge>
            <Text fz={12.5} c="dimmed" truncate>
              {doc.title}
            </Text>
          </Group>
        </Box>
        <ActionIcon size={34} variant="subtle" color="gray" onClick={onClose}>
          <Icon.x size={19} />
        </ActionIcon>
      </Group>

      {/* scroll body */}
      <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }} px={20} py={18}>
        <Group gap={10} mb={10}>
          <Group gap={7}>
            <Icon.users size={17} color="var(--mantine-color-gray-6)" />
            <Text fz={13.5} fw={700}>
              게시 계정
            </Text>
          </Group>
          <Text fz={12} fw={700} c="blue">
            {selected.length}개
          </Text>
          <Button
            size="compact-xs"
            variant="subtle"
            ml="auto"
            onClick={selectAllVisible}
          >
            {allVisibleOn ? "전체 해제" : "보이는 계정 전체"}
          </Button>
        </Group>
        <SegmentedControl
          fullWidth
          size="xs"
          value={acctFilter}
          onChange={(v) => setAcctFilter(v as "all" | PlatformId)}
          data={acctFilters}
        />
        <Box
          mt={10}
          p={5}
          style={{
            border: "1px solid var(--mantine-color-gray-2)",
            borderRadius: "var(--mantine-radius-md)",
            maxHeight: 198,
            overflowY: "auto",
          }}
        >
          {visibleAccts.map((a) => (
            <AccountRow
              key={a.id}
              a={a}
              selected={selected.includes(a.id)}
              onToggle={toggle}
            />
          ))}
          {visibleAccts.length === 0 && (
            <Text ta="center" py={22} fz={12.5} c="gray.5">
              해당 계정이 없어요
            </Text>
          )}
        </Box>

        {selPlatforms.length > 0 && (
          <>
            <Group gap={7} mt={22} mb={12}>
              <Icon.target size={17} color="var(--mantine-color-gray-6)" />
              <Text fz={13.5} fw={700}>
                게시 위치
              </Text>
            </Group>
            <DestinationPicker
              selPlatforms={selPlatforms}
              stockCodes={stockCodes}
              openStockModal={() => setStockModal(true)}
              removeStock={(c) =>
                setStockCodes((s) => s.filter((x) => x !== c))
              }
              cafe={cafe}
              setCafe={setCafe}
              cafeBoard={cafeBoard}
              setCafeBoard={setCafeBoard}
              band={band}
              setBand={setBand}
              cafes={cafes}
              bands={bands}
              stocks={stocks}
            />
          </>
        )}

        {showTokens && (
          <Box
            mt={22}
            p="md"
            style={{
              border: "1px solid var(--mantine-color-blue-filled)",
              background: "var(--mantine-color-blue-light)",
              borderRadius: "var(--mantine-radius-md)",
            }}
          >
            <Group gap={7}>
              <Icon.hash size={15} color="var(--mantine-color-blue-filled)" />
              <Text fz={13} fw={800} c="blue.8">
                변수 자동 치환
              </Text>
            </Group>
            <Group gap={6} mt={9}>
              {hasNameTok && tokenChip("#{종목명}")}
              {hasCodeTok && tokenChip("#{종목코드}")}
              {hasLinkTok && tokenChip("#{링크}")}
              <Text fz={12} c="gray.7">
                가 대상마다 자동으로 채워집니다.
              </Text>
            </Group>
            {hasLinkTok && (
              <Box mt={11}>
                <Text fz={11.5} fw={700} c="gray.7" mb={5}>
                  링크 값{" "}
                  <Text component="span" fw={500} c="gray.5">
                    (선택)
                  </Text>
                </Text>
                <TextInput
                  size="sm"
                  value={linkOverride}
                  onChange={(e) => setLinkOverride(e.currentTarget.value)}
                  placeholder="비우면 종목별 시세 링크 자동 삽입"
                  leftSection={<Icon.link size={14} />}
                />
              </Box>
            )}
            {jobs[0] && (
              <Box
                mt={11}
                pt={10}
                style={{ borderTop: "1px solid rgba(34,139,230,.25)" }}
              >
                <Text fz={11.5} c="dimmed">
                  예시 ·{" "}
                  <Text component="span" fw={700} c="gray.7">
                    {jobs[0].targetName}
                  </Text>
                  :{" "}
                  <Text component="span" c="gray.7">
                    {resolveTemplate(exampleText, jobs[0], linkOverride) || "—"}
                  </Text>
                </Text>
              </Box>
            )}
          </Box>
        )}

        <Group gap={7} mt={22} mb={12}>
          <Icon.clock size={17} color="var(--mantine-color-gray-6)" />
          <Text fz={13.5} fw={700}>
            게시 시점
          </Text>
        </Group>
        <Stack gap={8}>
          {timing.map((o) => {
            const on = when === o.v;
            return (
              <Group
                key={o.v}
                gap={11}
                px={12}
                py={11}
                wrap="nowrap"
                onClick={() => setWhen(o.v)}
                style={{
                  borderRadius: "var(--mantine-radius-md)",
                  border: `1.5px solid ${
                    on
                      ? "var(--mantine-color-blue-filled)"
                      : "var(--mantine-color-gray-2)"
                  }`,
                  background: on
                    ? "var(--mantine-color-blue-light)"
                    : "transparent",
                  cursor: "pointer",
                }}
              >
                <Radio checked={on} readOnly />
                <ThemeIcon
                  variant="transparent"
                  color={on ? "blue" : "gray"}
                  size="sm"
                >
                  {o.ic}
                </ThemeIcon>
                <Box style={{ flex: 1 }}>
                  <Text fz={13.5} fw={700}>
                    {o.t}
                  </Text>
                  <Text fz={11.5} c="dimmed">
                    {o.s}
                  </Text>
                </Box>
              </Group>
            );
          })}
        </Stack>
        {when === "schedule" && (
          <Group gap={8} mt={10} grow>
            <TextInput
              type="date"
              value={date}
              onChange={(e) => setDate(e.currentTarget.value)}
            />
            <TextInput
              type="time"
              value={time}
              onChange={(e) => setTime(e.currentTarget.value)}
              style={{ maxWidth: 120 }}
            />
          </Group>
        )}
      </Box>

      {/* footer */}
      <Group
        px={20}
        py={14}
        gap={10}
        wrap="nowrap"
        style={{
          flexShrink: 0,
          borderTop: "1px solid var(--mantine-color-gray-2)",
        }}
      >
        <Text fz={12} c="gray.5">
          {jobs.length}곳
        </Text>
        <PlatformPill ids={selPlatforms} size={16} />
        <Box style={{ flex: 1 }} />
        <Button
          size="sm"
          variant="default"
          leftSection={<Icon.eye size={16} />}
          onClick={() => setShowPreview(true)}
        >
          미리보기
        </Button>
        <Button
          size="sm"
          disabled={!canPublish}
          leftSection={
            when === "schedule" ? (
              <Icon.calendar size={18} />
            ) : (
              <Icon.send size={17} />
            )
          }
          onClick={doPublish}
        >
          {when === "schedule"
            ? `예약 (${jobs.length})`
            : `게시 (${jobs.length})`}
        </Button>
      </Group>

      <StockCrawlModal
        open={stockModal}
        preselected={stockCodes}
        onClose={() => setStockModal(false)}
        onConfirm={(stocks) => {
          setStockCodes(stocks.map((s) => s.code));
          setStockModal(false);
        }}
      />
      <PreviewModal
        open={showPreview}
        onClose={() => setShowPreview(false)}
        mode={mode}
        title={doc.title}
        body={doc.body ?? ""}
        comments={(doc.comments ?? []).filter(Boolean)}
        jobs={jobs}
        linkOverride={linkOverride}
        {...(doc.commentTarget ? { commentTarget: doc.commentTarget } : {})}
        {...(doc.commentCount ? { commentCount: doc.commentCount } : {})}
      />
      <PublishFlow
        state={flow}
        mode={mode}
        when={when}
        date={date}
        time={time}
        count={jobs.length}
        onClose={() => {
          setFlow(null);
          onClose();
        }}
        go={go}
      />
    </Modal>
  );
}

export function PublishModal(props: PublishModalProps) {
  // Remount per open / per document so useState initializers re-seed.
  return (
    <PublishModalInner
      key={props.open ? (props.doc?.id ?? "new") : "closed"}
      {...props}
    />
  );
}
