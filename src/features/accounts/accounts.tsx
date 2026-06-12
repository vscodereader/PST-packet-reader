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
  Popover,
  Select,
  Table,
  TagsInput,
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
import type {
  Account,
  AccountStatus,
  GoFn,
  PlatformId,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

const PER_PAGE = 10;
const PLATFORM_OPTIONS = [
  { value: "forum", label: "종목토론방" },
  { value: "naver", label: "네이버 카페" },
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

function TagCell({
  tags,
  suggestions,
  onChange,
}: {
  tags: string[];
  suggestions: string[];
  onChange: (t: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover
      opened={open}
      onChange={setOpen}
      width={220}
      position="bottom-start"
    >
      <Popover.Target>
        <Group
          gap={4}
          wrap="nowrap"
          style={{ cursor: "pointer", overflow: "hidden" }}
          onClick={() => setOpen(true)}
        >
          {tags.length === 0 ? (
            <Text size="xs" c="dimmed">
              + 태그
            </Text>
          ) : (
            tags.map((t) => (
              <Badge
                key={t}
                size="sm"
                variant="outline"
                color="gray"
                style={{ flexShrink: 0 }}
              >
                {t}
              </Badge>
            ))
          )}
        </Group>
      </Popover.Target>
      <Popover.Dropdown p="xs">
        <TagsInput
          size="xs"
          data={suggestions}
          value={tags}
          onChange={onChange}
          placeholder="태그 추가"
          maxDropdownHeight={130}
        />
      </Popover.Dropdown>
    </Popover>
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
  const loginPollRef = useRef<number | null>(null);

  // 화면을 떠날 때 로그인 상태 폴링 타이머를 정리한다.
  useEffect(() => {
    return () => {
      if (loginPollRef.current !== null)
        window.clearInterval(loginPollRef.current);
    };
  }, []);

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

  // 선택한 계정으로 네이버 로그인 자동화를 실행한다(쿠키 키 = loginId).
  // 성공/실패는 각 계정의 status(active/error)로 표시한다.
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
      // 플랫폼이 밴드인 계정은 네이버가 아니라 band.us로 로그인한다. 종목토론방·
      // 네이버카페는 기존대로 네이버 로그인 큐를 쓴다(기존 동작 무수정).
      const bandTargets = targets.filter((t) => t.platform === "band");
      const naverTargets = targets.filter((t) => t.platform !== "band");

      // 계정 자격증명은 양쪽 큐가 같은 accounts.json(id=loginId)을 읽으므로 한 번만 저장한다.
      // save_accounts는 id 기준 병합이라 두 그룹이 서로를 덮어쓰지 않는다.
      await ipc.auth.bootstrap();
      await ipc.auth.saveAccounts(
        targets.map((t) => ({
          id: t.loginId,
          password: t.pw,
          label: t.loginId,
        })),
      );

      if (naverTargets.length > 0) {
        // 명시적 선택 계정 로그인 → force=true: 서버측에서 죽었지만 로컬 검증만 통과하는
        // 쿠키도 실제 재로그인으로 새로 덮어쓴다(이슈 #132).
        await ipc.auth.enqueueLogin(
          naverTargets.map((t) => t.loginId),
          false,
          true,
        );
      }
      if (bandTargets.length > 0) {
        // 밴드 선택로그인: band.us(CDP)로 로그인. 랜선만 꽂으면 되며 ADB 불필요.
        // force=true: 명시적 재로그인이므로 유효 쿠키여도 실제 로그인해 새 비밀번호를
        // 검증한다(틀린 비번으로 바꾼 뒤 재로그인이 그대로 성공하던 문제 방지, 네이버 #132).
        await ipc.band.login(
          bandTargets.map((t) => t.loginId),
          false,
          false,
          true,
        );
      }
      pollLogin(targets);
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

  // get_queue_status를 2초마다 확인해 각 계정의 로그인 결과를 반영한다.
  const pollLogin = (targets: Account[]) => {
    if (loginPollRef.current !== null)
      window.clearInterval(loginPollRef.current);
    // 행 추적은 고유키 id로 한다(loginId는 유니크가 보장되지 않아 같은 loginId의 두 행이
    // 하나로 합쳐지면 한쪽만 반영된다).
    const remaining = new Set(targets.map((t) => t.id));
    // 밴드 계정은 별도 band 큐(get_band_queue_status)에서 결과를 읽고, 나머지는 기존
    // 네이버 큐(get_queue_status)에서 읽는다. 선택에 포함된 큐만 조회한다.
    const needNaver = targets.some((t) => t.platform !== "band");
    const needBand = targets.some((t) => t.platform === "band");

    loginPollRef.current = window.setInterval(() => {
      void Promise.all([
        needNaver ? ipc.auth.queueStatus() : Promise.resolve(null),
        needBand ? ipc.band.queueStatus() : Promise.resolve(null),
      ])
        .then(([naverStatus, bandStatus]) => {
          let resolvedThisTick = false;
          targets.forEach((t) => {
            if (!remaining.has(t.id)) return;
            // 계정 플랫폼에 맞는 큐 상태에서 잡을 찾는다.
            const status = t.platform === "band" ? bandStatus : naverStatus;
            if (!status) return;
            // 백엔드 잡은 loginId(=쿠키 키)로 식별된다. 같은 loginId를 쓰는 행들은
            // 같은 잡 결과를 각자(id별로) 반영한다.
            const job = [...status.jobs]
              .reverse()
              .find((j) => j.accountId === t.loginId);
            if (!job || job.status === "pending" || job.status === "running")
              return;

            remaining.delete(t.id);
            resolvedThisTick = true;
            const ok = job.status === "success" || job.status === "expired";
            toast(
              `${t.loginId}: ${ok ? "로그인 성공" : "로그인 실패 — " + job.message}`,
              ok ? "green" : "red",
            );
          });

          // 이번 틱에 하나라도 완료됐으면 권위 계정 리스트를 한 번만 재조회해 배지·tooltip에
          // 세밀 상태(active/blocked/challenge/badCredentials)와 사유를 반영한다. 백엔드
          // worker_loop가 큐 상태를 finished로 바꾸기 전에 계정 store를 먼저 기록하므로,
          // 여기서 읽으면 최신 상태가 보인다(틱당 1회 — 행별 중복 list 호출 방지).
          if (resolvedThisTick) void ipc.accounts.list().then(setRows);

          if (remaining.size === 0) {
            if (loginPollRef.current !== null) {
              window.clearInterval(loginPollRef.current);
              loginPollRef.current = null;
            }
            setLoggingIn(false);
          }
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
    }, 2000);
  };

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
          <Button
            size="sm"
            variant="light"
            color="green"
            loading={loggingIn}
            disabled={sel.length === 0}
            leftSection={<Icon.bolt size={16} />}
            onClick={() => void runLogin()}
          >
            선택 로그인 ({sel.length})
          </Button>
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
              <Table.Th w={200}>태그</Table.Th>
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
                  <TagCell
                    tags={r.tags}
                    suggestions={allTags}
                    onChange={(t) => update(r.id, { tags: t })}
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
