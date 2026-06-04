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
import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";

import { STATUS_ACCOUNT, STATUS_ACCOUNT_ORDER } from "@/shared/data/config";
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
  onChange,
}: {
  value: AccountStatus;
  onChange: (v: AccountStatus) => void;
}) {
  const st = STATUS_ACCOUNT[value] ?? { t: value, c: "gray" };
  return (
    <Badge
      size="sm"
      color={st.c}
      variant="light"
      style={{ cursor: "pointer" }}
      title="클릭하여 상태 변경"
      onClick={() => {
        const next =
          STATUS_ACCOUNT_ORDER[
            (STATUS_ACCOUNT_ORDER.indexOf(value) + 1) %
              STATUS_ACCOUNT_ORDER.length
          ];
        onChange(next as AccountStatus);
      }}
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
  // 로그인 폴링은 한 번 만들어진 인터벌 클로저에서 돈다. 그 안에서 "현재" 행을 보려면
  // 클로저에 갇힌 rows 대신 이 ref를 참조한다(아래 effect가 최신 rows로 동기화).
  const rowsRef = useRef(rows);
  useEffect(() => {
    rowsRef.current = rows;
  }, [rows]);

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
    const cur = rows.find((r) => r.id === id);
    setRows((rs) => rs.map((r) => (r.id === id ? { ...r, ...patch } : r)));
    if (cur) void ipc.accounts.update({ ...cur, ...patch }).then(setRows);
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
      await ipc.auth.bootstrap();
      await ipc.auth.saveAccounts(
        targets.map((t) => ({
          id: t.loginId,
          password: t.pw,
          label: t.loginId,
        })),
      );
      await ipc.auth.enqueueLogin(targets.map((t) => t.loginId));
      pollLogin(targets);
    } catch (err) {
      setLoggingIn(false);
      toast(err instanceof Error ? err.message : String(err), "red");
    }
  };

  // get_queue_status를 2초마다 확인해 각 계정의 로그인 결과를 반영한다.
  const pollLogin = (targets: Account[]) => {
    if (loginPollRef.current !== null)
      window.clearInterval(loginPollRef.current);
    // 행 추적은 고유키 id로 한다(loginId는 유니크가 보장되지 않아 같은 loginId의 두 행이
    // 하나로 합쳐지면 한쪽만 반영된다).
    const remaining = new Set(targets.map((t) => t.id));

    loginPollRef.current = window.setInterval(() => {
      void ipc.auth
        .queueStatus()
        .then((status) => {
          targets.forEach((t) => {
            if (!remaining.has(t.id)) return;
            // 백엔드 잡은 loginId(=쿠키 키)로 식별된다. 같은 loginId를 쓰는 행들은
            // 같은 잡 결과를 각자(id별로) 반영한다.
            const job = [...status.jobs]
              .reverse()
              .find((j) => j.accountId === t.loginId);
            if (!job || job.status === "pending" || job.status === "running")
              return;

            remaining.delete(t.id);
            const ok = job.status === "success" || job.status === "expired";
            const nextStatus = ok ? "active" : "error";
            // 폴링(수 초~분) 중 사용자가 같은 행을 편집했을 수 있으므로, 클릭 시점 스냅샷(t)이
            // 아니라 "현재" 행에 status만 머지하고 권위 리스트로 reconcile한다(다른 핸들러와 동일).
            const cur = rowsRef.current.find((r) => r.id === t.id);
            setRows((rs) =>
              rs.map((r) => (r.id === t.id ? { ...r, status: nextStatus } : r)),
            );
            if (cur)
              void ipc.accounts
                .update({ ...cur, status: nextStatus })
                .then(setRows);
            toast(
              `${t.loginId}: ${ok ? "로그인 성공" : "로그인 실패 — " + job.message}`,
              ok ? "green" : "red",
            );
          });

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
            onClick={() =>
              toast("엑셀(.xlsx) 파일에서 계정을 가져왔어요", "green")
            }
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
              await ipc.excel.exportAccounts(path);
              toast("현재 계정 목록을 엑셀로 내보냈어요", "green");
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
