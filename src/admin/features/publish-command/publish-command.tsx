import {
  Badge,
  Box,
  Button,
  Group,
  NumberInput,
  Paper,
  ScrollArea,
  Select,
  SimpleGrid,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useMemo, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api } from "../../api";

import {
  distributeEvenly,
  isExcludedByName,
  pickStocks,
  type SelectableStock,
} from "./stock-select";

// 게시 명령(설계서 07). **종토만 확정** — 카페/블로그/밴드는 버튼만(추후). UI 먼저 완성 단계라,
// 서버에 아직 없는 데이터(하위 글목록·성공계정·종목 미리보기, §8 신규 데이터흐름)는 Admin 다른
// 화면과 동일하게 **오프라인 더미**로 폴백해 화면을 완성한다(실데이터 배선은 서버 프록시 후속).

interface PubDevice {
  id: string;
  name: string;
  ip: string;
}

type Target = "forum" | "cafe" | "blog" | "band";
const TARGETS: { key: Target; label: string; soon: boolean }[] = [
  { key: "forum", label: "종목토론방", soon: false },
  { key: "cafe", label: "네이버카페", soon: true },
  { key: "blog", label: "네이버블로그", soon: true },
  { key: "band", label: "네이버밴드", soon: true },
];

// 카테고리/시장 — 데스크톱 종목선택(stock-crawl-modal)과 1:1 동일.
type Category =
  | "discussion"
  | "tradingValue"
  | "popular"
  | "rising"
  | "falling"
  | "volume";
const CATEGORIES: { key: Category; label: string }[] = [
  { key: "discussion", label: "토론" },
  { key: "tradingValue", label: "거래대금" },
  { key: "popular", label: "인기종목" },
  { key: "rising", label: "상승" },
  { key: "falling", label: "하락" },
  { key: "volume", label: "거래량" },
];
type Market = "all" | "kospi" | "kosdaq";
const MARKETS: { key: Market; label: string }[] = [
  { key: "all", label: "전체" },
  { key: "kospi", label: "코스피" },
  { key: "kosdaq", label: "코스닥" },
];

interface ForumCfg {
  category: Category;
  market: Market;
  count: number | "";
  accounts: string[];
}
const DEFAULT_CFG: ForumCfg = {
  category: "tradingValue",
  market: "all",
  count: "",
  accounts: [],
};

// ── 미리보기 더미(서버 프록시 배선 전) ──
const DUMMY_DEVICES: PubDevice[] = [
  { id: "d1", name: "하위-001", ip: "1.2.3.4" },
  { id: "d2", name: "하위-002", ip: "1.2.3.5" },
  { id: "d3", name: "하위-003", ip: "1.2.3.6" },
];
function mockPosts(deviceId: string): { id: string; title: string }[] {
  return [
    { id: `${deviceId}-p1`, title: "오늘의 급등주 분석과 전망" },
    { id: `${deviceId}-p2`, title: "반도체 섹터 단기 대응 전략" },
    { id: `${deviceId}-p3`, title: "코스닥 중소형주 모멘텀 점검" },
  ];
}
function mockAccounts(deviceId: string): string[] {
  const base = ["stock_id041", "invest_king7", "money_flow22", "trader_lee9"];
  // 하위마다 살짝 다르게 — 섞이지 않음을 눈으로 보이게.
  return base.map((a) => `${a}_${deviceId.slice(-1)}`);
}
// 종목 미리보기 더미 — 불꽃🔥 섞고, 삼성전자/하이닉스도 넣어 제외가 눈에 보이게 한다.
const MOCK_STOCK_POOL: SelectableStock[] = [
  { code: "005930", name: "삼성전자", isHotDiscussion: true },
  { code: "000660", name: "SK하이닉스", isHotDiscussion: true },
  { code: "005380", name: "현대차", isHotDiscussion: true },
  { code: "035720", name: "카카오", isHotDiscussion: true },
  { code: "035420", name: "NAVER", isHotDiscussion: true },
  { code: "247540", name: "에코프로비엠", isHotDiscussion: true },
  { code: "086520", name: "에코프로", isHotDiscussion: false },
  { code: "051910", name: "LG화학", isHotDiscussion: false },
  { code: "006400", name: "삼성SDI", isHotDiscussion: false },
  { code: "207940", name: "삼성바이오로직스", isHotDiscussion: false },
  { code: "005490", name: "POSCO홀딩스", isHotDiscussion: false },
  { code: "373220", name: "LG에너지솔루션", isHotDiscussion: false },
  { code: "000270", name: "기아", isHotDiscussion: false },
  { code: "068270", name: "셀트리온", isHotDiscussion: false },
  { code: "323410", name: "카카오뱅크", isHotDiscussion: false },
];
function mockStocks(category: Category, market: Market): SelectableStock[] {
  // 카테고리/시장에 따라 순서만 살짝 회전해 "바뀌는 느낌"을 준다(미리보기용).
  const shift =
    (CATEGORIES.findIndex((c) => c.key === category) + 1) *
    (MARKETS.findIndex((m) => m.key === market) + 1);
  return MOCK_STOCK_POOL.map(
    (_, i) => MOCK_STOCK_POOL[(i + shift) % MOCK_STOCK_POOL.length]!,
  );
}

