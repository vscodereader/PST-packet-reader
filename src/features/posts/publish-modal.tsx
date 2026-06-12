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

import type { BandTarget } from "@/shared/bindings/BandTarget";
import type { CommentTargetSpec } from "@/shared/bindings/CommentTargetSpec";
import type { ForumTarget } from "@/shared/bindings/ForumTarget";
import type { JoinedCafe } from "@/shared/bindings/JoinedCafe";
import type { NaverTarget } from "@/shared/bindings/NaverTarget";
import type { PostJob } from "@/shared/bindings/PostJob";
import type { PublishOutcome } from "@/shared/bindings/PublishOutcome";
import { isPostable, KIND, STATUS_ACCOUNT } from "@/shared/data/config";
import {
  acctPlatforms,
  hasToken,
  resolveTemplate,
} from "@/shared/data/helpers";
import type {
  Account,
  Board,
  GoFn,
  LibraryPost,
  PlatformId,
  PublishJob,
  PublishPlan,
  PublishResult,
  QueueLocation,
  QueueScheduledItem,
  Stock,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo, PlatformPill } from "@/shared/ui/platform-logo";

import {
  commentSummary,
  commentsAllOk,
  parseCafeArticleUrl,
  topNArticles,
} from "./comment-jobs";
import { PreviewModal } from "./preview-modal";
import { htmlToText, unreadyNaverAccountIds } from "./publish-helpers";
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

/** 저장된 밴드 링크 하나 → 조회된 실제 밴드명. 게시 대상 목록(드롭다운)을 이룬다. */
interface ResolvedBand {
  bandNo: string;
  name: string;
  link: string;
}

