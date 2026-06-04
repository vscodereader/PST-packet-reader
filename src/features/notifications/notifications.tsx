import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Card,
  Center,
  Container,
  Group,
  Loader,
  SegmentedControl,
  Select,
  SimpleGrid,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
  Title,
  Tooltip,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useCallback, useEffect, useState } from "react";

import type { EnvironmentStatus } from "@/shared/bindings/EnvironmentStatus";
import { ACTIVE_PLATFORMS, KIND } from "@/shared/data/config";
import { batchStatus } from "@/shared/data/helpers";
import type { BatchItem, LogBatch, LogFilter } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

function dayBucket(time: string): "오늘" | "어제" | "이전" {
  if (/^어제/.test(time)) return "어제";
  if (/^오늘/.test(time) || /방금|분 전|시간 전/.test(time)) return "오늘";
  return "이전";
}

const BATCH_STATUS: Record<string, { t: string; c: string }> = {
  running: { t: "처리중", c: "blue" },
  success: { t: "성공", c: "green" },
  partial: { t: "일부 실패", c: "yellow" },
  fail: { t: "실패", c: "red" },
};

function statusColor(s: string) {
  return s === "success"
    ? "green"
    : s === "fail"
      ? "red"
      : s === "running"
        ? "blue"
        : "gray";
}

function SubLog({ item }: { item: BatchItem }) {
  const [showTrace, setShowTrace] = useState(false);
  const ok = item.status === "success";
  const fail = item.status === "fail";
  const color = statusColor(item.status);
  return (
    <Box
      px={16}
      py={9}
      pl={52}
      style={{
        borderTop: "1px solid var(--mantine-color-gray-2)",
        background: "var(--mantine-color-gray-0)",
      }}
    >
      <Group gap={11} wrap="nowrap">
        <ThemeIcon size={22} radius="xl" variant="light" color={color}>
          {ok ? (
            <Icon.check size={13} />
          ) : fail ? (
            <Icon.x size={13} />
          ) : (
            <Loader size={13} color={color} />
          )}
        </ThemeIcon>
        <PlatformLogo id={item.platform} size={20} />
        <Group gap={5} style={{ flex: 1, minWidth: 0 }} wrap="nowrap">
          <Text fz={12.5} fw={700} truncate>
            {item.target}
          </Text>
          {item.code && (
            <Text fz={10} fw={700} c="forum" ff="monospace">
              {item.code}
            </Text>
          )}
          <Text fz={11.5} c="dimmed" ff="monospace">
            · {item.loginId}
          </Text>
        </Group>
        <Text fz={11.5} c={fail ? "red" : "dimmed"} style={{ flexShrink: 0 }}>
          {item.msg}
        </Text>
        {fail && item.trace && (
          <Button
            size="compact-xs"
            variant="default"
            radius="xl"
            rightSection={
              <Icon.chevronDown
                size={12}
                style={{
                  transform: showTrace ? "rotate(180deg)" : "none",
                  transition: "transform .15s",
                }}
              />
            }
            onClick={() => setShowTrace((s) => !s)}
          >
            {showTrace ? "접기" : "자세히 보기"}
          </Button>
        )}
      </Group>
      {fail && item.trace && showTrace && (
        <Box
          component="pre"
          ml={33}
          mt={9}
          p="sm"
          style={{
            background: "#1f2329",
            color: "#e6e8eb",
            borderRadius: "var(--mantine-radius-sm)",
            fontSize: 11.5,
            lineHeight: 1.6,
            fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
            whiteSpace: "pre-wrap",
            overflowX: "auto",
          }}
        >
          {item.trace}
        </Box>
      )}
    </Box>
  );
}

