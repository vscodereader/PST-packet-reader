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

import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../../api";

import type { ScheduledItem } from "./scheduled-posts";
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

export function PublishCommand({
  onSchedule,
}: {
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [devices, setDevices] = useState<PubDevice[]>(DUMMY_DEVICES);
  const [selDev, setSelDev] = useState<Set<string>>(new Set());
  const [postByDev, setPostByDev] = useState<Record<string, string | null>>({});
  // 게시 대상은 **하위별로** 고른다 — 한 대는 종토, 다른 대는 카페처럼 서로 다를 수 있다.
  const [targetByDev, setTargetByDev] = useState<Record<string, Target>>({});
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

      {/* ② 하위별 게시 대상 + 구성 — 하위마다 대상(종토/카페/…)을 따로 고른다(안 섞임). */}
      {selectedDevices.length > 0 && (
        <Box>
          <Text fw={700} size="sm" mb="xs">
            ② 하위별 게시 대상·구성{" "}
            <Text span c="dimmed" size="xs">
              (하위마다 대상·종목·계정 독립 — 서로 안 섞임)
            </Text>
          </Text>
          <Stack gap="md">
            {selectedDevices.map((d) => (
              <DeviceBlock
                key={d.id}
                device={d}
                postTitle={
                  postByDev[d.id]
                    ? (mockPosts(d.id).find((p) => p.id === postByDev[d.id])
                        ?.title ?? null)
                    : null
                }
                postId={postByDev[d.id] ?? null}
                target={targetByDev[d.id] ?? null}
                onSetTarget={(t) =>
                  setTargetByDev((prev) => ({ ...prev, [d.id]: t }))
                }
                cfg={cfgByDev[d.id] ?? DEFAULT_CFG}
                onPatch={(patch) => patchCfg(d.id, patch)}
                onSchedule={onSchedule}
              />
            ))}
          </Stack>
        </Box>
      )}
    </Stack>
  );
}