/** ID 마스킹: 앞 2글자 + • (설계 §10-4-1). */
export function maskId(loginId: string): string {
  return loginId.slice(0, 2) + "•".repeat(Math.max(0, loginId.length - 2));
}
/** 글 라벨 6자 말줄임. */
export function shortTitle(title: string): string {
  return title.length > 6 ? `${title.slice(0, 6)}…` : title;
}

export function PublishCommand() {
  const [devices, setDevices] = useState<PubDevice[]>(DUMMY_DEVICES);
  const [selDev, setSelDev] = useState<Set<string>>(new Set());
  const [postByDev, setPostByDev] = useState<Record<string, string | null>>({});
  const [target, setTarget] = useState<Target | null>(null);
  const [cfgByDev, setCfgByDev] = useState<Record<string, ForumCfg>>({});

  // online 하위 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지). "성공 계정 보유" 필터는
  // 서버 신규 플래그(§8) 배선 후 추가 — 지금은 connected 만.
  useEffect(() => {
    api.devices
      .list()
      .then((list) =>
        setDevices(
          list
            .filter((d) => d.connected)
            .map((d) => ({ id: d.id, name: d.name, ip: d.ip ?? "-" })),
        ),
      )
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  }, []);

  const toggleDev = (id: string) =>
    setSelDev((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        setCfgByDev((c) => (c[id] ? c : { ...c, [id]: { ...DEFAULT_CFG } }));
      }
      return next;
    });

  const patchCfg = (id: string, patch: Partial<ForumCfg>) =>
    setCfgByDev((prev) => ({
      ...prev,
      [id]: { ...(prev[id] ?? DEFAULT_CFG), ...patch },
    }));

  const selectedDevices = devices.filter((d) => selDev.has(d.id));

  return (
    <Stack gap="lg" p="md" h="100%">
      <Box>
        <Text fw={800} size="xl">
          게시 명령
        </Text>
        <Text size="sm" c="dimmed">
          하위 컴퓨터당 1묶음 — 글·종목·계정은 하위끼리 절대 섞이지 않습니다.
          실제 게시는 각 하위가 자기 IP로 수행합니다.
        </Text>
      </Box>

      {/* ① 하위 COM 카드(4열) + 글 선택 */}
      <Box>
        <Text fw={700} size="sm" mb="xs">
          ① 하위 선택 + 글 선택{" "}
          <Text span c="dimmed" size="xs">
            (분배+로그인 완료된 online 하위)
          </Text>
        </Text>
        <SimpleGrid cols={4} spacing="sm">
          {devices.map((d) => {
            const on = selDev.has(d.id);
            return (
              <Stack key={d.id} gap={6}>
                <Paper
                  withBorder
                  radius="md"
                  p="sm"
                  onClick={() => toggleDev(d.id)}
                  style={{
                    cursor: "pointer",
                    borderColor: on ? "var(--mantine-color-blue-6)" : undefined,
                    borderWidth: on ? 2 : 1,
                    background: on ? "var(--mantine-color-blue-0)" : undefined,
                  }}
                >
                  <Group gap="sm" wrap="nowrap">
                    <ThemeIcon
                      size={38}
                      radius="md"
                      variant="light"
                      color={on ? "blue" : "gray"}
                    >
                      <IconDeviceDesktop size={22} />
                    </ThemeIcon>
                    <Box style={{ minWidth: 0 }}>
                      <Text fw={700} size="sm" truncate>
                        {d.name}
                      </Text>
                      <Text size="xs" c="dimmed" truncate>
                        IP {d.ip}
                      </Text>
                    </Box>
                  </Group>
                </Paper>
                <Select
                  size="xs"
                  disabled={!on}
                  placeholder="글을 선택하세요"
                  value={postByDev[d.id] ?? null}
                  onChange={(v) =>
                    setPostByDev((prev) => ({ ...prev, [d.id]: v }))
                  }
                  data={mockPosts(d.id).map((p) => ({
                    value: p.id,
                    label: shortTitle(p.title),
                  }))}
                  comboboxProps={{ withinPortal: true }}
                  aria-label={`${d.name} 글 선택`}
                />
              </Stack>
            );
          })}
        </SimpleGrid>
      </Box>

      {/* ② 게시 대상 4버튼 */}
      <Box>
        <Text fw={700} size="sm" mb="xs">
          ② 게시 대상
        </Text>
        <Group gap="xs">
          {TARGETS.map((t) => (
            <Button
              key={t.key}
              variant={target === t.key ? "filled" : "default"}
              disabled={t.soon || selDev.size === 0}
              onClick={() => setTarget(t.key)}
              rightSection={
                t.soon ? (
                  <Badge size="xs" color="gray" variant="light">
                    추후
                  </Badge>
                ) : undefined
              }
            >
              {t.label}
            </Button>
          ))}
        </Group>
      </Box>

      {/* ③ (종토) 하위별 독립 패널 */}
      {target === "forum" && selectedDevices.length > 0 && (
        <Box>
          <Text fw={700} size="sm" mb="xs">
            ③ 하위별 종목·계정 구성{" "}
            <Text span c="dimmed" size="xs">
              (하위마다 독립 — 서로 안 섞임)
            </Text>
          </Text>
          <Stack gap="md">
            {selectedDevices.map((d) => (
              <ForumPanel
                key={d.id}
                device={d}
                cfg={cfgByDev[d.id] ?? DEFAULT_CFG}
                onPatch={(patch) => patchCfg(d.id, patch)}
                postTitle={
                  postByDev[d.id]
                    ? (mockPosts(d.id).find((p) => p.id === postByDev[d.id])
                        ?.title ?? null)
                    : null
                }
              />
            ))}
          </Stack>
        </Box>
      )}
    </Stack>
  );
}