function BatchRow({
  batch,
  expanded,
  onToggle,
}: {
  batch: LogBatch;
  expanded: boolean;
  onToggle: () => void;
}) {
  const status = batchStatus(batch);
  const bs = BATCH_STATUS[status] ?? { t: status, c: "gray" };
  const kd = KIND[batch.kind] ?? { t: batch.kind, c: "gray" };
  const okN = batch.items.filter((i) => i.status === "success").length;
  const failN = batch.items.filter((i) => i.status === "fail").length;
  return (
    <Box style={{ borderBottom: "1px solid var(--mantine-color-gray-2)" }}>
      <Group
        gap={14}
        px={16}
        py={12}
        wrap="nowrap"
        style={{ cursor: "pointer" }}
        {...(expanded ? { bg: "gray.0" } : {})}
        onClick={onToggle}
      >
        <ThemeIcon size={26} radius="xl" variant="light" color={bs.c}>
          {status === "success" ? (
            <Icon.check size={15} />
          ) : status === "fail" ? (
            <Icon.x size={15} />
          ) : status === "running" ? (
            <Loader size={14} color={bs.c} />
          ) : (
            <Icon.alert size={14} />
          )}
        </ThemeIcon>
        <Badge size="sm" color={kd.c} variant="light">
          {kd.t}
        </Badge>
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Text fz={13.5} fw={700} truncate>
            {batch.title}
          </Text>
          <Text fz={11.5} c="dimmed" mt={2}>
            {batch.items.length}곳 · 성공 {okN}
            {failN > 0 && (
              <Text fz={11.5} component="span" c="red">
                {" "}
                · 실패 {failN}
              </Text>
            )}
          </Text>
        </Box>
        <Badge size="sm" color={bs.c} variant="light">
          {bs.t}
        </Badge>
        <Text fz={12} c="dimmed" w={70} ta="right" style={{ flexShrink: 0 }}>
          {batch.time.replace(/^(오늘|어제)\s/, "")}
        </Text>
        <Icon.chevronDown
          size={17}
          style={{
            color: "var(--mantine-color-gray-5)",
            flexShrink: 0,
            transform: expanded ? "rotate(180deg)" : "none",
            transition: "transform .18s",
          }}
        />
      </Group>
      {expanded && batch.items.map((item, i) => <SubLog key={i} item={item} />)}
    </Box>
  );
}

interface SystemRow {
  id: string;
  status: "success" | "fail" | "info";
  title: string;
  time: string;
}

