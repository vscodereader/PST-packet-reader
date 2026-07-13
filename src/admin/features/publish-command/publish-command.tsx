import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Group,
  NumberInput,
  Paper,
  ScrollArea,
  Select,
  SimpleGrid,
  Stack,
  Text,
  Textarea,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useMemo, useState } from "react";

import { parseCafeArticleUrl } from "@/features/posts/comment-jobs";
import {
  parseBlogLink,
  parseBlogPostLink,
  parseCafeBoardLink,
  parseClipLink,
} from "@/features/posts/publish-helpers";
import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";

import { api, isOffline, type InvAccountDto } from "../../api";

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

// 글 종류(사용자 요청 2026-07-06) — 하위 선택 후 먼저 고른다. 고른 종류의 글만 목록에 뜬다.
// post=글, comment=댓글(종토=특정게시글), both=글+댓글.
type PostKind = "post" | "comment" | "both";
const KIND_OPTS: { value: PostKind; label: string }[] = [
  { value: "post", label: "글" },
  { value: "comment", label: "댓글" },
  { value: "both", label: "글+댓글" },
];
/** 글의 종류(없으면 post로 본다 — 옛 하위 하위호환). */
function postKindOf(p: { kind?: string }): PostKind {
  return p.kind === "comment" || p.kind === "both" ? p.kind : "post";
}

// 카페·밴드 댓글 대상 모드. url=특정 글 URL, latest=최신 N개, popular=인기 N개.
// (엔진 지원: 카페 collect_comment_targets / 밴드 band_comment·band_comment_on_post.)
export type CommentTargetMode = "url" | "latest" | "popular";
const COMMENT_MODE_OPTS: { value: CommentTargetMode; label: string }[] = [
  { value: "url", label: "특정 글" },
  { value: "latest", label: "최신글" },
  { value: "popular", label: "인기글" },
];

/** 카페·밴드 게시 명령에 실을 댓글 대상 필드(commentMode/commentCount)를 만든다. 순수 함수라
 *  단위 테스트로 검증한다. 댓글(comment) 모드에서만 대상 모드를 싣고, 최신/인기일 때만 개수를
 *  싣는다(특정 글·글·글+댓글은 개수 무의미). 개수는 최소 1로 보정한다. */
export function commentTargetPayload(
  kind: PostKind,
  mode: CommentTargetMode,
  count: number,
): { commentMode?: string; commentCount?: number } {
  if (kind !== "comment") return {};
  if (mode === "url") return { commentMode: "url" };
  const n = Number.isFinite(count) ? Math.floor(count) : 1;
  return { commentMode: mode, commentCount: Math.max(1, n) };
}

/** 종토 "특정 게시글" 댓글 나눠서 게시(#403)가 겹침 없이 1:1로 떨어지는 조건(순수 함수).
 *  데스크톱(publish-modal `canCommentDistribute`)과 동일하게 **작성 댓글 수 == 계정 수**이고
 *  둘 다 1 이상일 때만 참이다. 그때만 링크마다 댓글 풀이 계정에 정확히 1개씩 배정된다.
 *  ⚠️ Admin 인벤토리(InvPostDto)에는 글 본문 excerpt만 있고 저장된 댓글 배열이 없어 이 화면에서는
 *  실제 댓글 수를 알 수 없다. 그래서 UI 버튼은 이 헬퍼 대신 URL·계정 존재만으로 노출한다(아래
 *  ForumCommentConfig 주석 참조). 이 헬퍼는 의도한 1:1 규칙의 명세/테스트용이다. */
export function canForumCommentDistribute(
  commentCount: number,
  accountCount: number,
): boolean {
  return commentCount > 0 && accountCount > 0 && commentCount === accountCount;
}

type Target = "forum" | "cafe" | "blog" | "clip" | "band";

/** 게시 대상(target)에 맞는 계정 loginId 목록을 하위 인벤토리 accountRows에서 거른다(순수 함수).
 *  rows가 없거나 비면 null → 호출부가 오프라인 더미로 폴백한다. platform 매핑:
 *  - cafe: platform=="naver" 전부(상태 무관 — 카페는 게시 순간 재로그인하므로 로그인 성공/실패를 안 본다).
 *  - forum/blog/clip/band: platform==그 target && status=="active"만(로그인 성공 계정만).
 *    forum 계정은 platform이 "forum"(빈값도 forum으로 본다 — 옛 하위 하위호환).
 *  이렇게 해야 각 게시 대상 화면이 자기 platform 계정만 보여준다(카페·블로그가 종토 목록에 섞이지 않음). */
export function filterAccountsByTarget(
  target: Target,
  rows: InvAccountDto[] | undefined,
): string[] | null {
  if (!rows || rows.length === 0) return null;
  if (target === "cafe") {
    return rows.filter((r) => r.platform === "naver").map((r) => r.loginId);
  }
  return rows
    .filter((r) => (r.platform ?? "forum") === target && r.status === "active")
    .map((r) => r.loginId);
}
const TARGETS: { key: Target; label: string; soon: boolean }[] = [
  { key: "forum", label: "종목토론방", soon: false },
  { key: "cafe", label: "네이버카페", soon: false },
  { key: "blog", label: "네이버블로그", soon: false },
  { key: "clip", label: "네이버클립", soon: false },
  { key: "band", label: "네이버밴드", soon: false },
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
  // 게시 후 내용변경(종토 글 전용, 15-기타명령 §4). enabled=false면 payload에 싣지 않는다.
  contentChange: {
    enabled: boolean;
    title: string;
    body: string;
    delaySec: number;
  };
}
const DEFAULT_CFG: ForumCfg = {
  category: "tradingValue",
  market: "all",
  count: "",
  accounts: [],
  contentChange: { enabled: false, title: "", body: "", delaySec: 0 },
};

