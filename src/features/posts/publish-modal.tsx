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
import { notifications } from "@mantine/notifications";
import { useCallback, useEffect, useRef, useState } from "react";

import type { JoinedCafe } from "@/shared/bindings/JoinedCafe";
import type { PostJob } from "@/shared/bindings/PostJob";
import type { PublishOutcome } from "@/shared/bindings/PublishOutcome";
import { KIND, STATUS_ACCOUNT } from "@/shared/data/config";
import {
  acctPlatforms,
  hasToken,
  resolveTemplate,
} from "@/shared/data/helpers";
import type {
  Account,
  Band,
  Board,
  GoFn,
  LibraryPost,
  PlatformId,
  PublishJob,
  PublishResult,
  QueueLocation,
  QueueScheduledItem,
  Stock,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo, PlatformPill } from "@/shared/ui/platform-logo";

import {
  buildBothCommentJobs,
  buildUrlCommentJobs,
  commentSummary,
  parseCafeArticleUrl,
} from "./comment-jobs";
import { PreviewModal } from "./preview-modal";
import { StockCrawlModal } from "./stock-crawl-modal";

export interface PublishModalProps {
  open: boolean;
  doc: LibraryPost | null;
  onClose: () => void;
  go: GoFn;
}

/**
 * A single naver account's chosen cafe + board, resolved into the ids the
 * backend needs (`cafeId`/`menuId`/`boardType`). Built from the account's
 * joined-cafe list and the cafe's writable boards.
 */