export function Notifications({ filter }: { filter: LogFilter | null }) {
  const [cat, setCat] = useState<string>(filter?.batchId ? "post" : "all");
  const [status, setStatus] = useState("all");
  const [plat, setPlat] = useState(filter?.platform ?? "all");
  const [acct, setAcct] = useState<LogFilter | null>(
    filter?.loginId ? filter : null,
  );
  const [q, setQ] = useState("");
  const [expanded, setExpanded] = useState<Record<string, boolean>>(
    filter?.batchId ? { [filter.batchId]: true } : {},
  );
  const [logBatches, setLogBatches] = useState<LogBatch[]>([]);
  const [activity, setActivity] = useState<SystemRow[]>([]);
  const [env, setEnv] = useState<EnvironmentStatus | null>(null);
  const [envLoading, setEnvLoading] = useState(false);

  // Re-probe the live environment (Chrome/ADB) from the 새로고침 button. The
  // loading flag drives the button spinner; the initial probe runs in the effect
  // below (async setState only, to avoid synchronous setState in an effect).
  const refreshEnv = useCallback(() => {
    setEnvLoading(true);
    void ipc.diagnostics
      .getStatus()
      .then(setEnv)
      .finally(() => setEnvLoading(false));
  }, []);

  // Chrome 미설치 카드의 "설치 페이지 열기" — 공식 다운로드 페이지를 기본 브라우저로
  // 연다. 열기에 실패해도(드문 경우) 사용자가 막히지 않도록 직접 접속할 주소를 안내.
  const openChromeInstall = useCallback(() => {
    void ipc.diagnostics.openChromeDownload().catch(() => {
      notifications.show({
        message:
          "브라우저를 열지 못했어요. google.com/chrome 에서 직접 설치해 주세요.",
        color: "red",
      });
    });
  }, []);

  useEffect(() => {
    void ipc.diagnostics.getStatus().then(setEnv);
    void ipc.logBatches.list().then(setLogBatches);
    void ipc.activity.list().then((items) =>
      setActivity(
        items.map((a) => ({
          id: a.id,
          status:
            a.type === "error"
              ? "fail"
              : a.type === "success"
                ? "success"
                : "info",
          title: a.text,
          time: a.time,
        })),
      ),
    );
  }, []);

  const sysRows: SystemRow[] = activity;

  const matchBatch = (b: LogBatch) =>
    (status === "all" || batchStatus(b) === status) &&
    (plat === "all" || b.items.some((i) => i.platform === plat)) &&
    (!acct || b.items.some((i) => i.loginId === acct.loginId)) &&
    (!q ||
      b.title.includes(q) ||
      b.items.some((i) => i.target.includes(q) || i.loginId.includes(q)));
  const matchSys = (s: SystemRow) =>
    (status === "all" || s.status === status) &&
    !acct &&
    plat === "all" &&
    (!q || s.title.includes(q));

  type Row =
    | { kind: "batch"; b: LogBatch; time: string }
    | { kind: "system"; s: SystemRow; time: string };

  const batches: Row[] =
    cat === "system"
      ? []
      : logBatches.filter(matchBatch).map((b) => ({
          kind: "batch",
          b,
          time: b.time,
        }));
  const systems: Row[] =
    cat === "post"
      ? []
      : sysRows
          .filter(matchSys)
          .map((s) => ({ kind: "system", s, time: s.time }));
  const merged = [...batches, ...systems];

  const dayOrder: Record<string, number> = { 오늘: 0, 어제: 1, 이전: 2 };
  const groups: { day: string; rows: Row[] }[] = [];
  merged.forEach((row) => {
    const day = dayBucket(row.time);
    let g = groups.find((x) => x.day === day);
    if (!g) {
      g = { day, rows: [] };
      groups.push(g);
    }
    g.rows.push(row);
  });
  groups.sort((a, b) => (dayOrder[a.day] ?? 9) - (dayOrder[b.day] ?? 9));

  const okCount = logBatches.filter((b) => batchStatus(b) === "success").length;
  const failCount = logBatches.filter((b) =>
    ["fail", "partial"].includes(batchStatus(b)),
  ).length;

  const catTabs = [
    { value: "all", label: `전체 ${logBatches.length + sysRows.length}` },
    { value: "post", label: `게시·댓글 ${logBatches.length}` },
    { value: "system", label: `시스템 ${sysRows.length}` },
  ];
  const summary = [
    {
      t: "게시 배치",
      v: logBatches.length,
      color: "gray",
      ic: "layers" as const,
    },
    { t: "성공", v: okCount, color: "green", ic: "checkCircle" as const },
    { t: "실패 포함", v: failCount, color: "red", ic: "alert" as const },
  ];

  // Chrome/ADB 진단 카드용 표시값. env가 아직 없으면 "확인 중".
  // (옵셔널 필드는 ts-rs상 `string | null`이라 null 기준으로 분기한다.)
  // action: 사용자가 바로 취할 조치 버튼(Chrome 미설치 → 설치 페이지).
  // warning: 이 상태가 자동화에 끼치는 영향 경고(ADB 미연결 → IP 변경 불가).
  type EnvCard = {
    color: string;
    label: string;
    detail: string;
    action?: { label: string; onClick: () => void };
    warning?: string;
  };

  const chrome = env?.chrome ?? null;
  const chromeCard: EnvCard = !chrome
    ? { color: "gray", label: "확인 중", detail: "상태를 불러오는 중…" }
    : chrome.installed
      ? {
          color: "green",
          label: "설치됨",
          detail:
            chrome.version != null ? `버전 ${chrome.version}` : "버전 미상",
        }
      : {
          color: "red",
          label: "미설치",
          detail: chrome.error ?? "Chrome을 찾을 수 없습니다",
          // 자동화는 Chrome 으로 로그인하므로, 미설치 시 설치 페이지로 유도한다.
          action: { label: "설치 페이지 열기", onClick: openChromeInstall },
        };

  const adb = env?.adb ?? null;
  const adbCard: EnvCard = !adb
    ? { color: "gray", label: "확인 중", detail: "상태를 불러오는 중…" }
    : adb.connected
      ? { color: "green", label: "연결됨", detail: "디바이스 감지됨" }
      : {
          color: "gray",
          label: "미연결",
          // 백엔드가 원인별로 변환한 사용자용 안내 문구를 노출(개발자용 원문 아님).
          detail: adb.error ?? "감지된 디바이스 없음",
          // 미연결 자체는 정상(회색)이지만, IP 로테이션이 막히는 영향은 별도 경고한다.
          warning:
            "기기 미연결 상태에서는 IP 변경(로테이션)이 동작하지 않습니다.\n휴대폰을 USB로 연결하고 USB 디버깅을 켜 주세요.",
        };

  const envCards = [
    { t: "Chrome", ic: "globe" as const, ...chromeCard },
    { t: "ADB", ic: "bolt" as const, ...adbCard },
  ];

  return (
    <Container size={1020} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={22} wrap="wrap">
        <Box>
          <Title order={1} fz={25} fw={800}>
            알림
          </Title>
          <Text size="sm" c="dimmed" mt={6}>
            게시 배치별 결과와 세부 로그, 시스템 알림을 한곳에서 확인하세요.
          </Text>
        </Box>
        <Button
          size="sm"
          variant="default"
          leftSection={<Icon.download size={16} />}
          onClick={() =>
            notifications.show({
              message: "알림 내역을 엑셀로 내보냈어요",
              color: "green",
            })
          }
        >
          내보내기
        </Button>
      </Group>

      {acct && (
        <Group
          gap={10}
          p={11}
          mb={16}
          style={{
            background: "var(--mantine-color-blue-light)",
            border: "1px solid var(--mantine-color-blue-filled)",
            borderRadius: "var(--mantine-radius-md)",
          }}
        >
          <Icon.filter size={16} color="var(--mantine-color-blue-filled)" />
          <Text size="sm" fw={600}>
            계정 필터 적용됨
          </Text>
          <Group
            gap={7}
            px={10}
            h={28}
            style={{
              background: "var(--mantine-color-body)",
              border: "1px solid var(--mantine-color-gray-3)",
              borderRadius: 999,
            }}
          >
            {acct.platform && <PlatformLogo id={acct.platform} size={17} />}
            <Text fz={12.5} fw={700} ff="monospace">
              {acct.loginId}
            </Text>
          </Group>
          <Button
            ml="auto"
            size="compact-xs"
            variant="default"
            radius="xl"
            leftSection={<Icon.x size={13} />}
            onClick={() => {
              setAcct(null);
              setPlat("all");
            }}
          >
            필터 해제
          </Button>
        </Group>
      )}

      <SimpleGrid cols={3} spacing={14} mb={22}>
        {summary.map((s) => {
          const I = Icon[s.ic];
          return (
            <Card key={s.t} withBorder padding="md" radius="md">
              <Group gap={13} wrap="nowrap">
                <ThemeIcon
                  size={40}
                  radius="md"
                  variant="light"
                  color={s.color}
                >
                  <I size={21} />
                </ThemeIcon>
                <Box>
                  <Text fz={23} fw={800} lh={1}>
                    {s.v}
                  </Text>
                  <Text fz={12.5} c="dimmed" fw={600} mt={4}>
                    {s.t}
                  </Text>
                </Box>
              </Group>
            </Card>
          );
        })}
      </SimpleGrid>

      <Group justify="space-between" align="center" mb={10}>
        <Text fz={13} fw={700} c="dimmed">
          환경 상태
        </Text>
        <Tooltip label="다시 확인" withArrow>
          <ActionIcon
            variant="subtle"
            color="gray"
            aria-label="환경 상태 새로고침"
            loading={envLoading}
            onClick={refreshEnv}
          >
            <Icon.refresh size={16} />
          </ActionIcon>
        </Tooltip>
      </Group>

      <SimpleGrid cols={2} spacing={14} mb={22}>
        {envCards.map((c) => {
          const I = Icon[c.ic];
          return (
            <Card key={c.t} withBorder padding="md" radius="md">
              <Group gap={13} wrap="nowrap">
                <ThemeIcon
                  size={40}
                  radius="md"
                  variant="light"
                  color={c.color}
                >
                  <I size={21} />
                </ThemeIcon>
                <Box style={{ minWidth: 0 }}>
                  <Group gap={8} wrap="nowrap">
                    <Text fz={14} fw={800} lh={1}>
                      {c.t}
                    </Text>
                    <Badge size="sm" color={c.color} variant="light">
                      {c.label}
                    </Badge>
                  </Group>
                  {/* 한 줄로 잘리므로, 전체 사유(특히 에러)는 hover 툴팁으로. */}
                  <Tooltip label={c.detail} multiline maw={460} withArrow>
                    <Text fz={12.5} c="dimmed" fw={600} mt={5} truncate>
                      {c.detail}
                    </Text>
                  </Tooltip>
                  {/* 상태가 자동화에 끼치는 영향 경고(ADB 미연결 → IP 변경 불가). */}
                  {c.warning && (
                    <Group gap={6} mt={8} wrap="nowrap" align="flex-start">
                      <Icon.alert
                        size={14}
                        color="var(--mantine-color-orange-6)"
                        style={{ flexShrink: 0, marginTop: 1 }}
                      />
                      <Text
                        fz={11.5}
                        c="orange.8"
                        fw={600}
                        style={{ whiteSpace: "pre-line" }}
                      >
                        {c.warning}
                      </Text>
                    </Group>
                  )}
                  {/* 바로 취할 조치 버튼(Chrome 미설치 → 설치 페이지 열기). */}
                  {c.action && (
                    <Button
                      size="compact-xs"
                      variant="light"
                      mt={10}
                      leftSection={<Icon.arrowUpRight size={13} />}
                      onClick={c.action.onClick}
                    >
                      {c.action.label}
                    </Button>
                  )}
                </Box>
              </Group>
            </Card>
          );
        })}
      </SimpleGrid>

      <Group justify="space-between" mb={16} wrap="wrap">
        <SegmentedControl
          size="sm"
          value={cat}
          onChange={setCat}
          data={catTabs}
        />
        <Group gap="sm">
          <Select
            size="sm"
            w={120}
            value={status}
            onChange={(v) => setStatus(v ?? "all")}
            data={[
              { value: "all", label: "전체 상태" },
              { value: "success", label: "성공" },
              { value: "partial", label: "일부 실패" },
              { value: "fail", label: "실패" },
            ]}
          />
          {cat !== "system" && (
            <Select
              size="sm"
              w={140}
              value={plat}
              onChange={(v) => {
                const nv = v ?? "all";
                setPlat(nv);
                if (acct && nv !== acct.platform) setAcct(null);
              }}
              data={[
                { value: "all", label: "모든 플랫폼" },
                ...ACTIVE_PLATFORMS.map((p) => ({
                  value: p.id,
                  label: p.name,
                })),
              ]}
            />
          )}
          <TextInput
            size="sm"
            w={200}
            placeholder="내용·종목·계정 검색"
            leftSection={<Icon.search size={16} />}
            value={q}
            onChange={(e) => setQ(e.currentTarget.value)}
          />
        </Group>
      </Group>

      <Card withBorder padding={0} radius="md" style={{ overflow: "hidden" }}>
        {groups.map((g) => (
          <Box key={g.day}>
            <Box
              px={16}
              py={9}
              bg="gray.0"
              style={{ borderBottom: "1px solid var(--mantine-color-gray-2)" }}
            >
              <Text fz={11.5} fw={700} c="dimmed">
                {g.day}
              </Text>
            </Box>
            {g.rows.map((row) =>
              row.kind === "batch" ? (
                <BatchRow
                  key={row.b.id}
                  batch={row.b}
                  expanded={!!expanded[row.b.id]}
                  onToggle={() =>
                    setExpanded((e) => ({ ...e, [row.b.id]: !e[row.b.id] }))
                  }
                />
              ) : (
                <Group
                  key={row.s.id}
                  gap={14}
                  px={16}
                  py={12}
                  wrap="nowrap"
                  style={{
                    borderBottom: "1px solid var(--mantine-color-gray-2)",
                  }}
                >
                  <ThemeIcon
                    size={26}
                    radius="xl"
                    variant="light"
                    color={statusColor(row.s.status)}
                  >
                    {row.s.status === "success" ? (
                      <Icon.check size={15} />
                    ) : row.s.status === "fail" ? (
                      <Icon.x size={15} />
                    ) : (
                      <Icon.bell size={13} />
                    )}
                  </ThemeIcon>
                  <Badge size="sm" color="gray" variant="light">
                    시스템
                  </Badge>
                  <Text
                    fz={13.5}
                    fw={600}
                    c="gray.7"
                    truncate
                    style={{ flex: 1 }}
                  >
                    {row.s.title}
                  </Text>
                  <Text fz={12} c="dimmed" w={70} ta="right">
                    {row.s.time}
                  </Text>
                </Group>
              ),
            )}
          </Box>
        ))}
        {merged.length === 0 && (
          <Center py={56}>
            <Stack align="center" gap={6}>
              <Icon.inbox size={38} color="var(--mantine-color-gray-5)" />
              <Text size="sm" fw={600} c="dimmed">
                해당하는 알림이 없어요
              </Text>
            </Stack>
          </Center>
        )}
      </Card>
    </Container>
  );
}
