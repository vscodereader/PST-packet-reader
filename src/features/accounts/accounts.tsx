import {
  ActionIcon,
  Alert,
  Badge,
  Box,
  Button,
  Checkbox,
  Container,
  Group,
  Pagination,
  Select,
  Table,
  Text,
  TextInput,
  Title,
  UnstyledButton,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  STATUS_ACCOUNT,
  STATUS_ACCOUNT_CYCLE,
  STATUS_GUIDE,
} from "@/shared/data/config";
import { acctPlatforms } from "@/shared/data/helpers";
import type {
  Account,
  AccountStatus,
  GoFn,
  PlatformId,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

import { buildLoginNowItem, isSelectiveLoginPlatform } from "./login-queue";

const PER_PAGE = 10;
const PLATFORM_OPTIONS = [
  { value: "forum", label: "종목토론방" },
  { value: "naver", label: "네이버 카페" },
  { value: "blog", label: "네이버블로그" },
  { value: "clip", label: "네이버 클립" },
  { value: "band", label: "밴드" },
];

function toast(message: string, color = "blue") {
  notifications.show({ message, color, autoClose: 2400 });
}

function EditableCell({
  value,
  onSave,
  mono,
  placeholder,
}: {
  value: string;
  onSave: (v: string) => void;
  mono?: boolean;
  placeholder?: string;
}) {
  const [editing, setEditing] = useState(false);
  const [v, setV] = useState(value);
  const startEdit = () => {
    setV(value);
    setEditing(true);
  };

  if (editing) {
    return (
      <TextInput
        autoFocus
        size="xs"
        variant="filled"
        value={v}
        placeholder={placeholder}
        onChange={(e) => setV(e.currentTarget.value)}
        onBlur={() => {
          setEditing(false);
          onSave(v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            setEditing(false);
            onSave(v);
          }
          if (e.key === "Escape") {
            setV(value);
            setEditing(false);
          }
        }}
        {...(mono ? { styles: { input: { fontFamily: "monospace" } } } : {})}
      />
    );
  }
  return (
    <UnstyledButton
      onClick={startEdit}
      title="클릭하여 편집"
      style={{
        display: "block",
        width: "100%",
        fontSize: 13.5,
        fontFamily: mono ? "monospace" : undefined,
        color: value
          ? "var(--mantine-color-gray-7)"
          : "var(--mantine-color-gray-5)",
      }}
    >
      {value || placeholder}
    </UnstyledButton>
  );
}

function PwCell({
  value,
  onSave,
}: {
  value: string;
  onSave: (v: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [show, setShow] = useState(false);
  const [v, setV] = useState(value);
  const startEdit = () => {
    setV(value);
    setEditing(true);
  };

  if (editing) {
    return (
      <TextInput
        autoFocus
        size="xs"
        variant="filled"
        type={show ? "text" : "password"}
        value={v}
        onChange={(e) => setV(e.currentTarget.value)}
        onBlur={() => {
          setEditing(false);
          onSave(v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            setEditing(false);
            onSave(v);
          }
          if (e.key === "Escape") {
            setV(value);
            setEditing(false);
          }
        }}
        styles={{ input: { fontFamily: "monospace" } }}
      />
    );
  }
  return (
    <Group gap={4} wrap="nowrap">
      <UnstyledButton
        onClick={startEdit}
        title="클릭하여 편집"
        style={{
          flex: 1,
          display: "block",
          fontSize: 13.5,
          fontFamily: "monospace",
          letterSpacing: value && !show ? 2 : 0,
          color: value
            ? "var(--mantine-color-gray-7)"
            : "var(--mantine-color-gray-5)",
          overflow: "hidden",
        }}
      >
        {value
          ? show
            ? value
            : "•".repeat(Math.min(value.length, 10))
          : "비밀번호"}
      </UnstyledButton>
      <ActionIcon
        variant="subtle"
        color="gray"
        size="sm"
        onClick={() => setShow((s) => !s)}
        title={show ? "숨기기" : "보기"}
      >
        {show ? <Icon.eyeOff size={15} /> : <Icon.eye size={15} />}
      </ActionIcon>
    </Group>
  );
}

function StatusBadge({
  value,
  statusMsg,
  onChange,
}: {
  value: AccountStatus;
  statusMsg?: string | undefined;
  onChange: (v: AccountStatus) => void;
}) {
  const st = STATUS_ACCOUNT[value] ?? { t: value, c: "gray" };
  // tooltip: 상태별 조치 안내 + (있으면) 백엔드가 남긴 상세 사유.
  const guide = STATUS_GUIDE[value] ?? "클릭하여 상태 변경";
  const tip = statusMsg ? `${guide}\n${statusMsg}` : guide;
  // 클릭 순환은 사용자 의미 상태(STATUS_ACCOUNT_CYCLE)만 돈다. 현재 값이 cycle 밖(로그인
  // 워커가 자동 설정한 badCredentials/challenge/error)이면 첫 값으로 보낸다.
  const cycle = () => {
    // 대기(글 게시 성공 후, #267-3)는 클릭하면 곧장 활성으로 되돌린다 — 다시 게시에 쓸 수 있게.
    if (value === "waiting") {
      onChange("active");
      return;
    }
    const i = STATUS_ACCOUNT_CYCLE.indexOf(value);
    const next =
      i === -1
        ? STATUS_ACCOUNT_CYCLE[0]
        : STATUS_ACCOUNT_CYCLE[(i + 1) % STATUS_ACCOUNT_CYCLE.length];
    if (next) onChange(next as AccountStatus);
  };
  return (
    <Badge
      size="sm"
      color={st.c}
      variant="light"
      style={{ cursor: "pointer", whiteSpace: "pre-line" }}
      title={tip}
      onClick={cycle}
    >
      {st.t}
    </Badge>
  );
}

/**
 * 로그인 쿠키 만료까지 남은 시간을 "3일 12:04:07 남음"처럼 포맷한다(순수 함수).
 * `expiresAt`(unix seconds)이 null/undefined면 "—"(로그인 이력/실만료 없음),
 * 이미 지났으면 "만료됨".
 */
export function formatCookieCountdown(
  expiresAt: number | null | undefined,
  nowSec: number,
): string {
  if (expiresAt == null) return "—";
  const remain = Math.floor(expiresAt - nowSec);
  if (remain <= 0) return "만료됨";
  const days = Math.floor(remain / 86400);
  const h = Math.floor((remain % 86400) / 3600);
  const m = Math.floor((remain % 3600) / 60);
  const s = remain % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  const hms = `${pad(h)}:${pad(m)}:${pad(s)}`;
  return days > 0 ? `${days}일 ${hms} 남음` : `${hms} 남음`;
}

/** 계정관리 "쿠키만료" 열: 로그인 쿠키 만료까지 1초 간격으로 갱신되는 카운트다운. */
function CookieExpiryCell({
  expiresAt,
  nowSec,
}: {
  expiresAt: number | null | undefined;
  nowSec: number;
}) {
  const text = formatCookieCountdown(expiresAt, nowSec);
  const expired = text === "만료됨";
  const none = text === "—";
  const color = expired ? "red" : none ? "dimmed" : undefined;
  return (
    <Text
      size="xs"
      ff="monospace"
      {...(color ? { c: color } : {})}
      title={
        expiresAt == null
          ? "저장된 로그인 쿠키 없음(또는 세션 쿠키)"
          : "로그인 쿠키 만료까지 남은 시간"
      }
    >
      {text}
    </Text>
  );
}

export function Accounts({ go }: { go: GoFn }) {
  const [rows, setRows] = useState<Account[]>([]);

  useEffect(() => {
    void ipc.accounts.list().then(setRows);
  }, []);
  const [filter, setFilter] = useState<"all" | PlatformId>("all");
  const [tagFilter, setTagFilter] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<string[]>([]);
  const [page, setPage] = useState(1);
  const [loggingIn, setLoggingIn] = useState(false);
  const [rotatingIp, setRotatingIp] = useState(false);
  const [manualAdding, setManualAdding] = useState(false);
  const loginPollRef = useRef<number | null>(null);
  // "쿠키만료" 카운트다운을 1초마다 다시 그리기 위한 현재 시각(unix seconds).
  const [nowSec, setNowSec] = useState(() => Math.floor(Date.now() / 1000));
  // 계정(loginId)별 로그인 쿠키 만료 시각(unix seconds). null = 세션/이력 없음.
  const [expiries, setExpiries] = useState<Record<string, number | null>>({});
  // 우리 임시 프로필로 아직 도는 Chrome 개수(작업관리자 없이 앱에서 확인, 사수 요청).
  const [chromeCount, setChromeCount] = useState(0);

  // 화면을 떠날 때 로그인 상태 폴링 타이머를 정리한다.
  useEffect(() => {
    return () => {
      if (loginPollRef.current !== null)
        window.clearInterval(loginPollRef.current);
    };
  }, []);

  // 카운트다운용 시계: 1초마다 현재 시각을 갱신해 "쿠키만료" 셀이 실시간으로 줄어든다.
  useEffect(() => {
    const id = window.setInterval(
      () => setNowSec(Math.floor(Date.now() / 1000)),
      1000,
    );
    return () => window.clearInterval(id);
  }, []);

  // 잔존 Chrome 개수를 2초마다 폴링한다(백엔드 running_chrome_count, best-effort).
  useEffect(() => {
    let alive = true;
    const poll = () => {
      ipc.system
        .runningChromeCount()
        .then((n) => {
          if (alive) setChromeCount(n);
        })
        .catch(() => {});
    };
    poll();
    const id = window.setInterval(poll, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  // 계정 목록이 바뀌면 각 계정의 쿠키 만료 시각을 조회해 카운트다운의 기준값으로 쓴다.
  // 만료 시각은 재로그인 때만 바뀌므로 매초가 아니라 목록 변경 시에만 다시 읽는다.
  const loginIdsKey = rows.map((r) => r.loginId).join(" ");
  useEffect(() => {
    let alive = true;
    const ids = rows.map((r) => r.loginId).filter((id) => id.trim());
    Promise.all(
      ids.map((id) =>
        ipc.accounts
          .cookieExpiry(id)
          .then((exp) => [id, exp] as const)
          .catch(() => [id, null] as const),
      ),
    ).then((pairs) => {
      if (alive) setExpiries(Object.fromEntries(pairs));
    });
    return () => {
      alive = false;
    };
    // rows 자체가 아니라 loginId 목록이 바뀔 때만 다시 조회한다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loginIdsKey]);

  // Optimistically patch the row for snappy editing, then persist over IPC and
  // reconcile with the authoritative list the backend returns.
  const update = (id: string, patch: Partial<Account>) => {
    // 사용자가 status를 직접 바꾸면 워커가 남긴 사유(statusMsg)는 더 이상 유효하지 않으므로
    // 키를 제거한다 — 배지는 active인데 tooltip엔 옛 차단 사유가 남는 모순을 막는다.
    const merge = (base: Account): Account => {
      const next = { ...base, ...patch };
      if ("status" in patch) delete next.statusMsg;
      return next;
    };
    const cur = rows.find((r) => r.id === id);
    setRows((rs) => rs.map((r) => (r.id === id ? merge(r) : r)));
    if (cur) void ipc.accounts.update(merge(cur)).then(setRows);
  };

  const addRow = () => {
    const account: Account = {
      id: "n" + Date.now(),
      platform: "forum",
      loginId: "",
      pw: "",
      status: "new",
      last: "—",
      tags: [],
    };
    void ipc.accounts.add(account).then(setRows);
    toast("새 계정 행을 추가했어요", "green");
  };
  const removeSel = () => {
    void ipc.accounts.remove(sel).then(setRows);
    toast(`${sel.length}개 계정을 삭제했어요`);
    setSel([]);
  };

  // 선택한 계정을 즉시 처리 대기열(now 큐)에 로그인 작업으로 적재한다(#210). 로그인도
  // 게시와 같은 큐에서 처리되며, 성공/실패는 각 계정 status 배지와 알림 로그(자세히 보기의
  // 백트레이스 포함)에 반영된다. 종토방·블로그 모두 buildLoginNowItem이 naver 로그인으로 묶는다.
  const runLogin = async () => {
    const targets = rows.filter(
      (r) => sel.includes(r.id) && r.loginId.trim() && r.pw,
    );
    if (targets.length === 0) {
      toast("로그인할 계정을 선택하고 아이디·비밀번호를 채워주세요", "red");
      return;
    }
    setLoggingIn(true);
    try {
      // 자격증명을 accounts.json(id=loginId)에 저장한다(id 기준 병합).
      await ipc.auth.bootstrap();
      await ipc.auth.saveAccounts(
        targets.map((t) => ({
          id: t.loginId,
          password: t.pw,
          label: t.loginId,
        })),
      );
      // 로그인 배치 1개 = now 큐 아이템 1개. 워커가 계정별로 종토방(네이버) 로그인을 처리한다.
      const item = buildLoginNowItem(targets, crypto.randomUUID());
      await ipc.queue.addNow(item);
      toast(
        `${targets.length}개 계정 로그인을 큐에 추가했어요 — 진행 상황은 큐에서 확인하세요`,
        "green",
      );
      pollLoginCompletion(item.id);
    } catch (err) {
      setLoggingIn(false);
      toast(err instanceof Error ? err.message : String(err), "red");
      ipc.activity
        .append(
          "error",
          "로그인 시작 실패 — " +
            (err instanceof Error ? err.message : String(err)),
        )
        .catch(() => {});
    }
  };

  // now 큐를 폴링해 로그인 배치(itemId)가 큐에서 사라질 때까지 계정 리스트를 갱신한다.
  // 워커가 1계정 처리할 때마다 accounts store에 상태를 기록하므로 배지가 실시간으로 갱신되고,
  // 아이템이 큐에서 제거되면(완료) 폴링을 멈춘다. 큐 진행률은 큐 화면이 별도로 보여준다.
  const pollLoginCompletion = (itemId: string) => {
    if (loginPollRef.current !== null)
      window.clearInterval(loginPollRef.current);
    loginPollRef.current = window.setInterval(() => {
      void Promise.all([ipc.queue.listNow(), ipc.accounts.list()])
        .then(([queue, accounts]) => {
          // 워커가 계정별로 기록한 최신 상태를 배지·tooltip에 반영한다.
          setRows(accounts);
          if (queue.some((q) => q.id === itemId)) return;
          // 로그인 아이템이 큐에서 사라짐 = 배치 완료.
          if (loginPollRef.current !== null) {
            window.clearInterval(loginPollRef.current);
            loginPollRef.current = null;
          }
          setLoggingIn(false);
          toast(
            "계정 로그인이 끝났어요 — 상태 배지와 알림 로그에서 결과를 확인하세요",
            "green",
          );
        })
        .catch((err) => {
          if (loginPollRef.current !== null) {
            window.clearInterval(loginPollRef.current);
            loginPollRef.current = null;
          }
          setLoggingIn(false);
          toast(
            "로그인 상태 확인 중 오류가 발생했어요 — " +
              (err instanceof Error ? err.message : String(err)),
            "red",
          );
        });
    }, 1000);
  };

  // 선택 목록에서 더 이상 행에 존재하지 않는 id(삭제·재적재로 사라진 유령)를 걸러낸 실제
  // 선택분. "선택 로그인 (N)" 카운트가 화면에 보이는 체크박스 수보다 커지던 문제를 막는다.
  const selPresent = sel.filter((id) => rows.some((r) => r.id === id));

  // 선택 계정이 전부 선택 로그인 지원 플랫폼(종목토론방·네이버블로그)일 때만 선택 로그인을
  // 허용한다(#228 + 블로그 추가). 둘 다 네이버 쿠키 기반이라 buildLoginNowItem이 동일하게
  // naver 로그인으로 묶는다. 카페·밴드는 게시 직전 백엔드가 [회전→로그인→게시]를 원자 처리하므로
  // 별도 로그인이 불필요하다 — 하나라도 섞이면 버튼을 숨긴다.
  const loginEligible =
    selPresent.length > 0 &&
    acctPlatforms(selPresent, rows).every(isSelectiveLoginPlatform);

  const allTags = useMemo(
    () => [...new Set([...rows.flatMap((r) => r.tags)])],
    [rows],
  );
  const view = rows.filter(
    (r) =>
      (filter === "all" || r.platform === filter) &&
      (!tagFilter || r.tags.includes(tagFilter)) &&
      (!q || r.loginId.includes(q) || r.tags.some((t) => t.includes(q))),
  );
  const totalPages = Math.max(1, Math.ceil(view.length / PER_PAGE));
  const curPage = Math.min(page, totalPages);
  const pageItems = view.slice((curPage - 1) * PER_PAGE, curPage * PER_PAGE);
  const allSel =
    pageItems.length > 0 && pageItems.every((r) => sel.includes(r.id));

  const chips: { value: "all" | PlatformId; label: string; count: number }[] = [
    { value: "all", label: "전체", count: rows.length },
    {
      value: "forum",
      label: "종목토론방",
      count: rows.filter((r) => r.platform === "forum").length,
    },
    {
      value: "naver",
      label: "네이버 카페",
      count: rows.filter((r) => r.platform === "naver").length,
    },
    {
      value: "blog",
      label: "네이버블로그",
      count: rows.filter((r) => r.platform === "blog").length,
    },
    {
      value: "clip",
      label: "네이버 클립",
      count: rows.filter((r) => r.platform === "clip").length,
    },
    {
      value: "band",
      label: "밴드",
      count: rows.filter((r) => r.platform === "band").length,
    },
  ];

  const activeCount = rows.filter((r) => r.status === "active").length;
  const newCount = rows.filter((r) => r.status === "new").length;
  const errCount = rows.filter((r) => r.status === "error").length;

  return (
    <Container size={1080} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={20} wrap="wrap">
        <Box>
          <Title order={1} fz={25} fw={800}>
            계정 관리
          </Title>
          <Text size="sm" c="dimmed" mt={6}>
            로그인 계정을 엑셀처럼 관리하세요. 태그로 같은 로그인·주제끼리 묶을
            수 있어요.
          </Text>
        </Box>
        <Group gap="xs">
          {/* 잔존 Chrome 지표 — 작업관리자 없이 남은(고아) 크롬을 앱에서 바로 확인(사수 요청). */}
          <Badge
            size="lg"
            variant="light"
            color={chromeCount > 0 ? "orange" : "gray"}
            leftSection={<Icon.bolt size={13} />}
            title="우리가 띄운 임시 프로필로 아직 실행 중인 크롬 프로세스(자식 헬퍼 포함) 개수"
          >
            실행 중 크롬 {chromeCount}개
          </Badge>
          {loginEligible && (
            <Button
              size="sm"
              variant="light"
              color="green"
              loading={loggingIn}
              leftSection={<Icon.bolt size={16} />}
              onClick={() => void runLogin()}
            >
              선택 로그인 ({selPresent.length})
            </Button>
          )}
          <Button
            size="sm"
            variant="default"
            leftSection={<Icon.inbox size={16} />}
            onClick={async () => {
              const path = await open({
                multiple: false,
                filters: [{ name: "Excel", extensions: ["xlsx"] }],
              });
              if (typeof path !== "string") return;
              try {
                const summary = await ipc.excel.importAccounts(path);
                setRows(await ipc.accounts.list());
                toast(
                  `${summary.imported}건 가져옴${summary.skipped ? `, ${summary.skipped}건 건너뜀` : ""}`,
                  "green",
                );
              } catch (err) {
                toast(
                  "가져오기 실패: " +
                    (err instanceof Error ? err.message : String(err)),
                  "red",
                );
                ipc.activity
                  .append(
                    "error",
                    "계정 가져오기 실패 — " +
                      (err instanceof Error ? err.message : String(err)),
                  )
                  .catch(() => {});
              }
            }}
          >
            엑셀 가져오기
          </Button>
          <Button
            size="sm"
            variant="default"
            leftSection={<Icon.download size={16} />}
            onClick={async () => {
              const path = await save({
                defaultPath: "계정.xlsx",
                filters: [{ name: "Excel", extensions: ["xlsx"] }],
              });
              if (!path) return;
              try {
                await ipc.excel.exportAccounts(path);
                toast("현재 계정 목록을 엑셀로 내보냈어요", "green");
              } catch (err) {
                toast(
                  "내보내기 실패: " +
                    (err instanceof Error ? err.message : String(err)),
                  "red",
                );
                ipc.activity
                  .append(
                    "error",
                    "계정 내보내기 실패 — " +
                      (err instanceof Error ? err.message : String(err)),
                  )
                  .catch(() => {});
              }
            }}
          >
            내보내기
          </Button>
          <Button
            size="sm"
            variant="default"
            loading={manualAdding}
            leftSection={<Icon.plus size={16} />}
            onClick={async () => {
              // 사람이 직접 로그인(headed Chrome). 성공하면 쿠키는 자동로그인과 동일하게
              // 저장되고 계정 행이 status=Active로 자동 추가된다. 취소/타임아웃은 중립 토스트.
              setManualAdding(true);
              try {
                const acc = await ipc.auth.manualAdd();
                setRows(await ipc.accounts.list());
                toast(`수동추가 완료 — ${acc.loginId}`, "green");
                ipc.activity
                  .append("success", `수동추가 완료 — ${acc.loginId}`)
                  .catch(() => {});
              } catch (err) {
                toast(
                  "수동추가 안 됨 — " +
                    (err instanceof Error ? err.message : String(err)),
                  "orange",
                );
              } finally {
                setManualAdding(false);
              }
            }}
          >
            수동추가
          </Button>
          <Button
            size="sm"
            variant="default"
            loading={rotatingIp}
            leftSection={<Icon.bolt size={16} />}
            onClick={async () => {
              // 로그인·게시 없이 연결된 폰의 비행기모드만 토글해 IP만 회전(#247).
              // 비행기모드 ON/OFF·IP 회전 결과는 기존과 동일하게 로그에 남는다.
              setRotatingIp(true);
              try {
                const r = await ipc.auth.rotateIp();
                const detail = `원래: ${r.before} / 바뀐 IP: ${r.after}`;
                if (r.changed) {
                  toast(`IP 변경됨 — ${detail}`, "green");
                  ipc.activity
                    .append("success", `IP 변경됨 — ${detail}`)
                    .catch(() => {});
                } else {
                  toast(`IP가 그대로예요 — ${detail}`, "orange");
                  ipc.activity
                    .append("info", `IP 변경 안 됨 — ${detail}`)
                    .catch(() => {});
                }
              } catch (err) {
                toast(
                  "IP 변경 실패: " +
                    (err instanceof Error ? err.message : String(err)),
                  "red",
                );
                ipc.activity
                  .append(
                    "error",
                    "IP 변경 실패 — " +
                      (err instanceof Error ? err.message : String(err)),
                  )
                  .catch(() => {});
              } finally {
                setRotatingIp(false);
              }
            }}
          >
            IP 변경
          </Button>
          <Button
            size="sm"
            leftSection={<Icon.plus size={17} />}
            onClick={addRow}
          >
            계정 추가
          </Button>
        </Group>
      </Group>

      <Alert
        mb={18}
        variant="light"
        color="gray"
        styles={{ message: { width: "100%" } }}
      >
        <Group gap="sm" wrap="nowrap">
          <Group gap={6}>
            <PlatformLogo id="instagram" size={26} dim />
            <PlatformLogo id="threads" size={26} dim />
          </Group>
          <Text size="sm" fw={600}>
            인스타그램 · 스레드 연동 준비 중
          </Text>
          <Text size="xs" c="dimmed">
            곧 같은 방식으로 계정을 추가할 수 있어요.
          </Text>
          <Badge size="sm" color="blue" variant="light" ml="auto">
            출시 예정
          </Badge>
        </Group>
      </Alert>

      <Group justify="space-between" mb={14} wrap="wrap">
        <Group gap={6}>
          {chips.map((c) => (
            <Button
              key={c.value}
              size="xs"
              radius="xl"
              variant={c.value === filter ? "filled" : "default"}
              color={c.value === filter ? "dark" : "gray"}
              leftSection={
                c.value !== "all" ? (
                  <PlatformLogo id={c.value} size={16} />
                ) : undefined
              }
              onClick={() => {
                setFilter(c.value);
                setPage(1);
              }}
            >
              {c.label}
              <Text component="span" ml={6} fz={11} opacity={0.7}>
                {c.count}
              </Text>
            </Button>
          ))}
        </Group>
        <Group gap="sm">
          {sel.length > 0 && (
            <Button
              variant="light"
              color="red"
              size="sm"
              leftSection={<Icon.trash size={15} />}
              onClick={removeSel}
            >
              {sel.length}개 삭제
            </Button>
          )}
          <Select
            size="sm"
            w={140}
            data={[
              { value: "__all", label: "모든 태그" },
              ...allTags.map((t) => ({ value: t, label: "# " + t })),
            ]}
            value={tagFilter ?? "__all"}
            onChange={(v) => {
              setTagFilter(v === "__all" ? null : v);
              setPage(1);
            }}
          />
          <TextInput
            size="sm"
            w={200}
            placeholder="계정·태그 검색"
            leftSection={<Icon.search size={16} />}
            value={q}
            onChange={(e) => {
              setQ(e.currentTarget.value);
              setPage(1);
            }}
          />
        </Group>
      </Group>

      <Table.ScrollContainer minWidth={900}>
        <Table
          withTableBorder
          withColumnBorders
          verticalSpacing={8}
          striped="odd"
        >
          <Table.Thead bg="gray.1">
            <Table.Tr>
              <Table.Th w={40}>
                <Checkbox
                  size="xs"
                  checked={allSel}
                  onChange={(e) =>
                    setSel(
                      e.currentTarget.checked ? pageItems.map((r) => r.id) : [],
                    )
                  }
                />
              </Table.Th>
              <Table.Th w={44} ta="center">
                #
              </Table.Th>
              <Table.Th w={140}>플랫폼</Table.Th>
              <Table.Th w={160}>계정 ID</Table.Th>
              <Table.Th w={160}>계정 PW</Table.Th>
              <Table.Th w={200}>쿠키만료</Table.Th>
              <Table.Th w={96} ta="center">
                상태
              </Table.Th>
              <Table.Th w={110} ta="center">
                활동 내역
              </Table.Th>
              <Table.Th w={48} />
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {pageItems.map((r, idx) => (
              <Table.Tr
                key={r.id}
                {...(sel.includes(r.id) ? { bg: "blue.0" } : {})}
              >
                <Table.Td>
                  <Checkbox
                    size="xs"
                    checked={sel.includes(r.id)}
                    onChange={() =>
                      setSel((s) =>
                        s.includes(r.id)
                          ? s.filter((x) => x !== r.id)
                          : [...s, r.id],
                      )
                    }
                  />
                </Table.Td>
                <Table.Td ta="center" c="dimmed" ff="monospace" fz={12.5}>
                  {(curPage - 1) * PER_PAGE + idx + 1}
                </Table.Td>
                <Table.Td>
                  <Select
                    size="xs"
                    variant="unstyled"
                    data={PLATFORM_OPTIONS}
                    value={r.platform}
                    allowDeselect={false}
                    onChange={(v) =>
                      v && update(r.id, { platform: v as PlatformId })
                    }
                  />
                </Table.Td>
                <Table.Td>
                  <EditableCell
                    value={r.loginId}
                    mono
                    placeholder="네이버 아이디"
                    onSave={(v) => update(r.id, { loginId: v })}
                  />
                </Table.Td>
                <Table.Td>
                  <PwCell
                    value={r.pw}
                    onSave={(v) => update(r.id, { pw: v })}
                  />
                </Table.Td>
                <Table.Td>
                  <CookieExpiryCell
                    expiresAt={expiries[r.loginId]}
                    nowSec={nowSec}
                  />
                </Table.Td>
                <Table.Td ta="center">
                  <StatusBadge
                    value={r.status}
                    statusMsg={r.statusMsg}
                    onChange={(v) => update(r.id, { status: v })}
                  />
                </Table.Td>
                <Table.Td ta="center">
                  <Button
                    size="compact-xs"
                    variant="default"
                    radius="xl"
                    leftSection={<Icon.history size={14} />}
                    onClick={() =>
                      go("log", {
                        logFilter: {
                          loginId: r.loginId,
                          platform: r.platform,
                        },
                      })
                    }
                  >
                    보러가기
                  </Button>
                </Table.Td>
                <Table.Td>
                  <ActionIcon
                    variant="subtle"
                    color="gray"
                    size="sm"
                    title="삭제"
                    onClick={() => {
                      void ipc.accounts.remove([r.id]).then(setRows);
                      toast("계정을 삭제했어요");
                    }}
                  >
                    <Icon.trash size={16} />
                  </ActionIcon>
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      </Table.ScrollContainer>

      <Group gap="sm" mt={14} fz={12.5} c="dimmed">
        <Text size="xs">총 {rows.length}개 계정</Text>
        <Group gap={5}>
          <Box w={7} h={7} bg="green" style={{ borderRadius: 999 }} />
          <Text size="xs">활성 {activeCount}</Text>
        </Group>
        <Group gap={5}>
          <Box w={7} h={7} bg="gray.5" style={{ borderRadius: 999 }} />
          <Text size="xs">사용전 {newCount}</Text>
        </Group>
        <Group gap={5}>
          <Box w={7} h={7} bg="red" style={{ borderRadius: 999 }} />
          <Text size="xs">에러 {errCount}</Text>
        </Group>
        <Text size="xs" ml="auto">
          셀을 클릭해 편집 · 상태 배지로 전환
        </Text>
      </Group>

      {view.length > 0 && (
        <Group justify="space-between" mt={16}>
          <Text size="xs" c="dimmed">
            {view.length}개 중 {(curPage - 1) * PER_PAGE + 1}–
            {Math.min(curPage * PER_PAGE, view.length)} · {curPage}/{totalPages}{" "}
            페이지
          </Text>
          <Pagination
            size="sm"
            total={totalPages}
            value={curPage}
            onChange={setPage}
          />
        </Group>
      )}
    </Container>
  );
}