/** 밴드 링크에서 band_no를 뽑는다. 숫자만/`/band/{no}`/실패 시 원문 trim. */
function bandNoFromLink(link: string): string {
  const t = link.trim();
  if (/^\d+$/.test(t)) return t;
  const m = t.match(/\/band\/(\d+)/);
  return m ? m[1]! : t;
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
  // 로그인 실패 계열(error/badCredentials/challenge/blocked)은 게시 대상에서 막는다.
  // active(정상)와 new(아직 미로그인, 게시 시 로그인 시도)만 선택 가능.
  const disabled = !isPostable(a.status);
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
      <Checkbox
        checked={selected}
        onChange={() => !disabled && onToggle(a.id)}
        onClick={(e) => e.stopPropagation()}
        size="sm"
        disabled={disabled}
      />
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
  stockNames,
  openStockModal,
  removeStock,
  bandLink,
  setBandLink,
  bandResolving,
  resolvedBands,
  selectedBands,
  onSaveBandLink,
  onSelectBand,
  onRemoveBand,
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
  stockNames: Record<string, string>;
  openStockModal: () => void;
  removeStock: (code: string) => void;
  bandLink: string;
  setBandLink: (v: string) => void;
  bandResolving: boolean;
  resolvedBands: ResolvedBand[];
  selectedBands: string[];
  onSaveBandLink: () => void;
  onSelectBand: (bandNo: string) => void;
  onRemoveBand: (bandNo: string) => void;
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
                      {stockNames[code] ??
                        stocks.find((s) => s.code === code)?.name ??
                        code}
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
          <Stack gap={8} p={10}>
            {/* 사수 요구 흐름: 가입할 밴드 링크를 한 줄씩 입력→저장하면 실제 밴드명을
                조회해 아래 드롭다운(사수 UI)에 누적. 거기서 게시할 밴드를 다중 선택→칩. */}
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
              <Group gap={6} wrap="nowrap">
                <Loader size="xs" />
                <Text fz={12} c="dimmed">
                  밴드 정보를 확인하는 중…
                </Text>
              </Group>
            )}
            {/* 사수의 드롭다운: 저장으로 누적된 실제 밴드명 목록에서 게시할 밴드 선택.
                옵션 value는 고유한 band_no, label은 표시용 밴드명 — 이름이 같은 밴드가
                둘 이상이어도 value가 겹치지 않아 Select가 깨지지(흰 화면) 않는다. */}
            <Select
              placeholder={
                resolvedBands.length
                  ? "게시할 밴드 선택"
                  : "링크를 저장하면 밴드가 여기 표시됩니다"
              }
              data={resolvedBands.map((b) => ({
                value: b.bandNo,
                label: b.name,
              }))}
              value={null}
              disabled={resolvedBands.length === 0}
              onChange={(no) => {
                if (no) onSelectBand(no);
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
                          aria-label={`${b?.name ?? no} 제거`}
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

function PublishModalInner({ open, doc, onClose, go }: PublishModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [stocks, setStocks] = useState<Stock[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [stockCodes, setStockCodes] = useState<string[]>(["005930"]);
  // 라이브 검색으로 고른 종목의 이름(시드 목록에 없을 수 있어 onConfirm에서 받아둠).
  const [stockNames, setStockNames] = useState<Record<string, string>>({});
  const [stockModal, setStockModal] = useState(false);
  // 밴드: 링크를 한 줄씩 저장하면 그 링크의 실제 밴드명을 조회해 resolvedBands에 누적하고,
  // 사수의 드롭다운에서 게시할 밴드(selectedBands=bandNo[])를 다중 선택한다.
  const [resolvedBands, setResolvedBands] = useState<ResolvedBand[]>([]);
  const [selectedBands, setSelectedBands] = useState<string[]>([]);
  const [bandLink, setBandLink] = useState("");
  const [bandResolving, setBandResolving] = useState(false);

  // 링크 저장: 링크에서 band_no를 뽑고, 선택된 밴드 계정의 쿠키로 실제 밴드명을 조회해
  // resolvedBands에 추가한다(같은 band_no는 중복 제거). 조회 실패 시 링크를 이름으로 폴백.
  const saveBandLink = () => {
    const link = bandLink.trim();
    if (!link) return;
    const bandNo = bandNoFromLink(link);
    const bandAcct = selected
      .map((id) => accounts.find((a) => a.id === id))
      .find((a): a is Account => !!a && a.platform === "band");
    setBandLink("");
    const add = (name: string) =>
      setResolvedBands((prev) =>
        prev.some((b) => b.bandNo === bandNo)
          ? prev
          : [...prev, { bandNo, name, link }],
      );
    if (!bandAcct) {
      add(link);
      return;
    }
    setBandResolving(true);
    ipc.band
      .resolveName(bandAcct.loginId, link)
      .then((name) => add(name))
      .catch(() => add(link))
      .finally(() => setBandResolving(false));
  };
  const selectBand = (no: string) =>
    setSelectedBands((s) => (s.includes(no) ? s : [...s, no]));
  const removeBand = (no: string) =>
    setSelectedBands((s) => s.filter((x) => x !== no));
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
  // 댓글 대상 글 개수(최신글/인기글)는 댓글 템플릿(writer-modal)에서 정한 값을
  // 그대로 쓴다. 게시 모달에서 다시 고르지 않는다(중복 UI 제거). 없거나 허용값이
  // 아니면 1.
  const commentCount = [1, 3, 5, 10].includes(doc?.commentCount ?? 0)
    ? doc!.commentCount!
    : 1;
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
      // 게시 가능한 계정만 선택 대상이다. 기존 선택에서 로그인 실패 계열을 걸러내고
      // (모달 진입 시 실패 계정이 체크된 채 게시 위치가 파생되는 것을 막는다), 남은 게
      // 없으면 첫 게시 가능 계정 하나를 기본 선택한다.
      const postable = a.filter((x) => isPostable(x.status));
      const postableIds = new Set(postable.map((x) => x.id));
      setSelected((s) => {
        const kept = s.filter((id) => postableIds.has(id));
        if (kept.length) return kept;
        return postable[0] ? [postable[0].id] : [];
      });
    });
    void ipc.stocks.list().then(setStocks);
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
  // Three comment targets are wired: a pasted article URL (`url`), and the cafe's
  // latest/popular lists (`latest`/`popular`) fetched via list_cafe_articles —
  // the top-N (`commentCount`) of that list become the targets at publish time.
  const isListTarget =
    commentTargetMode === "latest" || commentTargetMode === "popular";
  const urlTarget =
    commentTargetMode === "url" ? parseCafeArticleUrl(doc.commentUrl) : null;
  const toggle = (id: string) => {
    // 게시 불가 계정(로그인 실패 계열)은 선택에 넣지 않는다 — 방어선(클릭은 disabled로
    // 이미 막히지만, 어떤 경로로도 실패 계정이 selected에 들어오지 못하게 한다).
    const acc = accounts.find((x) => x.id === id);
    if (acc && !isPostable(acc.status)) return;
    setSelected((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  };
  // 게시 위치·잡 산출은 게시 가능한 선택 계정만 본다. selected는 위에서 정화되지만,
  // 파생 지점에서도 한 번 더 걸러 실패 계정이 게시 위치에 절대 새어 나오지 않게 한다.
  const usableSelected = selected.filter((id) => {
    const acc = accounts.find((x) => x.id === id);
    return !!acc && isPostable(acc.status);
  });
  const selPlatforms = acctPlatforms(usableSelected, accounts);
  const selectedNaver = usableSelected
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
    .filter((a) => isPostable(a.status))
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
  usableSelected.forEach((aid) => {
    const a = accounts.find((x) => x.id === aid);
    if (!a) return;
    if (a.platform === "forum") {
      stockCodes.forEach((code) =>
        jobs.push({
          key: aid + "-" + code,
          platform: "forum",
          loginId: a.loginId,
          targetName:
            stockNames[code] ??
            stocks.find((x) => x.code === code)?.name ??
            code,
          code,
          board: "종목토론방",
          status: a.status,
        }),
      );
    } else if (a.platform === "naver") {
      if (mode === "comment") {
        // Comment-only: the target is the URL / latest / popular, not a board
        // pick. One job per selected account. latest/popular still need the
        // account's picked cafe (the list is fetched from it), so skip accounts
        // that haven't chosen a cafe yet — same silent-partial guard as posts.
        if (isListTarget && !naverPicks[aid]) return;
        const targetName =
          commentTargetMode === "url"
            ? urlTarget
              ? `게시글 #${urlTarget.articleId}`
              : "URL 미설정"
            : commentTargetMode === "popular"
              ? `인기글 ${commentCount}건`
              : `최신글 ${commentCount}건`;
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
      // 선택한 각 밴드마다 잡 1개(계정 × 밴드). 라벨은 조회된 실제 밴드명.
      selectedBands.forEach((no) => {
        const b = resolvedBands.find((x) => x.bandNo === no);
        if (!b) return;
        jobs.push({
          key: `${aid}-${no}`,
          platform: "band",
          loginId: a.loginId,
          targetName: b.name,
          // 가입 링크를 잡에 동결한다(밴드명이 같은 다른 밴드와의 오조회 방지).
          bandLink: b.link,
          board: "전체글",
          status: a.status,
        });
      });
    }
  });
  const targetsOk = !selPlatforms.includes("forum") || stockCodes.length > 0;
  // Comment-only mode needs comments and a resolved target. `url` resolves to a
  // single article; latest/popular resolve to a cafe whose list is fetched at
  // publish time, so they're ready once every selected naver account has picked
  // a cafe (the list source) — top-N extraction handles short lists gracefully.
  const listTargetReady =
    selectedNaver.length > 0 && selectedNaver.every((a) => !!naverPicks[a.id]);
  const commentReady =
    mode !== "comment" ||
    (comments.length > 0 &&
      (isListTarget ? listTargetReady : urlTarget !== null));
  // 게시판이 아직 안 정해진 네이버 계정은 job 생성에서 빠진다. 이들이 있으면 게시를
  // 막아 "일부만 올라가고 나머지는 결과에도 안 뜨는" 조용한 부분 게시를 방지한다.
  const naverNotReady = unreadyNaverAccountIds(
    usableSelected,
    accounts,
    naverPicks,
    mode,
  );
  // 밴드가 선택됐으면 게시할 밴드를 1개 이상 골라야 게시 가능(사수 요구 흐름).
  // 예약(schedule)도 이제 밴드를 plan에 싣으므로 면제하지 않는다 — 즉시·예약 공통으로
  // 밴드 플랫폼이 선택됐다면 최소 1개의 밴드를 골라야 게시할 수 있다.
  const bandReady = !selPlatforms.includes("band") || selectedBands.length > 0;
  const canPublish =
    usableSelected.length > 0 &&
    targetsOk &&
    commentReady &&
    naverNotReady.length === 0 &&
    bandReady &&
    jobs.length > 0;

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
    if (posted.length === 0) return postResults;
    // both = "위에서 작성한 글에 바로 댓글이 달립니다": 쓴 글마다 댓글 풀 전체를 단다.
    // 각 글을 댓글 수만큼 복제해 보내면, 백엔드 분배(셔플 후 pool[i % pool.len()])가
    // 글 블록(길이 = 풀 크기)마다 풀 전체를 정확히 한 번씩 깔아 준다.
    const targets = posted.flatMap((p) => comments.map(() => p));
    const couts = await ipc.cafes
      .runCommentJobs({ targets, comments })
      .catch((): null => null);
    // 글이 올라간 행이라도 그 계정 댓글이 전부 성공해야 "성공"으로 둔다. 일부/전부
    // 실패를 초록 배지로 묻으면(이전 동작) 운영자가 재시도를 안 한다. 건수는 msg에.
    return postResults.map((r) =>
      r.ok
        ? {
            ...r,
            ok: commentsAllOk(couts, r.loginId),
            msg: `${r.msg} · ${commentSummary(couts, r.loginId)}`,
          }
        : r,
    );
  };

  // comment-only: build the comment jobs from the chosen target, then run them.
  //  • url            → the single parsed article, shared by every account.
  //  • latest/popular → each account's picked cafe is queried for its
  //                     latest/popular list, and the top-N (commentCount)
  //                     articles become that account's targets (fewer than N →
  //                     only what the list returned).
  const runNaverComments = async (
    naverJobs: PublishJob[],
  ): Promise<PublishResult[]> => {
    if (!naverJobs.length) return [];
    if (comments.length === 0 || (!isListTarget && !urlTarget)) {
      return naverJobs.map((j) => ({
        ...j,
        ok: false,
        msg: "댓글 대상 또는 댓글 내용이 없어요",
      }));
    }
    // Resolve the comment targets, then let the backend shuffle `comments` and
    // deal one per target (issue #98). url → the single parsed article shared by
    // every account; latest/popular → each account's cafe list, top-N as targets.
    let targets: { accountId: string; cafeId: number; articleId: number }[];
    if (isListTarget) {
      const sortBy = commentTargetMode === "popular" ? "popular" : "latest";
      // Per account: fetch its cafe's list and take the top-N. A failed/empty
      // fetch yields no targets for that account (it then reads as "댓글 없음").
      const perAccount = await Promise.all(
        naverJobs.map(async (j) => {
          const cafeId = naverPicks[j.key]?.cafeId;
          if (!cafeId) return [];
          // Surface a fetch failure as a toast so it's distinguishable from a
          // cafe that genuinely has no articles — both otherwise read as the
          // benign "댓글 없음" row, hiding session/network errors.
          const list = await ipc.cafes
            .listArticles(cafeId, sortBy, j.loginId)
            .catch((err) => {
              notifications.show({
                message: `${j.loginId} ${
                  sortBy === "popular" ? "인기글" : "최신글"
                } 목록을 불러오지 못했어요: ${errText(err)}`,
                color: "red",
              });
              return null;
            });
          if (!list) return [];
          return topNArticles(list.articles, commentCount).map((a) => ({
            accountId: j.loginId,
            cafeId,
            articleId: a.articleId,
          }));
        }),
      );
      targets = perAccount.flat();
    } else {
      targets = naverJobs.map((j) => ({
        accountId: j.loginId,
        cafeId: urlTarget!.cafeId,
        articleId: urlTarget!.articleId,
      }));
    }

    const couts = await ipc.cafes
      .runCommentJobs({ targets, comments })
      .catch((): null => null);
    return naverJobs.map((j) => ({
      ...j,
      // 한 계정의 댓글이 여러 건이면 모두 성공해야 성공으로 본다 — 일부만 올라간
      // 경우(예: 2건 중 1건)를 성공 배지로 묻지 않는다. 자세한 건수는 msg에 표시.
      ok: commentsAllOk(couts, j.loginId),
      msg: commentSummary(couts, j.loginId),
    }));
  };

  // Publish now: naver cafe(글/댓글)·종목토론방(forum)·밴드(band)는 실제 백엔드를
  // 호출하고, 그 외 플랫폼(현재 없음)은 엔진이 없어 시뮬레이션으로 표시한다.
  const runNow = () => {
    setFlow("running");
    // 플랫폼별로 갈래를 나눈다: 네이버 카페·종목토론방·밴드는 실제 백엔드,
    // 그 외 나머지는 엔진 미구현이라 시뮬레이션(후속 작업).
    const naverJobs = jobs.filter((j) => j.platform === "naver");
    const forumJobs = jobs.filter((j) => j.platform === "forum");
    const bandJobs = jobs.filter((j) => j.platform === "band");
    const otherJobs = jobs.filter(
      (j) =>
        j.platform !== "naver" &&
        j.platform !== "forum" &&
        j.platform !== "band",
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

    // 밴드(band.us): 저장된 링크로 가입 후 글(+댓글) 게시 — 순수 HTTP 백엔드 호출.
    // 댓글 모드(both/comment)면 비어있지 않은 댓글을 모두 같은 글에 단다(카페 both와 동일).
    // 댓글 풀은 카페와 동일하게 위에서 만든 `comments`를 재사용한다.
    const bandComments = mode === "both" || mode === "comment" ? comments : [];
    const bandWork: Promise<PublishResult[]> = Promise.all(
      bandJobs.map((j) => {
        // 잡 생성 시 동결한 가입 링크를 쓴다(밴드명 재조회 없이 정확한 밴드).
        const link = j.bandLink ?? "";
        return ipc.band
          .publish({
            // 백엔드는 loginId(쿠키 파일 키)로 band 로그인 쿠키를 찾는다.
            accountId: j.loginId,
            bandLink: link,
            title: doc.title,
            content: htmlToText(doc.body ?? ""),
            comments: bandComments,
          })
          .then((out) => ({
            ...j,
            // 결과 라벨을 게시 응답의 실제 밴드명으로(없으면 잡의 밴드명 유지).
            targetName: out.bandName ?? j.targetName,
            // 댓글을 의도했으면 전부 성공해야 ok(카페 commentsAllOk와 동일 정책).
            // 부분/전량 실패는 초록 배지로 묻지 않는다.
            ok:
              out.commentTotal === 0 || out.commentedCount === out.commentTotal,
            msg:
              out.commentTotal === 0
                ? "글 게시 완료"
                : `글·댓글 ${out.commentedCount}/${out.commentTotal}개 게시 완료`,
          }))
          .catch((err: unknown) => ({ ...j, ok: false, msg: errText(err) }));
      }),
    );

    // 그 외 플랫폼(현재 없음): 진행 UI가 보이도록 지연 후 시뮬레이션 결과를 낸다.
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

    void Promise.all([naverWork, forumWork, bandWork, mockOthers]).then(
      ([nr, fr, br, or]) => {
        setFlow([...nr, ...fr, ...br, ...or]);
        // 밴드 게시 결과를 알림(게시 배치)에 기록한다 — 종토방(forum)이 백엔드에서
        // 배치를 남기는 것과 동일하게, 밴드는 프론트가 결과를 모아 한 번 기록한다.
        // (실패해도 게시 흐름엔 영향 없도록 best-effort.)
        if (br.length > 0) {
          void ipc.band
            .recordBatch({
              title: doc.title,
              body: htmlToText(doc.body ?? ""),
              // 로그 스냅샷은 대표로 첫 댓글만 남긴다(실제 게시는 위에서 전체 전달).
              comment: bandComments[0] ?? "",
              runPost: mode === "post" || mode === "both",
              runComment: mode === "comment" || mode === "both",
              items: br.map((r) => ({
                target: r.targetName,
                loginId: r.loginId,
                ok: r.ok,
                msg: r.msg,
              })),
            })
            .catch(() => {});
        }
      },
    );
  };

  // 예약 시점에 동결할 댓글 대상 스펙. comment/both 모드일 때만 만든다. url이면
  // 파싱된 cafeId/articleId를, latest/popular면 계정이 고른 cafeId + 상위 N(count)을
  // 박제한다(실제 글 목록 해석은 워커가 게시 시점에 수행). post 전용이면 undefined.
  const commentSpecFor = (j: PublishJob): CommentTargetSpec | undefined => {
    if (mode !== "comment" && mode !== "both") return undefined;
    if (commentTargetMode === "url") {
      if (!urlTarget) return undefined;
      return {
        mode: "url",
        cafeId: urlTarget.cafeId,
        articleId: urlTarget.articleId,
      };
    }
    const cafeId = naverPicks[j.key]?.cafeId;
    if (cafeId == null) return undefined;
    return { mode: commentTargetMode, count: commentCount, cafeId };
  };

  // 예약 plan(동결 실행 페이로드): 본문은 모달이 이미 평문화한 값을 박제하고,
  // 엔진이 있는 naver/forum/band 대상을 모두 싣는다. naver의 cafe/menuId/
  // boardType은 toPostJob과 동일하게 naverPicks에서 구하고, band 링크는
  // 즉시 게시(runNow)와 동일하게 resolvedBands에서 밴드명으로 찾는다.
  const buildPlan = (): PublishPlan => {
    const naver: NaverTarget[] = jobs
      .filter((j) => j.platform === "naver")
      .map((j) => {
        const pj = toPostJob(j);
        const spec = commentSpecFor(j);
        return {
          accountId: pj.accountId,
          cafe: pj.cafe,
          // 완료 로그에 카페 ID 대신 보여줄 표시 이름(동결). pick이 없으면 대상명으로.
          cafeName: naverPicks[j.key]?.cafeName ?? j.targetName,
          menuId: pj.menuId,
          boardType: pj.boardType,
          ...(spec ? { commentTarget: spec } : {}),
        };
      });
    const forum: ForumTarget[] = jobs
      .filter((j) => j.platform === "forum")
      .map((j) => ({
        accountId: j.loginId,
        name: j.targetName,
        code: j.code ?? "",
      }));
    const band: BandTarget[] = jobs
      .filter((j) => j.platform === "band")
      .map((j) => ({
        accountId: j.loginId,
        name: j.targetName,
        // 잡 생성 시 동결한 링크를 그대로 싣는다(밴드명 재조회 없음).
        link: j.bandLink ?? "",
      }));
    return {
      postId: doc.id,
      kind: doc.kind,
      title: doc.title,
      bodyText: htmlToText(doc.body ?? ""),
      comments: doc.comments ?? [],
      naver,
      forum,
      band,
    };
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
      at: toEpochMs(date, time),
      missed: false,
      locs,
      plan: buildPlan(),
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
              stockNames={stockNames}
              openStockModal={() => setStockModal(true)}
              removeStock={(c) =>
                setStockCodes((s) => s.filter((x) => x !== c))
              }
              bandLink={bandLink}
              setBandLink={setBandLink}
              bandResolving={bandResolving}
              resolvedBands={resolvedBands}
              selectedBands={selectedBands}
              onSaveBandLink={saveBandLink}
              onSelectBand={selectBand}
              onRemoveBand={removeBand}
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
        {naverNotReady.length > 0 && (
          <Text fz={12} c="orange.7">
            게시판 미설정 계정 {naverNotReady.length}개 — 게시판을 선택해야
            게시할 수 있어요
          </Text>
        )}
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
          setStockNames((m) => ({
            ...m,
            ...Object.fromEntries(stocks.map((s) => [s.code, s.name])),
          }));
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