// ── 미리보기 더미(서버 프록시 배선 전) ──
const DUMMY_DEVICES: PubDevice[] = [
  { id: "d1", name: "하위-001", ip: "1.2.3.4" },
  { id: "d2", name: "하위-002", ip: "1.2.3.5" },
  { id: "d3", name: "하위-003", ip: "1.2.3.6" },
];
function mockPosts(
  deviceId: string,
): { id: string; title: string; kind: PostKind; excerpt?: string }[] {
  return [
    { id: `${deviceId}-p1`, title: "오늘의 급등주 분석과 전망", kind: "post" },
    // 댓글은 제목 없음 → 내용(excerpt)이 보이는지 미리보기로 보여준다.
    {
      id: `${deviceId}-p2`,
      title: "제목 없음",
      kind: "comment",
      excerpt: "오늘 흐름 좋네요 👍 관심종목 추가요",
    },
    { id: `${deviceId}-p3`, title: "코스닥 모멘텀 글+댓글", kind: "both" },
  ];
}
function mockAccounts(deviceId: string): string[] {
  const base = ["stock_id041", "invest_king7", "money_flow22", "trader_lee9"];
  // 하위마다 살짝 다르게 — 섞이지 않음을 눈으로 보이게.
  return base.map((a) => `${a}_${deviceId.slice(-1)}`);
}
// 카페 계정 더미(오프라인 미리보기) — 카페는 로그인 실패해도 보이므로 상태 무관하게 몇 개 보여준다.
function mockCafeAccounts(deviceId: string): string[] {
  const base = ["cafe_writer1", "cafe_pen22", "cafe_daily9"];
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
/**
 * 목록/배지에 보여줄 글 라벨. 댓글은 제목이 없으니(당연) title이 비거나 "제목 없음"이라
 * 작성한 댓글 내용(excerpt)을 제목 대신 보여준다 — 글이 제목을 보여주는 것과 똑같이.
 * 글/글+댓글은 제목이 의미 있으므로 그대로 제목을 쓴다(내용 없으면 title로 폴백).
 */
export function postDisplay(p: {
  title: string;
  kind?: string;
  excerpt?: string;
}): string {
  if (postKindOf(p) === "comment") {
    const content = (p.excerpt ?? "").trim();
    if (content) return content;
  }
  return p.title;
}

export function PublishCommand({
  onSchedule,
}: {
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [devices, setDevices] = useState<PubDevice[]>(DUMMY_DEVICES);
  const [selDev, setSelDev] = useState<Set<string>>(new Set());
  // 하위별 실데이터 인벤토리(글목록·성공계정) — 서버가 있으면 채워지고, 없으면 더미로 폴백(UI 무손상).
  const [invByDev, setInvByDev] = useState<
    Record<
      string,
      {
        posts: {
          id: string;
          title: string;
          kind?: string;
          excerpt?: string;
          commentCount?: number;
        }[];
        accounts: string[];
        // 전체 계정(platform·status) — 카페는 로그인 무관 카페 계정을 전부 보여준다.
        accountRows: { loginId: string; platform?: string; status?: string }[];
      }
    >
  >({});
  const [postByDev, setPostByDev] = useState<Record<string, string | null>>({});
  // 하위별 선택한 글 종류(글/댓글/글+댓글). 종류를 바꾸면 그 종류 글만 목록에 뜬다.
  const [kindByDev, setKindByDev] = useState<Record<string, PostKind>>({});
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

  // 하위별 인벤토리(글목록·성공계정) 로드 — 하위가 서버로 보고한 실데이터. 오프라인/미보고면
  // 그 하위는 채우지 않아 더미로 폴백된다(postsFor/accountsFor).
  //
  // **주기 폴링(4초)** — 하위 COM에서 글/댓글을 지우거나 추가하면 하위가 서버로 재보고하고,
  // Admin이 폴링으로 최신본을 다시 읽어 **즉석 반영**한다(다른 페이지 갔다 오지 않아도 갱신).
  // (다른 Admin 화면 — 중지 명령·통신로그 — 과 동일한 폴링 패턴.)
  useEffect(() => {
    let cancelled = false;
    const loadInventory = () => {
      devices.forEach((d) => {
        api.devices
          .inventory(d.id)
          .then((inv) => {
            if (cancelled) return;
            setInvByDev((prev) => ({
              ...prev,
              [d.id]: {
                posts: inv.posts,
                accounts: inv.accounts,
                accountRows: inv.accountRows ?? [],
              },
            }));
          })
          .catch(() => {
            /* 오프라인/미보고 → 그 하위는 더미 폴백 */
          });
      });
    };
    loadInventory();
    const id = window.setInterval(loadInventory, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [devices]);

  // 실데이터 우선, 없으면(오프라인/미보고) 더미. 글목록이 비어있어도 보고된 것이면 실데이터로 본다.
  const postsFor = (
    deviceId: string,
  ): {
    id: string;
    title: string;
    kind?: string;
    excerpt?: string;
    commentCount?: number;
  }[] => invByDev[deviceId]?.posts ?? mockPosts(deviceId);
  // 선택한 글의 작성 댓글 수(≥2 게이트용, 15-기타명령 §3). 글 미선택/미보고면 0.
  const commentCountFor = (deviceId: string): number => {
    const pid = postByDev[deviceId];
    if (!pid) return 0;
    return postsFor(deviceId).find((x) => x.id === pid)?.commentCount ?? 0;
  };
  // 선택한 글 종류의 글만(사용자 요청: 종류별로 안 섞이게).
  const postsForKind = (deviceId: string, kind: PostKind) =>
    postsFor(deviceId).filter((p) => postKindOf(p) === kind);
  // 선택한 글의 표시 라벨(댓글=작성한 댓글 내용, 글=제목). null=아직 글 미선택.
  const postLabelFor = (deviceId: string): string | null => {
    const pid = postByDev[deviceId];
    if (!pid) return null;
    const p = postsFor(deviceId).find((x) => x.id === pid);
    return p ? postDisplay(p) : null;
  };
  // 게시 대상별 계정 목록. 실데이터(accountRows) 있으면 filterAccountsByTarget로 platform별로
  // 거르고, 없으면(오프라인/미보고) 더미로 폴백. 종토도 platform=="forum"만 보이게(카페/블로그가
  // 종토 목록에 섞이던 문제 수정 — 각 target은 자기 platform 계정만).
  const accountsFor = (deviceId: string, target: Target | null): string[] => {
    if (!target) return invByDev[deviceId]?.accounts ?? mockAccounts(deviceId);
    const filtered = filterAccountsByTarget(target, invByDev[deviceId]?.accountRows);
    if (filtered !== null) return filtered;
    // 오프라인/미보고 → 더미 폴백(카페는 전용 더미).
    return target === "cafe" ? mockCafeAccounts(deviceId) : mockAccounts(deviceId);
  };

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
            const devKind = kindByDev[d.id] ?? null;
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
                {/* 글 종류 먼저 고른다 → 그 종류 글만 아래 목록에 뜬다(안 섞임). */}
                <Select
                  size="xs"
                  disabled={!on}
                  placeholder="글 종류를 고르세요"
                  value={devKind}
                  onChange={(v) => {
                    if (!v) return;
                    setKindByDev((prev) => ({
                      ...prev,
                      [d.id]: v as PostKind,
                    }));
                    // 종류가 바뀌면 이전에 고른 글 선택을 초기화(다른 종류 글이 남지 않게).
                    setPostByDev((prev) => ({ ...prev, [d.id]: null }));
                  }}
                  data={KIND_OPTS}
                  comboboxProps={{ withinPortal: true }}
                  aria-label={`${d.name} 글 종류 선택`}
                />
                <Select
                  size="xs"
                  disabled={!on || devKind == null}
                  placeholder={
                    devKind == null ? "글 종류 먼저" : "글을 선택하세요"
                  }
                  value={postByDev[d.id] ?? null}
                  onChange={(v) =>
                    setPostByDev((prev) => ({ ...prev, [d.id]: v }))
                  }
                  data={
                    devKind != null
                      ? postsForKind(d.id, devKind).map((p) => ({
                          value: p.id,
                          // 댓글은 제목이 없으니 작성한 댓글 내용을 보여준다(글=제목).
                          label: shortTitle(postDisplay(p)),
                        }))
                      : []
                  }
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
                kind={kindByDev[d.id] ?? "post"}
                postTitle={postLabelFor(d.id)}
                postId={postByDev[d.id] ?? null}
                postCommentCount={commentCountFor(d.id)}
                accounts={accountsFor(d.id, targetByDev[d.id] ?? null)}
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
  kind,
  postId,
  postTitle,
  postCommentCount,
  accounts,
  target,
  onSetTarget,
  cfg,
  onPatch,
  onSchedule,
}: {
  device: PubDevice;
  kind: PostKind;
  postId: string | null;
  postTitle: string | null;
  postCommentCount: number;
  accounts: string[];
  target: Target | null;
  onSetTarget: (t: Target) => void;
  cfg: ForumCfg;
  onPatch: (patch: Partial<ForumCfg>) => void;
  onSchedule: (item: ScheduledItem) => void;
}) {
  const kindLabel = KIND_OPTS.find((k) => k.value === kind)?.label ?? "글";
  return (
    <Paper withBorder radius="md" p="md">
      {/* 기기 헤더 + 선택한 글 */}
      <Group gap="xs" mb="sm">
        <ThemeIcon size={26} radius="md" variant="light" color="blue">
          <IconDeviceDesktop size={16} />
        </ThemeIcon>
        <Text fw={700}>{device.name}</Text>
        <Badge variant="light" color="grape" radius="sm">
          {kindLabel}
        </Badge>
        {postTitle ? (
          <Badge variant="light" color="blue" radius="sm">
            {kindLabel}: {shortTitle(postTitle)}
          </Badge>
        ) : (
          <Badge variant="light" color="gray" radius="sm">
            {kindLabel} 선택하세요
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

      {/* 대상별 상세 구성 — 종토는 종류에 따라 다르다: 글/글+댓글=종목 게시, 댓글=특정게시글 URL. */}
      {target === "forum" && kind !== "comment" && (
        <ForumConfig
          device={device}
          mode={kind}
          cfg={cfg}
          onPatch={onPatch}
          postId={postId}
          postTitle={postTitle}
          postCommentCount={postCommentCount}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
      {target === "forum" && kind === "comment" && (
        <ForumCommentConfig
          device={device}
          postId={postId}
          postTitle={postTitle}
          postCommentCount={postCommentCount}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
      {/* 카페: 글/댓글/글+댓글 모두 게시판 링크 입력(요구서). 댓글 대상/개수는 글에 동결. */}
      {target === "cafe" && (
        <CafeConfig
          device={device}
          kind={kind}
          postId={postId}
          postTitle={postTitle}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
      {/* 블로그: 댓글 전용(특정 게시글 / 최신글 / 인기글). 데스크톱 blog 카드 미러. */}
      {target === "blog" && (
        <BlogConfig
          device={device}
          postId={postId}
          postTitle={postTitle}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
      {/* 클립: 댓글 전용·최신 N개(창작자 링크). 데스크톱 clip 카드 미러. */}
      {target === "clip" && (
        <ClipConfig
          device={device}
          postId={postId}
          postTitle={postTitle}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
      {/* 밴드: 글/댓글/글+댓글 모두 밴드 링크. 댓글 대상/개수는 글에 동결. 데스크톱 band 카드 미러. */}
      {target === "band" && (
        <BandConfig
          device={device}
          kind={kind}
          postId={postId}
          postTitle={postTitle}
          accounts={accounts}
          onSchedule={onSchedule}
        />
      )}
    </Paper>
  );
}

// 닉네임 랜덤(15-기타명령 §3·§6-2) 계정별 변경 가능 잔여 횟수 실시간 조회 훅. 켜지면(enabled)
// 각 계정을 "확인 중"으로 먼저 표시하고, 하위에 원격 조회를 요청(POST)한 뒤 회신을 1.5초 간격으로
// 폴링(GET)해 채운다. loginId → 남은횟수 | "loading"(확인 중) | "error"(확인 실패). 언마운트·의존성
// 변경 시 늦은 응답이 상태를 덮지 않도록 cancelled로 가드한다. ForumConfig·ForumCommentConfig가 공유.
function useNicknameRemaining(
  deviceId: string,
  accounts: string[],
  enabled: boolean,
): Record<string, number | "loading" | "error"> {
  const [remaining, setRemaining] = useState<
    Record<string, number | "loading" | "error">
  >({});
  // acctsKey는 accounts의 안정 문자열 키(배열 참조 대신 값으로 비교).
  const acctsKey = accounts.join(",");
  useEffect(() => {
    if (!enabled || accounts.length === 0) return;
    let cancelled = false;
    const targets = accounts;
    // 각 계정을 "확인 중"으로 먼저 표시(데스크톱 publish-modal과 동일 — 계정별 setState).
    targets.forEach((id) => setRemaining((m) => ({ ...m, [id]: "loading" })));
    const applyMap = (map: Record<string, number | null>) => {
      if (cancelled) return;
      setRemaining((m) => {
        const next = { ...m };
        targets.forEach((id) => {
          if (id in map) next[id] = map[id] ?? "error";
        });
        return next;
      });
    };
    // 원격 조회 요청 → 이후 회신을 폴링. 오프라인/실패는 확인 실패로 표시.
    void api.devices.queryNicknameRemaining(deviceId, targets).catch(() => {
      if (!cancelled)
        setRemaining((m) => {
          const next = { ...m };
          targets.forEach((id) => {
            next[id] = "error";
          });
          return next;
        });
    });
    const poll = () => {
      api.devices
        .nicknameRemaining(deviceId)
        .then(applyMap)
        .catch(() => {
          /* 미회신/오프라인 → 다음 폴링까지 "확인 중" 유지 */
        });
    };
    poll();
    const timer = window.setInterval(poll, 1500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, acctsKey, deviceId]);
  return remaining;
}

// 종토 상세 구성(카테고리/시장/종목수/계정 + 4버튼). 외곽 Paper·기기헤더는 DeviceBlock이 제공.
export function ForumConfig({
  device,
  mode,
  cfg,
  onPatch,
  postId,
  postTitle,
  postCommentCount,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  mode: PostKind; // "post"(글) | "both"(글+댓글) — 댓글은 ForumCommentConfig가 처리.
  cfg: ForumCfg;
  onPatch: (patch: Partial<ForumCfg>) => void;
  postId: string | null;
  postTitle: string | null;
  postCommentCount: number;
  accounts: string[];
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
      .list({
        category: cfg.category,
        exchange: "krx",
        market: effectiveMarket,
      })
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

  // 계정 = 상위(PublishCommand)가 넘긴 이 하위의 성공(Active) 계정(실데이터/더미 폴백).

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

  // 닉네임 랜덤(15-기타명령 §3): 글+댓글(both)에서 그 글의 작성 댓글 수 ≥ 2일 때만 노출한다.
  // 순수 "글"(post) 모드는 댓글이 없으므로 절대 보이지 않는다(데스크톱 publish-modal 게이트 미러:
  // (mode==="comment" || mode==="both") — 종토 댓글은 ForumCommentConfig가, 여기선 both만 담당).
  const showNicknameRandom = mode === "both" && postCommentCount >= 2;
  const [nicknameRandom, setNicknameRandom] = useState(false);
  // 게이트(both·≥2)가 열렸고 체크됐을 때만 payload에 싣는다. 숨겨지면 항상 false(post 모드 무해).
  const commentNicknameRandom = showNicknameRandom && nicknameRandom;
  // 켜면 선택 계정(cfg.accounts)의 잔여 변경 횟수를 §6-2 실시간 원격 조회로 채운다(공유 훅).
  const remaining = useNicknameRemaining(
    device.id,
    cfg.accounts,
    commentNicknameRandom,
  );

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

  // 게시 후 내용변경(15-기타명령 §4) — 체크됐을 때만 payload에 싣는다. 엔진이 게시 후 지연 뒤 edit.
  const contentChangePayload = cfg.contentChange.enabled
    ? {
        contentChange: {
          title: cfg.contentChange.title,
          body: cfg.contentChange.body,
          delaySec: cfg.contentChange.delaySec,
        },
      }
    : {};

  const runNow = (split: boolean) => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split,
          mode, // 글=post / 글+댓글=both(글 게시 후 그 글에 저장된 댓글까지).
          ...contentChangePayload,
          commentNicknameRandom,
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
    const at = toEpochMs(sched.date, sched.time);
    const detail = detailFor(split);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(null);
    void (async () => {
      try {
        // 서버가 예약을 보관하고 스케줄러가 시각되면 발송한다(4단계). 계정×종목·글 전체를 함께 보낸다.
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split,
          mode,
          ...contentChangePayload,
          commentNicknameRandom,
          assignments: buildAssignments(split),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 예약 등록`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          // 서버 오프라인 → 로컬 예약 목록으로 폴백(미리보기 무손상).
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "종목토론방",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "예약 등록 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
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

      {/* 게시 후 내용변경(15-기타명령 §4) — 종목토론방(글쓰기) 전용. 종목 수와 계정 사이 위치.
          체크하면 새 제목/내용/지연(초)을 입력한다. 게시 성공 후 지연 뒤 그 글을 새 내용으로 edit한다
          (엔진 spawn_forum_content_edit이 처리 — 여기선 payload에 값만 싣는다). 데스크톱 내용변경 블록 이식. */}
      <Checkbox
        mb={cfg.contentChange.enabled ? 8 : "sm"}
        label="게시 후 내용변경"
        checked={cfg.contentChange.enabled}
        onChange={(e) => {
          const enabled = e.currentTarget.checked;
          onPatch({ contentChange: { ...cfg.contentChange, enabled } });
        }}
      />
      {cfg.contentChange.enabled && (
        <Stack gap={10} mb="sm">
          <TextInput
            size="xs"
            label="제목"
            placeholder="변경할 새 제목"
            value={cfg.contentChange.title}
            onChange={(e) => {
              const title = e.currentTarget.value;
              onPatch({ contentChange: { ...cfg.contentChange, title } });
            }}
          />
          <Textarea
            size="xs"
            label="내용"
            placeholder="변경할 새 내용"
            rows={4}
            value={cfg.contentChange.body}
            onChange={(e) => {
              const body = e.currentTarget.value;
              onPatch({ contentChange: { ...cfg.contentChange, body } });
            }}
          />
          <Group gap={8} align="flex-end" wrap="nowrap">
            <NumberInput
              size="xs"
              label="변경 지연"
              min={0}
              w={120}
              value={cfg.contentChange.delaySec}
              onChange={(v) => {
                const delaySec = typeof v === "number" ? v : 0;
                onPatch({ contentChange: { ...cfg.contentChange, delaySec } });
              }}
            />
            <Text fz={13} fw={600} mb={8}>
              초
            </Text>
          </Group>
        </Stack>
      )}

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

      {/* 닉네임 랜덤(15-기타명령 §3) — 계정 선택 밑·[지금 게시] 위. 글+댓글(both)에서 그 글의 작성
          댓글 수 ≥ 2일 때만 노출(순수 글 모드는 댓글이 없어 숨김). 체크하면 계정별 변경 가능횟수를
          §6-2 실시간 원격 조회로 채워 보여준다. 켜지면 payload에 commentNicknameRandom=true가 실린다. */}
      {showNicknameRandom && (
        <>
          <Checkbox
            mt="sm"
            label="닉네임 랜덤 (각 댓글마다 닉네임을 랜덤으로 바꿔 게시)"
            checked={nicknameRandom}
            onChange={(e) => setNicknameRandom(e.currentTarget.checked)}
          />
          {nicknameRandom && cfg.accounts.length > 0 && (
            <Stack gap={2} mt={8}>
              {cfg.accounts.map((id) => {
                const r = remaining[id];
                const label =
                  r === undefined || r === "loading"
                    ? "확인 중…"
                    : r === "error"
                      ? "확인 실패"
                      : `변경 가능횟수 ${r}회`;
                return (
                  <Text key={id} fz={11.5} c="dimmed">
                    {maskId(id)} : {label}
                  </Text>
                );
              })}
            </Stack>
          )}
        </>
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

// 종토 댓글 상세 구성(사용자 확정 2026-07-06: 종토 댓글=특정 게시글). 링크 여러 개 입력 + 링크추가
// → 각 URL의 글에 저장된 댓글을 단다(엔진 기존 comment_url 경로 재사용). 계정 선택 후 지금/예약
// 게시. 나눠서는 없다(URL 댓글은 계정마다 같은 URL에 단다). 데스크톱 writer-modal 링크 UX 이식.
export function ForumCommentConfig({
  device,
  postId,
  postTitle,
  postCommentCount,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  postId: string | null;
  postTitle: string | null;
  postCommentCount: number;
  accounts: string[];
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [urls, setUrls] = useState<string[]>(["", "", ""]);
  const [accts, setAccts] = useState<string[]>([]);
  const [armed, setArmed] = useState<boolean>(false);
  const [sched, setSched] = useState(() => nowParts());
  // 닉네임 랜덤(15-기타명령 §3): 선택 글의 작성 댓글 수 ≥ 2일 때만 체크박스 노출(데스크톱 게이트 +
  // Admin "댓글 수 ≥ 2"). 댓글 1개면 계정을 여러 개 골라도 숨긴다.
  const showNicknameRandom = postCommentCount >= 2;
  const [nicknameRandom, setNicknameRandom] = useState(false);

  const cleanUrls = urls.map((u) => u.trim()).filter((u) => u.length > 0);
  const valid = postTitle != null && cleanUrls.length > 0 && accts.length > 0;
  const detail = `특정 게시글 ${cleanUrls.length}개 · 계정 ${accts.length}`;
  // 닉네임 랜덤 flag는 게이트(≥2)가 열렸고 체크됐을 때만 payload에 싣는다. 숨겨지면 항상 false.
  const commentNicknameRandom = showNicknameRandom && nicknameRandom;
  // 계정별 변경 가능 잔여 횟수(§6-2 실시간) — 공유 훅이 원격 조회·폴링을 담당한다.
  const remaining = useNicknameRemaining(device.id, accts, commentNicknameRandom);

  // 댓글은 계정마다 같은 URL들에 단다(나눠서 없음). assignment는 계정만(종목 없음).
  const buildAssignments = () =>
    accts.map((loginId) => ({ loginId, stocks: [] }));

  const setUrl = (i: number, v: string) =>
    setUrls((prev) => prev.map((u, idx) => (idx === i ? v : u)));
  const removeUrl = (i: number) =>
    setUrls((prev) =>
      prev.length <= 1 ? [""] : prev.filter((_, idx) => idx !== i),
    );

  const runNow = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split: false,
          mode: "comment",
          commentUrls: cleanUrls,
          commentNicknameRandom,
          assignments: buildAssignments(),
        });
        notifications.show({
          title: `${device.name} · 댓글 명령 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 댓글 게시(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "댓글 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  // 나눠서 게시(#403): 데스크톱과 달리 여기선 전체 계정을 한 명령(단일 plan)으로 보내고
  // forumCommentDistribute=true를 실어 하위/엔진이 링크마다 댓글을 계정에 1:1 분배하게 한다.
  // ⚠️ 활성 조건은 URL·계정 존재(valid)만 본다 — Admin 인벤토리(InvPostDto)엔 글 본문 excerpt만
  // 있고 저장된 댓글 배열이 없어 "댓글 수 == 계정 수"(canForumCommentDistribute)를 이 화면에서
  // 판정할 수 없다. 겹침 없는 1:1은 실제 댓글 수가 계정 수와 같을 때만 보장되며(그 규칙은 헬퍼로
  // 명세), 그렇지 않으면 엔진이 링크마다 댓글 풀을 셔플해 계정에 1개씩 배정한다(일부 미소진 가능).
  const runDistribute = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split: false,
          mode: "comment",
          commentUrls: cleanUrls,
          forumCommentDistribute: true,
          commentNicknameRandom,
          assignments: buildAssignments(),
        });
        notifications.show({
          title: `${device.name} · 댓글 나눠서 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail} · 계정에 1개씩 분배`,
          color: "teal",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 나눠서 게시(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "나눠서 게시 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const at = toEpochMs(sched.date, sched.time);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(false);
    void (async () => {
      try {
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "종목토론방",
          split: false,
          mode: "comment",
          commentUrls: cleanUrls,
          commentNicknameRandom,
          assignments: buildAssignments(),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 댓글 예약`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "종목토론방(댓글)",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 댓글 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "댓글 예약 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 특정 게시글 URL 입력 + 링크 추가(데스크톱 writer-modal와 동일 UX) */}
      <Text size="xs" c="dimmed" mb={4}>
        특정 게시글 링크 (넣은 링크의 글마다 저장된 댓글을 답니다)
      </Text>
      <Stack gap={6} mb="sm">
        {urls.map((u, i) => (
          <Group key={i} gap={6} wrap="nowrap">
            <TextInput
              size="xs"
              style={{ flex: 1 }}
              placeholder={`종목토론방 글 URL ${i + 1}`}
              value={u}
              onChange={(e) => setUrl(i, e.currentTarget.value)}
              styles={{ input: { fontFamily: "monospace" } }}
            />
            <ActionIcon
              size="md"
              variant="subtle"
              color="gray"
              title="링크 삭제"
              onClick={() => removeUrl(i)}
            >
              <Icon.x size={15} />
            </ActionIcon>
          </Group>
        ))}
        <Button
          size="xs"
          variant="light"
          leftSection={<Icon.plus size={13} />}
          onClick={() => setUrls((prev) => [...prev, ""])}
          style={{ alignSelf: "flex-start" }}
        >
          링크 추가
        </Button>
      </Stack>

      {/* 계정 선택 — 이 하위의 성공 계정만 */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 로그인 성공 계정만 · {accts.length}명 선택)
      </Text>
      <Group gap={6}>
        {accounts.map((a) => {
          const on = accts.includes(a);
          return (
            <Button
              key={a}
              size="xs"
              variant={on ? "filled" : "default"}
              color={on ? "blue" : "gray"}
              onClick={() =>
                setAccts((prev) =>
                  on ? prev.filter((x) => x !== a) : [...prev, a],
                )
              }
            >
              {maskId(a)}
            </Button>
          );
        })}
      </Group>

      {/* 닉네임 랜덤(15-기타명령 §3) — 계정 선택 밑·[지금 게시] 위. 선택 글의 작성 댓글 수 ≥ 2일
          때만 노출(댓글 1개면 계정 여러 개여도 숨김). 체크하면 계정별 변경 가능횟수를 §6-2 실시간
          원격 조회로 채워 보여준다. 켜지면 payload에 commentNicknameRandom=true가 실린다. */}
      {showNicknameRandom && (
        <>
          <Checkbox
            mt="sm"
            label="닉네임 랜덤 (각 댓글마다 닉네임을 랜덤으로 바꿔 게시)"
            checked={nicknameRandom}
            onChange={(e) => setNicknameRandom(e.currentTarget.checked)}
          />
          {nicknameRandom && accts.length > 0 && (
            <Stack gap={2} mt={8}>
              {accts.map((id) => {
                const r = remaining[id];
                const label =
                  r === undefined || r === "loading"
                    ? "확인 중…"
                    : r === "error"
                      ? "확인 실패"
                      : `변경 가능횟수 ${r}회`;
                return (
                  <Text key={id} fz={11.5} c="dimmed">
                    {maskId(id)} : {label}
                  </Text>
                );
              })}
            </Stack>
          )}
        </>
      )}

      {/* 지금/예약 게시 + 나눠서 게시(#403) */}
      <Stack gap={8} mt="md">
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!valid}
            leftSection={<Icon.bolt size={15} />}
            onClick={runNow}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed ? "filled" : "light"}
            color="grape"
            disabled={!valid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            예약 게시
          </Button>
        </Group>
        {/* 나눠서 게시(#403): 넣은 링크마다 저장된 댓글을 계정에 1개씩 무작위 배정(겹침 없음).
            ⚠️ 데스크톱은 "댓글 수 == 계정 수"일 때만 활성하지만, Admin 인벤토리엔 저장된 댓글
            배열이 없어 여기선 그 비교가 불가 → URL·계정 존재(valid)만으로 노출한다. */}
        <Button
          size="sm"
          fw={700}
          variant="light"
          color="teal"
          disabled={!valid}
          leftSection={<Icon.send size={15} />}
          onClick={runDistribute}
        >
          나눠서 게시 (댓글을 계정들에 1개씩)
        </Button>
        {armed && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              댓글 예약 — 게시 시각 선택
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
                onClick={() => setArmed(false)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}
      </Stack>
    </Box>
  );
}

// ── 네이버 카페 게시판 대상(게시판 링크 파싱 결과) ──
// 데스크톱 publish-modal 카페 카드와 동일: 링크를 붙여넣으면 cafeId+menuId(게시판) 또는
// cafeId+articleId(특정 글)를 파싱해 목록에 쌓고, 고른 게시판이 동그라미 배지로 뜬다.
// board_type은 게시 시점 백엔드가 menu_id로 해석하므로 여기선 파싱만 한다(네트워크 불필요).
interface ResolvedCafeBoard {
  cafeId: number;
  menuId: number; // 게시판(글쓰기 대상). 0=글 링크만.
  articleId: number; // 특정 글(url 댓글 대상). 0=게시판 링크.
  link: string;
}
function cafeBoardKey(b: ResolvedCafeBoard): string {
  return `${b.cafeId}-${b.menuId}-${b.articleId}`;
}
function cafeBoardLabel(b: ResolvedCafeBoard): string {
  return b.articleId > 0
    ? `카페 ${b.cafeId} · 글 ${b.articleId}`
    : `카페 ${b.cafeId} · 게시판 ${b.menuId}`;
}

// 네이버 카페 상세 구성(글/댓글/글+댓글 모두 게시판 링크). 카페는 로그인 성공/실패 무관 계정을
// 전부 쓰고(상위 accountsFor가 이미 카페 계정 전부를 넘김), 게시 시점에 하위가 재로그인해 올린다.
// 댓글 대상/개수는 글(LibraryPost)에 동결돼 있어 여기선 고르지 않는다(엔진이 글에서 읽음).
function CafeConfig({
  device,
  kind,
  postId,
  postTitle,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  kind: PostKind;
  postId: string | null;
  postTitle: string | null;
  accounts: string[];
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [link, setLink] = useState("");
  const [resolved, setResolved] = useState<ResolvedCafeBoard[]>([]);
  const [selected, setSelected] = useState<string[]>([]); // cafeBoardKey 목록
  const [accts, setAccts] = useState<string[]>([]);
  const [armed, setArmed] = useState<boolean>(false);
  const [sched, setSched] = useState(() => nowParts());
  // 댓글 대상 모드/개수(댓글 모드에서만 의미). url=특정 글, latest=최신 N, popular=인기 N.
  const [commentMode, setCommentMode] = useState<CommentTargetMode>("latest");
  const [count, setCount] = useState<number | "">(20);

  const isComment = kind === "comment";
  const isList = isComment && commentMode !== "url"; // 최신/인기=개수 N 필요.
  const commentFields = () =>
    commentTargetPayload(
      kind,
      commentMode,
      typeof count === "number" ? count : 1,
    );
  const selectedBoards = resolved.filter((b) =>
    selected.includes(cafeBoardKey(b)),
  );
  const valid =
    postTitle != null && selectedBoards.length > 0 && accts.length > 0;
  const detail = isComment
    ? `${COMMENT_MODE_OPTS.find((m) => m.value === commentMode)?.label} · 게시판 ${selectedBoards.length}개 · 계정 ${accts.length}`
    : `게시판 ${selectedBoards.length}개 · 계정 ${accts.length}`;

  // 게시판 링크 추가: cafeId+menuId(게시판) 또는 cafeId+articleId(특정 글)를 파싱해 목록에 쌓는다.
  const addLink = () => {
    const raw = link.trim();
    if (!raw) return;
    const board = parseCafeBoardLink(raw);
    const article = parseCafeArticleUrl(raw);
    if (!board && !article) {
      notifications.show({
        title: "링크 인식 실패",
        message: "카페 게시판/글 링크를 확인하세요.",
        color: "red",
      });
      return;
    }
    const b: ResolvedCafeBoard = {
      cafeId: board?.cafeId ?? article?.cafeId ?? 0,
      menuId: board?.menuId ?? 0,
      articleId: article?.articleId ?? 0,
      link: raw,
    };
    const key = cafeBoardKey(b);
    setResolved((prev) =>
      prev.some((x) => cafeBoardKey(x) === key) ? prev : [...prev, b],
    );
    setLink("");
  };
  const onSelect = (key: string) =>
    setSelected((prev) => (prev.includes(key) ? prev : [...prev, key]));
  const onRemove = (key: string) =>
    setSelected((prev) => prev.filter((k) => k !== key));

  const cafeBoardsPayload = () =>
    selectedBoards.map((b) => ({
      cafeId: b.cafeId,
      menuId: b.menuId,
      articleId: b.articleId,
      link: b.link,
    }));
  const assignments = () => accts.map((loginId) => ({ loginId, stocks: [] }));

  const runNow = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 카페",
          split: false,
          mode: kind,
          target: "naver",
          cafeBoards: cafeBoardsPayload(),
          ...commentFields(),
          assignments: assignments(),
        });
        notifications.show({
          title: `${device.name} · 카페 게시 명령 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 카페 게시(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "카페 게시 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const at = toEpochMs(sched.date, sched.time);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(false);
    void (async () => {
      try {
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 카페",
          split: false,
          mode: kind,
          target: "naver",
          cafeBoards: cafeBoardsPayload(),
          ...commentFields(),
          assignments: assignments(),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 카페 예약`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "네이버 카페",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 카페 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "카페 예약 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 댓글 대상 모드(댓글 모드에서만) — 특정 글 / 최신글 / 인기글. 엔진이 실제로 지원하는
          카페 댓글 대상(collect_comment_targets: url·latest·popular)을 그대로 노출한다. */}
      {isComment && (
        <>
          <Text size="xs" c="dimmed" mb={4}>
            댓글 대상
          </Text>
          <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
            {COMMENT_MODE_OPTS.map((m) => (
              <Button
                key={m.value}
                size="xs"
                variant={commentMode === m.value ? "filled" : "default"}
                color={commentMode === m.value ? "teal" : "gray"}
                onClick={() => setCommentMode(m.value)}
                aria-label={`카페 댓글 대상 ${m.label}`}
              >
                {m.label}
              </Button>
            ))}
            {isList && (
              <NumberInput
                size="xs"
                label="개수"
                w={90}
                min={1}
                max={50}
                value={count}
                onChange={(v) => setCount(typeof v === "number" ? v : "")}
                aria-label="카페 최신/인기 댓글 개수"
              />
            )}
          </Group>
        </>
      )}
      {/* 게시판/글 링크 입력 + 추가(데스크톱 publish-modal 카페 카드와 동일 UX) */}
      <Text size="xs" c="dimmed" mb={4}>
        {isComment && commentMode === "url"
          ? "글 링크 (넣은 특정 글에 저장된 댓글을 답니다)"
          : isList
            ? "게시판/카페 링크 (그 카페의 최신/인기 상위 N개 글에 댓글을 답니다)"
            : `게시판 링크 (넣은 게시판/글에 ${kind === "comment" ? "댓글을" : "글을"} 올립니다)`}
      </Text>
      <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
        <TextInput
          size="xs"
          style={{ flex: 1 }}
          placeholder={
            isComment && commentMode === "url"
              ? "https://cafe.naver.com/f-e/cafes/31732304/articles/12345"
              : "https://cafe.naver.com/f-e/cafes/31732304/menus/1"
          }
          value={link}
          onChange={(e) => setLink(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addLink();
          }}
          styles={{ input: { fontFamily: "monospace" } }}
          aria-label="카페 게시판 링크"
        />
        <Button
          size="xs"
          variant="light"
          color="teal"
          disabled={!link.trim()}
          onClick={addLink}
        >
          추가
        </Button>
      </Group>
      <Select
        size="xs"
        mb="sm"
        placeholder={
          resolved.length > 0
            ? "게시할 게시판 선택"
            : "게시판 링크를 추가하면 여기 표시됩니다"
        }
        data={resolved.map((b) => ({
          value: cafeBoardKey(b),
          label: cafeBoardLabel(b),
        }))}
        value={null}
        disabled={resolved.length === 0}
        comboboxProps={{ withinPortal: true }}
        onChange={(k) => {
          if (k) onSelect(k);
        }}
        aria-label="카페 게시판 선택"
      />
      {selectedBoards.length > 0 ? (
        <Group gap={6} mb="sm">
          {selectedBoards.map((b) => {
            const key = cafeBoardKey(b);
            return (
              <Badge
                key={key}
                color="teal"
                variant="light"
                radius="sm"
                rightSection={
                  <ActionIcon
                    size={14}
                    variant="transparent"
                    color="teal"
                    aria-label={`${cafeBoardLabel(b)} 제거`}
                    onClick={() => onRemove(key)}
                  >
                    <Icon.x size={10} />
                  </ActionIcon>
                }
              >
                {cafeBoardLabel(b)}
              </Badge>
            );
          })}
        </Group>
      ) : (
        <Text fz={12} c="orange.7" mb="sm">
          게시할 게시판을 선택하세요.
        </Text>
      )}

      {/* 계정 선택 — 카페는 로그인 성공/실패 무관 카페 계정 전부(게시 순간 재로그인). */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 카페 계정 · 로그인 성공/실패 무관 · {accts.length}명
        선택)
      </Text>
      <Group gap={6}>
        {accounts.length === 0 ? (
          <Text size="xs" c="dimmed">
            이 하위에 카페 계정이 없습니다(계정 분배에서 플랫폼=네이버 카페로
            분배하세요).
          </Text>
        ) : (
          accounts.map((a) => {
            const on = accts.includes(a);
            return (
              <Button
                key={a}
                size="xs"
                variant={on ? "filled" : "default"}
                color={on ? "teal" : "gray"}
                onClick={() =>
                  setAccts((prev) =>
                    on ? prev.filter((x) => x !== a) : [...prev, a],
                  )
                }
              >
                {maskId(a)}
              </Button>
            );
          })
        )}
      </Group>

      {/* 지금/예약 게시(로그인 상태 안 봄 — 카페는 게시 때 재로그인) */}
      <Stack gap={8} mt="md">
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!valid}
            leftSection={<Icon.bolt size={15} />}
            onClick={runNow}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed ? "filled" : "light"}
            color="grape"
            disabled={!valid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            예약 게시
          </Button>
        </Group>
        {armed && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              카페 예약 — 게시 시각 선택
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
                onClick={() => setArmed(false)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}
      </Stack>
    </Box>
  );
}

// ── 네이버 블로그 댓글 대상(글/블로그 링크 파싱 결과) ──
// 데스크톱 publish-modal 블로그 카드와 동일: 블로그는 댓글 전용(#271/#279). 두 모드가 있다:
//   - 특정 게시글: 글 링크 → parseBlogPostLink → {blogId, logNo}. 그 글 1개에 댓글.
//   - 최신글 / 인기글: 블로그 링크 → parseBlogLink → {blogId, categoryNo?} + 개수 N. 최신 N개에 댓글.
// ⚠️ 최신글/인기글 버튼은 둘 다 **동일한 최신 N개 대상**을 만든다(백엔드에 인기 정렬이 없음).
type BlogMode = "specific" | "latest" | "popular";
const BLOG_MODES: { key: BlogMode; label: string }[] = [
  { key: "specific", label: "특정 게시글" },
  { key: "latest", label: "최신글" },
  { key: "popular", label: "인기글" },
];

/** 블로그 댓글 대상 1건 — 특정 글(logNo) 또는 최신 N개(count[, categoryNo]). */
interface BlogItem {
  blogId: string;
  logNo?: string; // 특정 글(있으면). 없으면 최신 N개.
  categoryNo?: number; // 최신 N개 글 목록 카테고리(있으면).
  count?: number; // 최신 N개(logNo 없을 때).
  link: string;
}
function blogItemKey(it: BlogItem): string {
  return it.logNo !== undefined
    ? `${it.blogId}/post/${it.logNo}`
    : `${it.blogId}/latest/${it.categoryNo ?? ""}`;
}
function blogItemLabel(it: BlogItem): string {
  if (it.logNo !== undefined) return `${it.blogId} · 글 ${it.logNo}`;
  const cat = it.categoryNo !== undefined ? ` · 카테고리 ${it.categoryNo}` : "";
  return `${it.blogId} · 최신 ${it.count ?? 1}개${cat}`;
}

// 네이버 블로그 상세 구성(댓글 전용). 특정 게시글=글 링크에 댓글, 최신글/인기글=블로그 링크의
// 최신 N개에 댓글. 블로그는 유효 쿠키가 필요해 로그인 성공(active) 블로그 계정만 쓴다(상위
// accountsFor가 이미 걸러 넘김). 댓글 본문은 글(LibraryPost)에 저장된 댓글을 엔진이 읽어 단다.
function BlogConfig({
  device,
  postId,
  postTitle,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  postId: string | null;
  postTitle: string | null;
  accounts: string[];
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [blogMode, setBlogMode] = useState<BlogMode>("specific");
  const [link, setLink] = useState("");
  const [count, setCount] = useState<number | "">(1);
  const [items, setItems] = useState<BlogItem[]>([]);
  const [accts, setAccts] = useState<string[]>([]);
  const [armed, setArmed] = useState<boolean>(false);
  const [sched, setSched] = useState(() => nowParts());

  const isList = blogMode !== "specific"; // 최신글/인기글 = 최신 N개(동일 동작).
  const valid = postTitle != null && items.length > 0 && accts.length > 0;
  const detail = `${isList ? "블로그" : "특정 글"} ${items.length}개 · 계정 ${accts.length}`;

  // 링크 추가: 특정 게시글=글 링크(parseBlogPostLink), 최신글/인기글=블로그 링크(parseBlogLink).
  const addLink = () => {
    const raw = link.trim();
    if (!raw) return;
    let it: BlogItem | null = null;
    if (blogMode === "specific") {
      const p = parseBlogPostLink(raw);
      if (p) it = { blogId: p.blogId, logNo: p.logNo, link: raw };
    } else {
      const p = parseBlogLink(raw);
      if (p) {
        const n =
          typeof count === "number" && count > 0 ? Math.floor(count) : 1;
        it =
          p.categoryNo !== undefined
            ? {
                blogId: p.blogId,
                categoryNo: p.categoryNo,
                count: n,
                link: raw,
              }
            : { blogId: p.blogId, count: n, link: raw };
      }
    }
    if (!it) {
      notifications.show({
        title: "링크 인식 실패",
        message:
          blogMode === "specific"
            ? "블로그 글 링크를 확인하세요."
            : "블로그 링크를 확인하세요.",
        color: "red",
      });
      return;
    }
    const added = it;
    const key = blogItemKey(added);
    setItems((prev) =>
      prev.some((x) => blogItemKey(x) === key) ? prev : [...prev, added],
    );
    setLink("");
  };
  const onRemove = (key: string) =>
    setItems((prev) => prev.filter((x) => blogItemKey(x) !== key));

  // 와이어 계약: 특정 글={blogId,logNo,link}, 최신 N개={blogId,categoryNo?,count,link}.
  const blogLinksPayload = () =>
    items.map((it) =>
      it.logNo !== undefined
        ? { blogId: it.blogId, logNo: it.logNo, link: it.link }
        : it.categoryNo !== undefined
          ? {
              blogId: it.blogId,
              categoryNo: it.categoryNo,
              count: it.count ?? 1,
              link: it.link,
            }
          : { blogId: it.blogId, count: it.count ?? 1, link: it.link },
    );
  const assignments = () => accts.map((loginId) => ({ loginId, stocks: [] }));

  const runNow = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 블로그",
          split: false,
          mode: "comment",
          target: "blog",
          blogLinks: blogLinksPayload(),
          assignments: assignments(),
        });
        notifications.show({
          title: `${device.name} · 블로그 댓글 명령 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 블로그 댓글(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "블로그 댓글 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const at = toEpochMs(sched.date, sched.time);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(false);
    void (async () => {
      try {
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 블로그",
          split: false,
          mode: "comment",
          target: "blog",
          blogLinks: blogLinksPayload(),
          assignments: assignments(),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 블로그 예약`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "네이버 블로그",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 블로그 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "블로그 예약 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 댓글 대상 모드 — 특정 게시글 / 최신글 / 인기글(최신글·인기글은 동일 동작). */}
      <Text size="xs" c="dimmed" mb={4}>
        댓글 대상
      </Text>
      <Group gap="xs" mb="sm">
        {BLOG_MODES.map((m) => (
          <Button
            key={m.key}
            size="xs"
            variant={blogMode === m.key ? "filled" : "default"}
            color={blogMode === m.key ? "blue" : "gray"}
            onClick={() => {
              setBlogMode(m.key);
              setLink("");
            }}
          >
            {m.label}
          </Button>
        ))}
      </Group>

      {/* 링크 입력 + 추가. 특정 게시글=글 링크, 최신글/인기글=블로그 링크 + 개수 N. */}
      <Text size="xs" c="dimmed" mb={4}>
        {isList
          ? "블로그 링크 (블로그의 최신 N개 글에 댓글을 답니다)"
          : "글 링크 (넣은 글에 저장된 댓글을 답니다)"}
      </Text>
      <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
        <TextInput
          size="xs"
          style={{ flex: 1 }}
          placeholder={
            isList
              ? "https://blog.naver.com/press02"
              : "https://blog.naver.com/press02/224311392458"
          }
          value={link}
          onChange={(e) => setLink(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addLink();
          }}
          styles={{ input: { fontFamily: "monospace" } }}
          aria-label={isList ? "블로그 링크" : "블로그 글 링크"}
        />
        {isList && (
          <NumberInput
            size="xs"
            label="개수"
            w={90}
            min={1}
            max={50}
            value={count}
            onChange={(v) => setCount(typeof v === "number" ? v : "")}
            aria-label="최신 글 개수"
          />
        )}
        <Button
          size="xs"
          variant="light"
          color="blue"
          disabled={!link.trim()}
          onClick={addLink}
        >
          추가
        </Button>
      </Group>

      {items.length > 0 ? (
        <Group gap={6} mb="sm">
          {items.map((it) => {
            const key = blogItemKey(it);
            return (
              <Badge
                key={key}
                color="blue"
                variant="light"
                radius="sm"
                rightSection={
                  <ActionIcon
                    size={14}
                    variant="transparent"
                    color="blue"
                    aria-label={`${blogItemLabel(it)} 제거`}
                    onClick={() => onRemove(key)}
                  >
                    <Icon.x size={10} />
                  </ActionIcon>
                }
              >
                {blogItemLabel(it)}
              </Badge>
            );
          })}
        </Group>
      ) : (
        <Text fz={12} c="orange.7" mb="sm">
          댓글을 달 {isList ? "블로그를" : "블로그 글을"} 추가하세요.
        </Text>
      )}

      {/* 계정 선택 — 블로그는 유효 쿠키 필요(로그인 성공 계정만). */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 블로그 로그인 성공 계정만 · {accts.length}명 선택)
      </Text>
      <Group gap={6}>
        {accounts.length === 0 ? (
          <Text size="xs" c="dimmed">
            이 하위에 로그인 성공한 블로그 계정이 없습니다(계정 분배에서
            플랫폼=네이버 블로그로 분배·로그인하세요).
          </Text>
        ) : (
          accounts.map((a) => {
            const on = accts.includes(a);
            return (
              <Button
                key={a}
                size="xs"
                variant={on ? "filled" : "default"}
                color={on ? "blue" : "gray"}
                onClick={() =>
                  setAccts((prev) =>
                    on ? prev.filter((x) => x !== a) : [...prev, a],
                  )
                }
              >
                {maskId(a)}
              </Button>
            );
          })
        )}
      </Group>

      {/* 지금/예약 게시(블로그 댓글은 나눠서 없음 — 계정마다 같은 대상에 단다). */}
      <Stack gap={8} mt="md">
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!valid}
            leftSection={<Icon.bolt size={15} />}
            onClick={runNow}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed ? "filled" : "light"}
            color="grape"
            disabled={!valid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            예약 게시
          </Button>
        </Group>
        {armed && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              블로그 예약 — 게시 시각 선택
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
                onClick={() => setArmed(false)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}
      </Stack>
    </Box>
  );
}

// ── 네이버 클립 댓글 대상(#클립) ──
// 데스크톱 publish-modal 클립 카드(#클립)와 동일: 클립은 **댓글 전용·항상 "최신 N개"**. 창작자
// 링크(@handle)를 추가하면 그 창작자의 최신 미디어 상위 N개에 댓글을 단다. ?tab=video면 영상만.
// 특정 영상 1건·인기 정렬은 엔진에 없음(블로그처럼 최신만). 개수 N은 운영자가 지정(블로그 UI 재사용).
interface ClipItem {
  handle: string;
  mediaType?: "all" | "video";
  count: number;
  link: string;
}
function clipItemKey(it: ClipItem): string {
  return `${it.handle}/${it.mediaType ?? "all"}`;
}
function clipItemLabel(it: ClipItem): string {
  const tab = it.mediaType === "video" ? " · 영상만" : "";
  return `@${it.handle}${tab} · 최신 ${it.count}개`;
}

// 네이버 클립 상세 구성(댓글 전용·최신 N개). 창작자 링크에 최신 N개 미디어에 댓글. 클립은 유효
// 네이버 쿠키가 필요해 로그인 성공(active) 클립 계정만 쓴다(상위 accountsFor가 이미 걸러 넘김).
// 댓글 본문은 글(LibraryPost)에 저장된 댓글을 엔진이 읽어 단다.
function ClipConfig({
  device,
  postId,
  postTitle,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  postId: string | null;
  postTitle: string | null;
  accounts: string[];
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [link, setLink] = useState("");
  const [count, setCount] = useState<number | "">(1);
  const [items, setItems] = useState<ClipItem[]>([]);
  const [accts, setAccts] = useState<string[]>([]);
  const [armed, setArmed] = useState<boolean>(false);
  const [sched, setSched] = useState(() => nowParts());

  const valid = postTitle != null && items.length > 0 && accts.length > 0;
  const detail = `창작자 ${items.length}명 · 계정 ${accts.length}`;

  // 링크 추가: parseClipLink로 {handle, mediaType?} 파싱 + 개수 N. ?tab=video면 영상만.
  const addLink = () => {
    const raw = link.trim();
    if (!raw) return;
    const p = parseClipLink(raw);
    if (!p) {
      notifications.show({
        title: "링크 인식 실패",
        message: "클립 창작자 링크(@아이디)를 확인하세요.",
        color: "red",
      });
      return;
    }
    const n = typeof count === "number" && count > 0 ? Math.floor(count) : 1;
    const it: ClipItem =
      p.mediaType !== undefined
        ? { handle: p.handle, mediaType: p.mediaType, count: n, link: raw }
        : { handle: p.handle, count: n, link: raw };
    const key = clipItemKey(it);
    setItems((prev) =>
      prev.some((x) => clipItemKey(x) === key) ? prev : [...prev, it],
    );
    setLink("");
  };
  const onRemove = (key: string) =>
    setItems((prev) => prev.filter((x) => clipItemKey(x) !== key));

  // 와이어 계약: {handle, mediaType?, count, link}. mediaType 없으면 전체.
  const clipLinksPayload = () =>
    items.map((it) =>
      it.mediaType !== undefined
        ? {
            handle: it.handle,
            mediaType: it.mediaType,
            count: it.count,
            link: it.link,
          }
        : { handle: it.handle, count: it.count, link: it.link },
    );
  const assignments = () => accts.map((loginId) => ({ loginId, stocks: [] }));

  const runNow = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 클립",
          split: false,
          mode: "comment",
          target: "clip",
          clipLinks: clipLinksPayload(),
          assignments: assignments(),
        });
        notifications.show({
          title: `${device.name} · 클립 댓글 명령 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 클립 댓글(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "클립 댓글 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const at = toEpochMs(sched.date, sched.time);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(false);
    void (async () => {
      try {
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 클립",
          split: false,
          mode: "comment",
          target: "clip",
          clipLinks: clipLinksPayload(),
          assignments: assignments(),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 클립 예약`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "네이버 클립",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 클립 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "클립 예약 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 창작자 링크 입력 + 개수 N + 추가(데스크톱 클립 카드 미러) */}
      <Text size="xs" c="dimmed" mb={4}>
        클립 창작자 링크 (창작자의 최신 N개 미디어에 댓글을 답니다)
      </Text>
      <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
        <TextInput
          size="xs"
          style={{ flex: 1 }}
          placeholder="https://clip.naver.com/@dongzzi_chef"
          value={link}
          onChange={(e) => setLink(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addLink();
          }}
          styles={{ input: { fontFamily: "monospace" } }}
          aria-label="클립 창작자 링크"
        />
        <NumberInput
          size="xs"
          label="개수"
          w={90}
          min={1}
          max={50}
          value={count}
          onChange={(v) => setCount(typeof v === "number" ? v : "")}
          aria-label="최신 미디어 개수"
        />
        <Button
          size="xs"
          variant="light"
          color="green"
          disabled={!link.trim()}
          onClick={addLink}
        >
          추가
        </Button>
      </Group>

      {items.length > 0 ? (
        <Group gap={6} mb="sm">
          {items.map((it) => {
            const key = clipItemKey(it);
            return (
              <Badge
                key={key}
                color="green"
                variant="light"
                radius="sm"
                rightSection={
                  <ActionIcon
                    size={14}
                    variant="transparent"
                    color="green"
                    aria-label={`${clipItemLabel(it)} 제거`}
                    onClick={() => onRemove(key)}
                  >
                    <Icon.x size={10} />
                  </ActionIcon>
                }
              >
                {clipItemLabel(it)}
              </Badge>
            );
          })}
        </Group>
      ) : (
        <Text fz={12} c="orange.7" mb="sm">
          댓글을 달 클립 창작자를 추가하세요.
        </Text>
      )}

      {/* 계정 선택 — 클립은 유효 쿠키 필요(로그인 성공 계정만). */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 클립 로그인 성공 계정만 · {accts.length}명 선택)
      </Text>
      <Group gap={6}>
        {accounts.length === 0 ? (
          <Text size="xs" c="dimmed">
            이 하위에 로그인 성공한 클립 계정이 없습니다(계정 분배에서
            플랫폼=네이버 클립으로 분배·로그인하세요).
          </Text>
        ) : (
          accounts.map((a) => {
            const on = accts.includes(a);
            return (
              <Button
                key={a}
                size="xs"
                variant={on ? "filled" : "default"}
                color={on ? "green" : "gray"}
                onClick={() =>
                  setAccts((prev) =>
                    on ? prev.filter((x) => x !== a) : [...prev, a],
                  )
                }
              >
                {maskId(a)}
              </Button>
            );
          })
        )}
      </Group>

      {/* 지금/예약 게시(클립 댓글은 나눠서 없음). */}
      <Stack gap={8} mt="md">
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!valid}
            leftSection={<Icon.bolt size={15} />}
            onClick={runNow}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed ? "filled" : "light"}
            color="grape"
            disabled={!valid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            예약 게시
          </Button>
        </Group>
        {armed && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              클립 예약 — 게시 시각 선택
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
                onClick={() => setArmed(false)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}
      </Stack>
    </Box>
  );
}

// ── 네이버 밴드(band.us) 게시 대상 ──
// 데스크톱 밴드 카드 미러: 밴드 링크를 추가하면 band_no를 파싱해 목록에 쌓고, 고른 밴드가 동그라미
// 배지로 뜬다. 글/댓글/글+댓글 모두 밴드 링크(카페와 동일 UX). 댓글 대상/개수(최신·인기·특정글URL)는
// 글(LibraryPost)에 동결된 값을 엔진이 쓴다. 밴드명 실시간 조회는 band 세션이 필요해 Admin에선 안 함
// — band_no 라벨만 쓰고, 실제 게시는 하위가 band.us 쿠키로 수행한다(카페의 게시판명 해석과 동일).
/** 밴드 링크에서 band_no를 뽑는다(순수·데스크톱 bandNoFromLink 미러). `/band/{no}`·숫자만·실패 시 원문. */
function bandNoFromLink(link: string): string {
  const t = link.trim();
  const m = t.match(/\/band\/(\d+)/);
  if (m?.[1]) return m[1];
  if (/^\d+$/.test(t)) return t;
  return t;
}
interface ResolvedBand {
  bandNo: string;
  link: string;
}
function bandKey(b: ResolvedBand): string {
  return `${b.bandNo}::${b.link}`;
}
function bandLabel(b: ResolvedBand): string {
  return `밴드 ${b.bandNo}`;
}

// 네이버 밴드 상세 구성(글/댓글/글+댓글 모두 밴드 링크). 밴드는 로그인 성공(active) 밴드 계정만 쓰고
// (상위 accountsFor가 이미 걸러 넘김), 게시 시점에 하위가 band.us 쿠키로 올린다. 댓글 대상/개수는
// 글(LibraryPost)에 동결돼 있어 여기선 고르지 않는다(엔진이 글에서 읽음 — 카페와 동일).
function BandConfig({
  device,
  kind,
  postId,
  postTitle,
  accounts,
  onSchedule,
}: {
  device: PubDevice;
  kind: PostKind;
  postId: string | null;
  postTitle: string | null;
  accounts: string[];
  onSchedule: (item: ScheduledItem) => void;
}) {
  const [link, setLink] = useState("");
  const [resolved, setResolved] = useState<ResolvedBand[]>([]);
  const [selected, setSelected] = useState<string[]>([]); // bandKey 목록
  const [accts, setAccts] = useState<string[]>([]);
  const [armed, setArmed] = useState<boolean>(false);
  const [sched, setSched] = useState(() => nowParts());
  // 댓글 대상 모드/개수(댓글 모드에서만). url=특정 글 URL(band_comment_on_post),
  // latest/popular=최신/인기 상위 N개(band_comment). 엔진이 실제 지원하는 대상을 그대로 노출.
  const [commentMode, setCommentMode] = useState<CommentTargetMode>("latest");
  const [count, setCount] = useState<number | "">(20);

  const isComment = kind === "comment";
  const isList = isComment && commentMode !== "url";
  const commentFields = () =>
    commentTargetPayload(
      kind,
      commentMode,
      typeof count === "number" ? count : 1,
    );
  const selectedBands = resolved.filter((b) => selected.includes(bandKey(b)));
  const valid =
    postTitle != null && selectedBands.length > 0 && accts.length > 0;
  const detail = isComment
    ? `${COMMENT_MODE_OPTS.find((m) => m.value === commentMode)?.label} · 밴드 ${selectedBands.length}개 · 계정 ${accts.length}`
    : `밴드 ${selectedBands.length}개 · 계정 ${accts.length}`;

  // 링크 추가: band_no 파싱해 목록에 쌓는다(밴드 홈 또는 특정 글 URL — 게시 시점 백엔드가 해석).
  const addLink = () => {
    const raw = link.trim();
    if (!raw) return;
    const bandNo = bandNoFromLink(raw);
    if (!bandNo) {
      notifications.show({
        title: "링크 인식 실패",
        message: "밴드 링크를 확인하세요.",
        color: "red",
      });
      return;
    }
    const b: ResolvedBand = { bandNo, link: raw };
    const key = bandKey(b);
    setResolved((prev) =>
      prev.some((x) => bandKey(x) === key) ? prev : [...prev, b],
    );
    setLink("");
  };
  const onSelect = (key: string) =>
    setSelected((prev) => (prev.includes(key) ? prev : [...prev, key]));
  const onRemove = (key: string) =>
    setSelected((prev) => prev.filter((k) => k !== key));

  const bandTargetsPayload = () =>
    selectedBands.map((b) => ({ bandNo: b.bandNo, link: b.link }));
  const assignments = () => accts.map((loginId) => ({ loginId, stocks: [] }));

  const runNow = () => {
    void (async () => {
      try {
        await api.publish.send({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 밴드",
          split: false,
          mode: kind,
          target: "band",
          bandTargets: bandTargetsPayload(),
          ...commentFields(),
          assignments: assignments(),
        });
        notifications.show({
          title: `${device.name} · 밴드 게시 명령 전송`,
          message: `"${shortTitle(postTitle ?? "")}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 밴드 게시(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "밴드 게시 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  const confirmSchedule = () => {
    const at = toEpochMs(sched.date, sched.time);
    const when = scheduleMoment(sched.date, sched.time).when;
    setArmed(false);
    void (async () => {
      try {
        await api.scheduled.create({
          deviceId: device.id,
          postId: postId ?? "",
          postTitle: postTitle ?? "",
          targetLabel: "네이버 밴드",
          split: false,
          mode: kind,
          target: "band",
          bandTargets: bandTargetsPayload(),
          ...commentFields(),
          assignments: assignments(),
          at,
          detail,
        });
        notifications.show({
          title: `${device.name} 밴드 예약`,
          message: `${when} · ${detail}`,
          color: "grape",
        });
      } catch (e) {
        if (isOffline(e)) {
          onSchedule({
            id: `sch-${Date.now()}-${Math.floor(Math.random() * 1e6)}`,
            deviceName: device.name,
            postTitle: postTitle ?? "-",
            targetLabel: "네이버 밴드",
            detail,
            at,
          });
          notifications.show({
            title: `${device.name} 밴드 예약(미리보기)`,
            message: `${when} · ${detail} · 서버 오프라인(로컬에만 표시)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "밴드 예약 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 댓글 대상 모드(댓글 모드에서만) — 특정 글 / 최신글 / 인기글. 엔진이 실제 지원하는
          밴드 댓글 대상(band_comment_on_post=url · band_comment=latest/popular)을 그대로 노출. */}
      {isComment && (
        <>
          <Text size="xs" c="dimmed" mb={4}>
            댓글 대상
          </Text>
          <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
            {COMMENT_MODE_OPTS.map((m) => (
              <Button
                key={m.value}
                size="xs"
                variant={commentMode === m.value ? "filled" : "default"}
                color={commentMode === m.value ? "teal" : "gray"}
                onClick={() => setCommentMode(m.value)}
                aria-label={`밴드 댓글 대상 ${m.label}`}
              >
                {m.label}
              </Button>
            ))}
            {isList && (
              <NumberInput
                size="xs"
                label="개수"
                w={90}
                min={1}
                max={50}
                value={count}
                onChange={(v) => setCount(typeof v === "number" ? v : "")}
                aria-label="밴드 최신/인기 댓글 개수"
              />
            )}
          </Group>
        </>
      )}
      {/* 밴드 링크 입력 + 추가(데스크톱 밴드 카드와 동일 UX) */}
      <Text size="xs" c="dimmed" mb={4}>
        {isComment && commentMode === "url"
          ? "밴드 글 링크 (넣은 특정 글에 저장된 댓글을 답니다)"
          : isList
            ? "밴드 링크 (그 밴드의 최신/인기 상위 N개 글에 댓글을 답니다)"
            : `밴드 링크 (넣은 밴드에 ${kind === "comment" ? "댓글을" : "글을"} 올립니다)`}
      </Text>
      <Group gap={8} align="flex-end" wrap="nowrap" mb="sm">
        <TextInput
          size="xs"
          style={{ flex: 1 }}
          placeholder={
            isComment && commentMode === "url"
              ? "https://band.us/band/103043410/post/9"
              : "https://band.us/band/103043410"
          }
          value={link}
          onChange={(e) => setLink(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addLink();
          }}
          styles={{ input: { fontFamily: "monospace" } }}
          aria-label="밴드 링크"
        />
        <Button
          size="xs"
          variant="light"
          color="teal"
          disabled={!link.trim()}
          onClick={addLink}
        >
          추가
        </Button>
      </Group>
      <Select
        size="xs"
        mb="sm"
        placeholder={
          resolved.length > 0
            ? "게시할 밴드 선택"
            : "밴드 링크를 추가하면 여기 표시됩니다"
        }
        data={resolved.map((b) => ({ value: bandKey(b), label: bandLabel(b) }))}
        value={null}
        disabled={resolved.length === 0}
        comboboxProps={{ withinPortal: true }}
        onChange={(k) => {
          if (k) onSelect(k);
        }}
        aria-label="밴드 선택"
      />
      {selectedBands.length > 0 ? (
        <Group gap={6} mb="sm">
          {selectedBands.map((b) => {
            const key = bandKey(b);
            return (
              <Badge
                key={key}
                color="teal"
                variant="light"
                radius="sm"
                rightSection={
                  <ActionIcon
                    size={14}
                    variant="transparent"
                    color="teal"
                    aria-label={`${bandLabel(b)} 제거`}
                    onClick={() => onRemove(key)}
                  >
                    <Icon.x size={10} />
                  </ActionIcon>
                }
              >
                {bandLabel(b)}
              </Badge>
            );
          })}
        </Group>
      ) : (
        <Text fz={12} c="orange.7" mb="sm">
          게시할 밴드를 선택하세요.
        </Text>
      )}

      {/* 계정 선택 — 밴드는 로그인 성공(active) 밴드 계정만(band.us 쿠키 필요). */}
      <Text size="xs" c="dimmed" mb={4}>
        계정 (이 하위의 밴드 로그인 성공 계정만 · {accts.length}명 선택)
      </Text>
      <Group gap={6}>
        {accounts.length === 0 ? (
          <Text size="xs" c="dimmed">
            이 하위에 로그인 성공한 밴드 계정이 없습니다(계정 분배에서
            플랫폼=밴드로 분배·로그인하세요).
          </Text>
        ) : (
          accounts.map((a) => {
            const on = accts.includes(a);
            return (
              <Button
                key={a}
                size="xs"
                variant={on ? "filled" : "default"}
                color={on ? "teal" : "gray"}
                onClick={() =>
                  setAccts((prev) =>
                    on ? prev.filter((x) => x !== a) : [...prev, a],
                  )
                }
              >
                {maskId(a)}
              </Button>
            );
          })
        )}
      </Group>

      {/* 지금/예약 게시 */}
      <Stack gap={8} mt="md">
        <Group grow gap="xs">
          <Button
            size="sm"
            fw={700}
            disabled={!valid}
            leftSection={<Icon.bolt size={15} />}
            onClick={runNow}
          >
            지금 게시
          </Button>
          <Button
            size="sm"
            fw={700}
            variant={armed ? "filled" : "light"}
            color="grape"
            disabled={!valid}
            leftSection={<Icon.calendar size={15} />}
            onClick={() => setArmed(true)}
          >
            예약 게시
          </Button>
        </Group>
        {armed && (
          <Paper withBorder radius="md" p="sm" bg="var(--mantine-color-gray-0)">
            <Text fz={12} fw={700} mb={6}>
              밴드 예약 — 게시 시각 선택
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
                onClick={() => setArmed(false)}
              >
                취소
              </Button>
            </Group>
          </Paper>
        )}
      </Stack>
    </Box>
  );
}