interface NaverPick {
  cafeId: number;
  cafeName: string;
  cafeUrl: string;
  boardName: string;
  menuId: number;
  boardType: string;
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
  band,
  setBand,
  bands,
  stocks,
  naverAccounts,
  joinedByAccount,
  joinedLoading,
  boardsByCafe,
  boardsLoading,
  naverPicks,
  onPickCafe,
  onPickBoard,
  onRefreshJoined,
}: {
  selPlatforms: PlatformId[];
  stockCodes: string[];
  openStockModal: () => void;
  removeStock: (code: string) => void;
  band: string;
  setBand: (v: string) => void;
  bands: Band[];
  stocks: Stock[];
  naverAccounts: Account[];
  joinedByAccount: Record<string, JoinedCafe[]>;
  joinedLoading: Record<string, boolean>;
  boardsByCafe: Record<string, Board[]>;
  boardsLoading: Record<string, boolean>;
  naverPicks: Record<string, NaverPick | undefined>;
  onPickCafe: (accountId: string, cafeId: number) => void;
  onPickBoard: (accountId: string, boardName: string) => void;
  onRefreshJoined: (accountId: string) => void;
}) {
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
          <Group gap={9} px={11} py={9} wrap="nowrap" style={head}>
            <PlatformLogo id="naver" size={22} />
            <Text fz={13} fw={700} style={{ flex: 1 }}>
              네이버 카페
            </Text>
            <Text fz={11.5} c="gray.5">
              계정별 가입 카페에서 선택
            </Text>
          </Group>
          <Stack gap={8} p={10}>
            {naverAccounts.map((a) => {
              const joined = joinedByAccount[a.id];
              const loading = joinedLoading[a.id];
              const pick = naverPicks[a.id];
              const cafeKey = pick ? String(pick.cafeId) : "";
              const boards = pick ? boardsByCafe[cafeKey] : undefined;
              const bLoading = pick ? boardsLoading[cafeKey] : false;
              return (
                <Group key={a.id} gap={8} wrap="nowrap" align="center">
                  <Text
                    fz={12}
                    fw={700}
                    ff="monospace"
                    c="dimmed"
                    truncate
                    style={{ width: 92, flexShrink: 0 }}
                  >
                    {a.loginId}
                  </Text>
                  {loading && !joined ? (
                    <Group gap={6} style={{ flex: 1 }}>
                      <Loader size="xs" />
                      <Text fz={12} c="dimmed">
                        가입 카페 불러오는 중…
                      </Text>
                    </Group>
                  ) : joined && joined.length === 0 ? (
                    <Text fz={12} c="gray.5" style={{ flex: 1 }}>
                      가입한 카페가 없어요
                    </Text>
                  ) : (
                    <>
                      <Select
                        placeholder="가입 카페 선택"
                        value={pick ? String(pick.cafeId) : null}
                        data={(joined ?? []).map((c) => ({
                          value: String(c.cafeId),
                          label: c.cafeName,
                        }))}
                        onChange={(v) => v && onPickCafe(a.id, Number(v))}
                        searchable
                        style={{ flex: 1 }}
                      />
                      <Select
                        placeholder={bLoading ? "불러오는 중…" : "게시판"}
                        value={pick?.boardName || null}
                        data={(boards ?? []).map((b) => b.name)}
                        onChange={(v) => v && onPickBoard(a.id, v)}
                        disabled={
                          !pick || bLoading || (boards?.length ?? 0) === 0
                        }
                        style={{ width: 132, flexShrink: 0 }}
                      />
                    </>
                  )}
                  <ActionIcon
                    variant="subtle"
                    color="gray"
                    aria-label="가입 카페 새로고침"
                    onClick={() => onRefreshJoined(a.id)}
                  >
                    <Icon.refresh size={15} />
                  </ActionIcon>
                </Group>
              );
            })}
          </Stack>
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

/** Fresh id for a newly scheduled queue item (kept out of render per purity). */
function newScheduledId(): string {
  return "qs" + Date.now();
}

/** Flatten the document's HTML body into plain text for the article body. */
function htmlToText(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/(p|div|li|h[1-6])>/gi, "\n")
    .replace(/<[^>]*>/g, "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** Map a backend per-job outcome onto the UI's PublishResult. */
// 백엔드가 거부하는 값은 ErrorEnvelope(`{ code, message? }`)이거나 Error다. 사용자에게
// 보일 짧은 사유 문자열로 환원한다(쿠키 만료/없음 등 침묵 실패를 드러내기 위함).
function errText(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (err && typeof err === "object") {
    const e = err as { message?: unknown; code?: unknown };
    if (typeof e.message === "string" && e.message) return e.message;
    if (typeof e.code === "string" && e.code) return e.code;
  }
  return String(err);
}

function outcomeToResult(
  job: PublishJob,
  outcome: PublishOutcome | undefined,
  action: string,
): PublishResult {
  const ok = outcome?.success ?? false;
  return {
    ...job,
    ok,
    msg: ok
      ? `${action} 게시 완료`
      : (outcome?.errorMessage ?? "게시 실패 — 잠시 후 재시도"),
  };
}

/**
 * Fallback Chrome DevTools endpoint, used ONLY when the backend command isn't
 * available (browser preview / Vitest). The authoritative endpoint comes from
 * the backend (`ipc.forum.endpoint()` → `forum_endpoint`), so the port is not a
 * hardcoded frontend constant in the real app.
 */
function fallbackEndpoint(): { host: string; port: number } {
  return { host: "127.0.0.1", port: 9222 };
}

const pad2 = (n: number) => String(n).padStart(2, "0");

/** Current date/time as the picker's `{ date, time }` strings (minute precision). */
function nowParts(): { date: string; time: string } {
  const n = new Date();
  return {
    date: `${n.getFullYear()}-${pad2(n.getMonth() + 1)}-${pad2(n.getDate())}`,
    time: `${pad2(n.getHours())}:${pad2(n.getMinutes())}`,
  };
}

/** Local epoch-ms for a `YYYY-MM-DD` + `HH:MM` pair (for the IPC time guard). */
function toEpochMs(date: string, time: string): number {
  const [y, m, d] = date.split("-").map(Number);
  const [h, mi] = time.split(":").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1, h ?? 0, mi ?? 0).getTime();
}

/** Turn the picked date/time into the queue's `{ when, rel }` display strings. */
function scheduleMoment(
  date: string,
  time: string,
): { label: string; when: string } {
  const [y, m, d] = date.split("-").map(Number);
  const target = new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const diff = Math.round((target.getTime() - today.getTime()) / 86400000);
  const label =
    diff <= 0
      ? "오늘"
      : diff === 1
        ? "내일"
        : diff === 2
          ? "모레"
          : `${m}/${d}`;
  return { label, when: `${label} ${time}` };
}

function PublishModalInner({ open, doc, onClose, go }: PublishModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [stocks, setStocks] = useState<Stock[]>([]);
  const [bands, setBands] = useState<Band[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [stockCodes, setStockCodes] = useState<string[]>(["005930"]);
  const [stockModal, setStockModal] = useState(false);
  const [band, setBand] = useState("");
  // Account-driven naver state: joined cafes per account, boards per cafe, and
  // each account's chosen cafe/board. Loaded live on selection and cached for
  // the modal session (the refresh control re-fetches).
  const [joinedByAccount, setJoinedByAccount] = useState<
    Record<string, JoinedCafe[]>
  >({});
  const [joinedLoading, setJoinedLoading] = useState<Record<string, boolean>>(
    {},
  );
  const [boardsByCafe, setBoardsByCafe] = useState<Record<string, Board[]>>({});
  const [boardsLoading, setBoardsLoading] = useState<Record<string, boolean>>(
    {},
  );
  const [naverPicks, setNaverPicks] = useState<
    Record<string, NaverPick | undefined>
  >({});
  const joinedReqRef = useRef<Set<string>>(new Set());
  const boardPromiseRef = useRef<Map<string, Promise<Board[]>>>(new Map());
  const [when, setWhen] = useState<"now" | "schedule">("now");
  const [date, setDate] = useState(() => nowParts().date);
  const [time, setTime] = useState(() => nowParts().time);
  const [acctFilter, setAcctFilter] = useState<"all" | PlatformId>("all");
  const [linkOverride, setLinkOverride] = useState("");
  const [showPreview, setShowPreview] = useState(false);
  const [flow, setFlow] = useState<null | "running" | PublishResult[]>(null);
  // 게시 엔드포인트는 백엔드가 단일 출처. 받아오기 전/실패 시엔 폴백을 쓴다(브라우저·테스트).
  const [endpoint, setEndpoint] = useState<{ host: string; port: number }>(
    fallbackEndpoint,
  );

  useEffect(() => {
    void ipc.forum
      .endpoint()
      .then(setEndpoint)
      .catch(() => {
        /* 비-Tauri 환경: 폴백 유지 */
      });
    void ipc.accounts.list().then((a) => {
      setAccounts(a);
      const firstUsable = a.find((x) => x.status !== "error");
      setSelected((s) => (s.length || !firstUsable ? s : [firstUsable.id]));
    });
    void ipc.stocks.list().then(setStocks);
    void ipc.bands.list().then((b) => {
      setBands(b);
      setBand((cur) => cur || (b[0]?.name ?? ""));
    });
  }, []);

  // Fetch an account's joined cafes once (cached in `joinedReqRef`); the refresh
  // control clears the guard to force a re-fetch. State is keyed by the UI's
  // unique account id, but the backend looks cafes up by the account's `loginId`
  // (its cookie-file key) — passing the UI id finds no cookie and returns nothing.
  const fetchJoined = useCallback(
    (accountId: string) => {
      if (joinedReqRef.current.has(accountId)) return;
      const loginId = accounts.find((a) => a.id === accountId)?.loginId;
      if (!loginId) return;
      joinedReqRef.current.add(accountId);
      setJoinedLoading((m) => ({ ...m, [accountId]: true }));
      ipc.cafes
        .listJoined(loginId)
        .then((cs) => setJoinedByAccount((m) => ({ ...m, [accountId]: cs })))
        .catch((err) => {
          notifications.show({
            message: `${loginId} 가입 카페를 불러오지 못했어요: ${errText(err)}`,
            color: "red",
          });
          setJoinedByAccount((m) => ({ ...m, [accountId]: [] }));
        })
        .finally(() => setJoinedLoading((m) => ({ ...m, [accountId]: false })));
    },
    [accounts],
  );

  // Lazily discover a cafe's writable boards (reuses resolve_cafe, which returns
  // boards for a numeric cafeId). De-dupes concurrent/repeat calls via an
  // in-flight promise cache and resolves to the boards for the caller.
  const resolveBoards = useCallback(
    (cafeId: number, accountId: string): Promise<Board[]> => {
      const key = String(cafeId);
      const existing = boardPromiseRef.current.get(key);
      if (existing) return existing;
      // resolve_cafe reads the account's cookie too — pass the `loginId`, not the
      // UI account id (same cookie-file-key mismatch as fetchJoined).
      const loginId = accounts.find((a) => a.id === accountId)?.loginId;
      if (!loginId) return Promise.resolve([]);
      setBoardsLoading((m) => ({ ...m, [key]: true }));
      const p = ipc.cafes
        .resolve(key, loginId)
        .then((c) => {
          setBoardsByCafe((m) => ({ ...m, [key]: c.boards }));
          return c.boards;
        })
        .catch((err): Board[] => {
          notifications.show({
            message: `${loginId} 게시판을 불러오지 못했어요: ${errText(err)}`,
            color: "red",
          });
          setBoardsByCafe((m) => ({ ...m, [key]: [] }));
          return [];
        })
        .finally(() => setBoardsLoading((m) => ({ ...m, [key]: false })));
      boardPromiseRef.current.set(key, p);
      return p;
    },
    [accounts],
  );

  // Auto-load joined cafes for every selected naver account.
  useEffect(() => {
    selected.forEach((aid) => {
      const a = accounts.find((x) => x.id === aid);
      if (a?.platform === "naver") fetchJoined(aid);
    });
  }, [selected, accounts, fetchJoined]);

  if (!doc) {
    return <Modal opened={false} onClose={onClose} />;
  }

  const mode = doc.kind;
  const comments = (doc.comments ?? []).filter(Boolean);
  const commentTargetMode = doc.commentTarget ?? "latest";
  // Phase 1 wires two comment targets: the just-posted article (`both`) and a
  // pasted article URL (`comment` + url). latest/popular need a board-listing
  // backend (Phase 2) and are not posted yet.
  const urlTarget =
    commentTargetMode === "url" ? parseCafeArticleUrl(doc.commentUrl) : null;
  const toggle = (id: string) =>
    setSelected((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  const selPlatforms = acctPlatforms(selected, accounts);
  const selectedNaver = selected
    .map((id) => accounts.find((a) => a.id === id))
    .filter((a): a is Account => !!a && a.platform === "naver");

  // Choose a cafe for an account: seed the pick, then discover the cafe's boards
  // and default to the first one (unless the user has since changed cafe/board).
  const pickCafe = (accountId: string, cafeId: number) => {
    const jc = (joinedByAccount[accountId] ?? []).find(
      (c) => c.cafeId === cafeId,
    );
    if (!jc) return;
    setNaverPicks((m) => ({
      ...m,
      [accountId]: {
        cafeId: jc.cafeId,
        cafeName: jc.cafeName,
        cafeUrl: jc.cafeUrl,
        boardName: "",
        menuId: 0,
        boardType: "L",
      },
    }));
    void resolveBoards(jc.cafeId, accountId).then((boards) => {
      const first = boards[0];
      if (!first) return;
      setNaverPicks((m) => {
        const p = m[accountId];
        // Skip if the account moved to another cafe or already picked a board.
        if (!p || p.cafeId !== jc.cafeId || p.boardName) return m;
        return {
          ...m,
          [accountId]: {
            ...p,
            boardName: first.name,
            menuId: first.menuId,
            boardType: first.boardType,
          },
        };
      });
    });
  };

  const pickBoard = (accountId: string, boardName: string) => {
    const pick = naverPicks[accountId];
    if (!pick) return;
    const b = (boardsByCafe[String(pick.cafeId)] ?? []).find(
      (x) => x.name === boardName,
    );
    if (!b) return;
    setNaverPicks((m) => ({
      ...m,
      [accountId]: {
        ...pick,
        boardName: b.name,
        menuId: b.menuId,
        boardType: b.boardType,
      },
    }));
  };

  // Refresh: drop the cached joined list + pick for this account and re-fetch.
  const refreshJoined = (accountId: string) => {
    joinedReqRef.current.delete(accountId);
    setJoinedByAccount((m) => {
      const n = { ...m };
      delete n[accountId];
      return n;
    });
    setNaverPicks((m) => {
      const n = { ...m };
      delete n[accountId];
      return n;
    });
    fetchJoined(accountId);
  };

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
      if (mode === "comment") {
        // Comment-only: the target is the URL / latest / popular, not a board
        // pick. One job per selected account.
        const targetName =
          commentTargetMode === "url"
            ? urlTarget
              ? `게시글 #${urlTarget.articleId}`
              : "URL 미설정"
            : commentTargetMode === "popular"
              ? "인기글"
              : "최신글";
        jobs.push({
          key: aid,
          platform: "naver",
          loginId: a.loginId,
          targetName,
          board: "댓글",
          status: a.status,
        });
      } else {
        const pick = naverPicks[aid];
        // Skip accounts whose cafe/board isn't fully chosen yet.
        if (!pick || !pick.boardName) return;
        jobs.push({
          key: aid,
          platform: "naver",
          loginId: a.loginId,
          targetName: pick.cafeName,
          board: pick.boardName,
          status: a.status,
        });
      }
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
  // Comment-only mode needs comments and a resolved target. Phase 1 only resolves
  // the `url` target; latest/popular are Phase 2, so they can't publish yet.
  const commentReady =
    mode !== "comment" || (comments.length > 0 && urlTarget !== null);
  const canPublish =
    selected.length > 0 && targetsOk && commentReady && jobs.length > 0;

  const action =
    mode === "comment" ? "댓글" : mode === "both" ? "글+댓글" : "글";

  // Build the backend PostJob for a naver UI job — the cafeId/menuId/boardType
  // come straight from the account's pick (joined-cafe + resolved board).
  const toPostJob = (j: PublishJob): PostJob => {
    const pick = naverPicks[j.key];
    return {
      // 백엔드는 loginId(쿠키 파일 키)로 계정을 찾는다. UI 키(j.key)는 picks 조회용일 뿐.
      accountId: j.loginId,
      cafe: pick ? String(pick.cafeId) : j.targetName,
      menuId: pick?.menuId ?? 0,
      boardType: pick?.boardType ?? "L",
      subject: doc.title,
      bodyText: htmlToText(doc.body ?? ""),
      tagList: [],
    };
  };

  // post / both: post each naver article. For `both`, then comment on every
  // successfully-posted article and fold a "댓글 N/M건" summary into its row.
  const runNaverPosts = async (
    naverJobs: PublishJob[],
  ): Promise<PublishResult[]> => {
    if (!naverJobs.length) return [];
    const outs = await ipc.cafes
      .runPostJobs(naverJobs.map(toPostJob))
      .catch((): null => null);
    if (!outs) {
      return naverJobs.map((j) => ({
        ...j,
        ok: false,
        msg: "게시 실패 — 잠시 후 재시도",
      }));
    }
    const postResults = naverJobs.map((j, i) =>
      outcomeToResult(j, outs[i], action),
    );
    if (mode !== "both" || comments.length === 0) return postResults;

    // Comment on each post that actually landed, reusing its returned articleId.
    const posted = naverJobs
      .map((j, i) => ({ j, out: outs[i] }))
      .filter(
        (x): x is { j: PublishJob; out: PublishOutcome } =>
          !!x.out && x.out.success && x.out.articleId != null,
      )
      .map((x) => ({
        accountId: x.j.loginId,
        cafeId: naverPicks[x.j.key]?.cafeId ?? 0,
        articleId: x.out.articleId as number,
      }));
    const commentJobs = buildBothCommentJobs(posted, comments);
    if (!commentJobs.length) return postResults;
    const couts = await ipc.cafes
      .runCommentJobs(commentJobs)
      .catch((): null => null);
    return postResults.map((r) =>
      r.ok
        ? { ...r, msg: `${r.msg} · ${commentSummary(couts, r.loginId)}` }
        : r,
    );
  };

  // comment-only (url target): comment on the parsed article with each account.
  const runNaverComments = async (
    naverJobs: PublishJob[],
  ): Promise<PublishResult[]> => {
    if (!naverJobs.length) return [];
    if (!urlTarget || comments.length === 0) {
      return naverJobs.map((j) => ({
        ...j,
        ok: false,
        msg: "댓글 대상 URL 또는 댓글 내용이 없어요",
      }));
    }
    const commentJobs = buildUrlCommentJobs(
      naverJobs.map((j) => j.loginId),
      urlTarget,
      comments,
    );
    const couts = await ipc.cafes
      .runCommentJobs(commentJobs)
      .catch((): null => null);
    return naverJobs.map((j) => {
      const mine = (couts ?? []).filter((o) => o.accountId === j.loginId);
      return {
        ...j,
        // 한 계정의 댓글이 여러 건이면 모두 성공해야 성공으로 본다 — 일부만 올라간
        // 경우(예: 2건 중 1건)를 성공 배지로 묻지 않는다. 자세한 건수는 msg에 표시.
        ok: couts != null && mine.length > 0 && mine.every((o) => o.success),
        msg: commentSummary(couts, j.loginId),
      };
    });
  };

  // Publish now: naver cafe(글/댓글)와 종목토론방(forum)은 실제 백엔드를 호출하고,
  // 밴드 등 나머지는 엔진이 없어 시뮬레이션으로 표시한다.
  const runNow = () => {
    setFlow("running");
    // 플랫폼별로 갈래를 나눈다: 네이버 카페·종목토론방은 실제 백엔드, 밴드 등
    // 나머지는 엔진 미구현이라 시뮬레이션(후속 작업).
    const naverJobs = jobs.filter((j) => j.platform === "naver");
    const forumJobs = jobs.filter((j) => j.platform === "forum");
    const otherJobs = jobs.filter(
      (j) => j.platform !== "naver" && j.platform !== "forum",
    );

    // 네이버 카페: 실제 백엔드(글/댓글).
    const naverWork: Promise<PublishResult[]> =
      mode === "comment"
        ? runNaverComments(naverJobs)
        : runNaverPosts(naverJobs);

    // 종목토론방(forum): 패킷 게시 엔진을 계정별로 호출한다.
    const ep = endpoint;
    const firstComment = (doc.comments ?? []).find((c) => c.trim()) ?? "";
    const byAccount = new Map<string, typeof forumJobs>();
    forumJobs.forEach((j) => {
      const list = byAccount.get(j.loginId) ?? [];
      list.push(j);
      byAccount.set(j.loginId, list);
    });
    const forumWork: Promise<PublishResult[]> = Promise.all(
      [...byAccount.entries()].map(([loginId, accJobs]) =>
        ipc.forum
          .publishNow({
            host: ep.host,
            port: ep.port,
            // 계정 loginId로 저장된 로그인 쿠키를 사용한다.
            accountId: loginId,
            runPost: mode === "post" || mode === "both",
            runComment: mode === "comment" || mode === "both",
            title: doc.title,
            body: doc.body ?? "",
            comment: firstComment,
            stocks: accJobs.map((j) => ({
              name: j.targetName,
              code: j.code ?? "",
              link: "",
            })),
          })
          .then((results) => {
            // 엔진이 결과를 비워(빈 배열·누락) 돌려줄 수 있으므로 방어적으로 다룬다.
            const list = Array.isArray(results) ? results : [];
            // 결과는 code가 아니라 보낸 순서(인덱스)로 매칭한다. 백엔드(run_forum_publish)는
            // 보낸 stocks 순서대로 결과를 돌려주므로, 같은 code가 두 번 들어가도 두 행이 첫
            // 결과에 묶여 두 번째 종목의 실제 결과(성공 중복/실패 은폐)가 가려지지 않는다.
            return accJobs.map((j, i) => {
              const r = list[i];
              return {
                ...j,
                ok: r?.ok ?? false,
                msg: r?.message ?? "결과 없음",
              };
            });
          })
          .catch((err: unknown) =>
            accJobs.map((j) => ({
              ...j,
              ok: false,
              msg: err instanceof Error ? err.message : String(err),
            })),
          ),
      ),
    ).then((forumArr) => forumArr.flat());

    // 밴드 등 나머지: 진행 UI가 보이도록 약간 지연 후 시뮬레이션 결과를 낸다.
    const mockOthers = new Promise<PublishResult[]>((resolve) => {
      window.setTimeout(
        () =>
          resolve(
            otherJobs.map((j) => {
              const ok = Math.random() > 0.1;
              return {
                ...j,
                ok,
                msg: ok ? `${action} 게시 완료` : "게시 실패 — 잠시 후 재시도",
              };
            }),
          ),
        otherJobs.length ? 1200 : 0,
      );
    });

    void Promise.all([naverWork, forumWork, mockOthers]).then(([nr, fr, or]) =>
      setFlow([...nr, ...fr, ...or]),
    );
  };

  const doPublish = () => {
    if (when !== "schedule") {
      runNow();
      return;
    }
    // Add the post to the scheduled queue so it shows up under 예약 대기.
    const seen = new Set<string>();
    const locs: QueueLocation[] = [];
    jobs.forEach((j) => {
      const key = `${j.platform}|${j.targetName}|${j.code ?? ""}`;
      if (seen.has(key)) return;
      seen.add(key);
      locs.push({
        p: j.platform,
        name: j.targetName,
        ...(j.code ? { code: j.code } : {}),
      });
    });
    const moment = scheduleMoment(date, time);
    const item: QueueScheduledItem = {
      id: newScheduledId(),
      title: doc.title,
      kind: doc.kind,
      when: moment.when,
      rel: moment.label,
      locs,
    };
    // Defense-in-depth: the backend rejects a past time even though the picker
    // already prevents it.
    ipc.queue
      .addScheduled(item, toEpochMs(date, time))
      .then(() =>
        setFlow(
          jobs.map((j) => ({ ...j, ok: true, msg: `${action} 예약 완료` })),
        ),
      )
      .catch(() =>
        notifications.show({
          message: "예약 시각이 현재보다 과거예요. 시간을 다시 선택하세요.",
          color: "red",
        }),
      );
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
              band={band}
              setBand={setBand}
              bands={bands}
              stocks={stocks}
              naverAccounts={selectedNaver}
              joinedByAccount={joinedByAccount}
              joinedLoading={joinedLoading}
              boardsByCafe={boardsByCafe}
              boardsLoading={boardsLoading}
              naverPicks={naverPicks}
              onPickCafe={pickCafe}
              onPickBoard={pickBoard}
              onRefreshJoined={refreshJoined}
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
          <Box mt={10}>
            <DateTimePicker
              date={date}
              time={time}
              onChange={(v) => {
                setDate(v.date);
                setTime(v.time);
              }}
            />
          </Box>
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