function ForumPanel({
  device,
  cfg,
  onPatch,
  postTitle,
}: {
  device: PubDevice;
  cfg: ForumCfg;
  onPatch: (patch: Partial<ForumCfg>) => void;
  postTitle: string | null;
}) {
  // 토론 카테고리는 시장 구분이 없어 전체 고정(데스크톱과 동일 규칙).
  const marketDisabled = cfg.category === "discussion";
  const effectiveMarket: Market = marketDisabled ? "all" : cfg.market;

  const pool = useMemo(
    () => mockStocks(cfg.category, effectiveMarket),
    [cfg.category, effectiveMarket],
  );
  const n = typeof cfg.count === "number" ? cfg.count : 0;
  const { picked, error } = useMemo(() => pickStocks(pool, n), [pool, n]);

  const accounts = mockAccounts(device.id);

  // 게시 실행 조건. 즉시/예약(각 계정 전체 종목)은 글·종목수·계정만 있으면 됨. 나눠서(균등분배)는
  // 데스크톱과 동일: 계정 2개↑ + 종목 2개↑ + 종목수 ≥ 계정수(#267-5).
  const allValid =
    postTitle != null &&
    typeof cfg.count === "number" &&
    cfg.count > 0 &&
    cfg.accounts.length > 0;
  const canDistribute =
    allValid &&
    cfg.accounts.length >= 2 &&
    picked.length >= 2 &&
    picked.length >= cfg.accounts.length;

  const run = (timing: "now" | "schedule", split: boolean) => {
    const names = picked.map((s) => s.name);
    const detail = split
      ? `나눠서 ${cfg.accounts.length}계정 분배 ${distributeEvenly(
          names,
          cfg.accounts.length,
        )
          .map((b) => b.length)
          .join("·")}`
      : `계정마다 ${names.length}종목 전체`;
    notifications.show({
      title: `${device.name} · ${timing === "now" ? "지금 바로" : "예약"}${
        split ? " 나눠서" : ""
      } 게시(미리보기)`,
      message: `글 "${shortTitle(postTitle ?? "")}" · ${detail}`,
      color: "blue",
    });
  };

  return (
    <Paper withBorder radius="md" p="md">
      <Group gap="xs" mb="sm">
        <ThemeIcon size={26} radius="md" variant="light" color="blue">
          <IconDeviceDesktop size={16} />
        </ThemeIcon>
        <Text fw={700}>{device.name}</Text>
      </Group>

      {/* 카테고리 6버튼 */}
      <Text size="xs" c="dimmed" mb={4}>
        카테고리
      </Text>
      <Group gap={6} mb="sm">
        {CATEGORIES.map((c) => (
          <Button
            key={c.key}
            size="xs"
            variant={cfg.category === c.key ? "filled" : "default"}
            onClick={() => onPatch({ category: c.key })}
          >
            {c.label}
          </Button>
        ))}
      </Group>

      {/* 시장 3버튼(토론이면 전체 고정) */}
      <Text size="xs" c="dimmed" mb={4}>
        시장 {marketDisabled && "(토론은 전체 고정)"}
      </Text>
      <Group gap={6} mb="sm">
        {MARKETS.map((m) => (
          <Button
            key={m.key}
            size="xs"
            variant={effectiveMarket === m.key ? "filled" : "default"}
            disabled={marketDisabled && m.key !== "all"}
            onClick={() => onPatch({ market: m.key })}
          >
            {m.label}
          </Button>
        ))}
      </Group>

      {/* 종목 수 N + 미리보기 */}
      <Group align="flex-end" gap="sm" mb="xs">
        <NumberInput
          size="xs"
          label="종목 수"
          placeholder="N"
          min={1}
          w={120}
          value={cfg.count}
          onChange={(v) => onPatch({ count: typeof v === "number" ? v : "" })}
        />
        {error && (
          <Text size="xs" c="red" fw={600}>
            {error}
          </Text>
        )}
      </Group>
      <ScrollArea.Autosize mah={140} mb="sm">
        <Group gap={6}>
          {picked.length === 0 ? (
            <Text size="xs" c="dimmed">
              종목 수를 입력하면 불꽃🔥 우선으로 자동
              선택됩니다(삼성전자·하이닉스 제외).
            </Text>
          ) : (
            picked.map((s) => (
              <Badge
                key={s.code}
                variant="light"
                color={s.isHotDiscussion ? "orange" : "gray"}
                radius="sm"
              >
                {s.isHotDiscussion ? "🔥 " : ""}
                {s.name}
              </Badge>
            ))
          )}
        </Group>
      </ScrollArea.Autosize>

      {/* 계정 선택 — 이 하위의 성공(Active) 계정만. 다중 선택이라 버튼 토글 목록으로 처리. */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 로그인 성공 계정만 · {cfg.accounts.length}명 선택)
      </Text>
      <Group gap={6}>
        {accounts.map((a) => {
          const on = cfg.accounts.includes(a);
          return (
            <Button
              key={a}
              size="xs"
              variant={on ? "filled" : "default"}
              color={on ? "blue" : "gray"}
              onClick={() =>
                onPatch({
                  accounts: on
                    ? cfg.accounts.filter((x) => x !== a)
                    : [...cfg.accounts, a],
                })
              }
            >
              {maskId(a)}
            </Button>
          );
        })}
      </Group>

      {/* 제외 안내 */}
      {pool.some((s) => isExcludedByName(s.name)) && (
        <Text size="xs" c="dimmed" mt="sm">
          제외: 이름에 “삼성전자”·“하이닉스” 포함 종목은 후보에서 자동
          제외됩니다.
        </Text>
      )}

      {/* ④ 게시 실행 — 데스크톱 pstmacro와 동일한 4버튼(#267-5). 하위마다 독립 발행(안 섞임):
          즉시/예약 = 각 계정이 선택 종목 전체 게시, 나눠서 = 종목을 계정 수만큼 균등 분배. */}
      <Stack gap={8} mt="md">
        <Group grow>
          <Button
            disabled={!allValid}
            leftSection={<Icon.bolt size={16} />}
            onClick={() => run("now", false)}
          >
            지금 바로 게시
          </Button>
          <Button
            variant="light"
            color="grape"
            disabled={!allValid}
            leftSection={<Icon.calendar size={16} />}
            onClick={() => run("schedule", false)}
          >
            예약 게시
          </Button>
        </Group>
        <Button
          variant="light"
          fullWidth
          disabled={!canDistribute}
          leftSection={<Icon.send size={16} />}
          onClick={() => run("now", true)}
        >
          나눠서 즉시 게시하기
          {canDistribute
            ? ` (${cfg.accounts.length}계정 · ${picked.length}종목)`
            : ""}
        </Button>
        <Button
          variant="light"
          color="grape"
          fullWidth
          disabled={!canDistribute}
          leftSection={<Icon.calendar size={16} />}
          onClick={() => run("schedule", true)}
        >
          나눠서 게시 예약하기
        </Button>
        {!canDistribute && cfg.accounts.length > 1 && picked.length > 0 && (
          <Text fz={11} c="dimmed">
            나눠서 게시는 계정 2개 이상 + 종목 2개 이상이고, 종목 수가 계정 수
            이상일 때 켜집니다.
          </Text>
        )}
      </Stack>
    </Paper>
  );
}