// 하위 1대 블록: 기기 헤더 + 게시 대상(하위별) + 대상별 상세 구성. 종토만 상세 구현, 나머지는 추후.
function DeviceBlock({
  device,
  postId,
  postTitle,
  target,
  onSetTarget,
  cfg,
  onPatch,
  onSchedule,
}: {
  device: PubDevice;
  postId: string | null;
  postTitle: string | null;
  target: Target | null;
  onSetTarget: (t: Target) => void;
  cfg: ForumCfg;
  onPatch: (patch: Partial<ForumCfg>) => void;
  onSchedule: (item: ScheduledItem) => void;
}) {
  return (
    <Paper withBorder radius="md" p="md">
      {/* 기기 헤더 + 선택한 글 */}
      <Group gap="xs" mb="sm">
        <ThemeIcon size={26} radius="md" variant="light" color="blue">
          <IconDeviceDesktop size={16} />
        </ThemeIcon>
        <Text fw={700}>{device.name}</Text>
        {postTitle ? (
          <Badge variant="light" color="blue" radius="sm">
            글: {shortTitle(postTitle)}
          </Badge>
        ) : (
          <Badge variant="light" color="gray" radius="sm">
            글을 먼저 선택하세요
          </Badge>
        )}
      </Group>

      {/* 게시 대상 — 하위별(글 선택돼야 활성). 카페/블로그/밴드는 추후(비활성). */}
      <Text size="xs" c="dimmed" mb={4}>
        게시 대상
      </Text>
      <Group gap="xs" mb="sm">
        {TARGETS.map((t) => (
          <Button
            key={t.key}
            size="xs"
            variant={target === t.key ? "filled" : "default"}
            disabled={t.soon || postTitle == null}
            onClick={() => onSetTarget(t.key)}
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

      {/* 대상별 상세 구성 */}
      {target === "forum" && (
        <ForumConfig
          device={device}
          cfg={cfg}
          onPatch={onPatch}
          postId={postId}
          postTitle={postTitle}
          onSchedule={onSchedule}
        />
      )}
      {target != null && target !== "forum" && (
        <Text size="sm" c="dimmed">
          {TARGETS.find((t) => t.key === target)?.label} 상세 구성은 추후
          구현됩니다.
        </Text>
      )}
    </Paper>
  );
}

// 종토 상세 구성(카테고리/시장/종목수/계정 + 4버튼). 외곽 Paper·기기헤더는 DeviceBlock이 제공.
function ForumConfig({
  device,
  cfg,
  onPatch,
  postId,
  postTitle,
  onSchedule,
}: {
  device: PubDevice;
  cfg: ForumCfg;
  onPatch: (patch: Partial<ForumCfg>) => void;
  postId: string | null;
  postTitle: string | null;
  onSchedule: (item: ScheduledItem) => void;
}) {
  // 토론 카테고리는 시장 구분이 없어 전체 고정(데스크톱과 동일 규칙).
  const marketDisabled = cfg.category === "discussion";
  const effectiveMarket: Market = marketDisabled ? "all" : cfg.market;

  // 종목 목록 = 서버 프록시(네이버 공개 front-api) 실데이터. 카테고리/시장이 바뀔 때마다 다시 가져온다.
  // 서버가 종목 코드의 원천 — 여기서 고른 실코드로 게시 명령을 만든다. 오프라인이면 더미 폴백(UI 무손상).
  const [pool, setPool] = useState<SelectableStock[]>(() =>
    mockStocks(cfg.category, effectiveMarket),
  );
  useEffect(() => {
    let cancelled = false;
    api.forumStocks
      .list({ category: cfg.category, exchange: "krx", market: effectiveMarket })
      .then((p) => {
        if (cancelled) return;
        setPool(
          p.stocks.map((s) => ({
            code: s.code,
            name: s.name,
            isHotDiscussion: s.isHotDiscussion,
          })),
        );
      })
      .catch(() => {
        // 서버 오프라인/프록시 실패 → 더미 유지(미리보기 무손상).
        if (!cancelled) setPool(mockStocks(cfg.category, effectiveMarket));
      });
    return () => {
      cancelled = true;
    };
  }, [cfg.category, effectiveMarket]);
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

  // 예약 폼: null=닫힘, false=예약 게시, true=나눠서 예약. 예약 버튼을 누르면 아래에 달력+시간이 뜬다.
  const [armed, setArmed] = useState<boolean | null>(null);
  const [sched, setSched] = useState(() => nowParts());

  const detailFor = (split: boolean) => {
    const names = picked.map((s) => s.name);
    return split
      ? `종목 ${picked.length} · 계정 ${cfg.accounts.length} · 나눠서 ${distributeEvenly(
          names,
          cfg.accounts.length,
        )
          .map((b) => b.length)
          .join("·")}`
      : `종목 ${picked.length} · 계정 ${cfg.accounts.length} 전체`;
  };

  // 서버로 내려보낼 계정×종목 배정. 전체=각 계정이 picked 전부, 나눠서=distributeEvenly로 분배.
  const buildAssignments = (split: boolean) => {
    const stocks = picked.map((s) => ({ code: s.code, name: s.name }));
    if (!split) {
      return cfg.accounts.map((loginId) => ({ loginId, stocks }));
    }
    const slices = distributeEvenly(stocks, cfg.accounts.length);
    return cfg.accounts.map((loginId, i) => ({
      loginId,
      stocks: slices[i] ?? [],
    }));
  };

  const runNow = (split: boolean) => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split,
          assignments: buildAssignments(split),
        });
        notifications.show({
          title: `${device.name} · 게시 명령 전송`,
          message: `글 "${shortTitle(postTitle ?? "")}" · ${detailFor(split)}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 지금${split ? " 나눠서" : ""} 게시(미리보기)`,
            message: `글 "${shortTitle(postTitle ?? "")}" · ${detailFor(split)} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "게시 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const split = armed === true;
    onSchedule({
      id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
      deviceName: device.name,
      postTitle: postTitle ?? "-",
      targetLabel: "종목토론방",
      detail: detailFor(split),
      at: toEpochMs(sched.date, sched.time),
    });
    notifications.show({
      title: `${device.name} 예약 등록`,
      message: `${scheduleMoment(sched.date, sched.time).when} · ${detailFor(split)}`,
      color: "grape",
    });
    setArmed(null);
  };

  return (
    <Box>
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
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!allValid}
            leftSection={<Icon.bolt size={15} />}
            onClick={() => runNow(false)}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed === false ? "filled" : "light"}
            color="grape"
            disabled={!allValid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(false)}
          >
            예약 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant="light"
            disabled={!canDistribute}
            leftSection={<Icon.send size={15} />}
            onClick={() => runNow(true)}
          >
            나눠서 즉시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed === true ? "filled" : "light"}
            color="grape"
            disabled={!canDistribute}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            나눠서 예약
          </Button>
        </Group>

        {/* 예약 버튼을 누르면 바로 밑에 달력+시간(공유 DateTimePicker 재사용). 확정하면 '예약된 글'로. */}
        {armed != null && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              {armed ? "나눠서 예약" : "예약 게시"} — 게시 시각 선택
            </Text>
            <Group gap="sm" wrap="wrap">
              <DateTimePicker
                date={sched.date}
                time={sched.time}
                onChange={setSched}
              />
              <Button size="sm" color="grape" onClick={confirmSchedule}>
                예약 확정
              </Button>
              <Button
                size="sm"
                variant="subtle"
                color="gray"
                onClick={() => setArmed(null)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}

        {!canDistribute && cfg.accounts.length > 1 && picked.length > 0 && (
          <Text fz={11} c="dimmed">
            나눠서 게시는 계정 2개 이상 + 종목 2개 이상이고, 종목 수가 계정 수
            이상일 때 켜집니다.
          </Text>
        )}
      </Stack>
    </Box>
  );
}
