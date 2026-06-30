import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Group,
  Paper,
  PasswordInput,
  ScrollArea,
  Stack,
  Table,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../../api";

// 계정 분배 화면(§10-3). 상단=계정 풀(스테이징), 하단=연결된 하위. 선택 후 분배하면
// 균등+랜덤(MOVE)으로 나뉘어 전송되고, 보낸 계정은 풀에서 사라진다.
// 서버 연결 시 실데이터(스테이징·online 하위·분배), 오프라인 미리보기면 더미로 폴백.

interface Account {
  id: string;
  loginId: string;
}

// 계정 추가용 인라인 편집 행(모달/prompt 대신). 저장 전까지만 클라이언트에 머무는 임시 행이라
// 비밀번호도 여기서만 잠깐 들고 있다가 import 성공 시 버린다(서버가 at-rest 암호화, §7).
interface Draft {
  key: string;
  loginId: string;
  pw: string;
}

// 인라인 추가 행 key 일련번호. 같은 tick에 여러 행을 추가해도 충돌하지 않게 카운터로 발급한다.
let draftSeq = 0;

interface OnlineDevice {
  id: string;
  name: string;
  ip: string;
}

const INITIAL_ACCOUNTS: Account[] = Array.from({ length: 12 }, (_, i) => ({
  id: `a${i + 1}`,
  loginId: `stock_id${String(i + 1).padStart(3, "0")}`,
}));

const INITIAL_ONLINE_DEVICES: OnlineDevice[] = [
  { id: "d1", name: "하위-001", ip: "1.2.3.4" },
  { id: "d3", name: "하위-003", ip: "5.6.7.8" },
  { id: "d5", name: "하위-005", ip: "9.10.11.12" },
];

// 균등 분배(±1) 인원수. 랜덤성을 위해 어느 대가 +1 받을지 셔플로 정한다(§10-3).
function splitCounts(total: number, buckets: number): number[] {
  const base = Math.floor(total / buckets);
  const rem = total % buckets;
  const order = Array.from({ length: buckets }, (_, i) => i);
  // Fisher–Yates 셔플로 +1 받을 버킷을 무작위화.
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    const tmp = order[i] as number;
    order[i] = order[j] as number;
    order[j] = tmp;
  }
  const plusOne = new Set(order.slice(0, rem));
  return Array.from(
    { length: buckets },
    (_, i) => base + (plusOne.has(i) ? 1 : 0),
  );
}

export function AccountDistribute() {
  const [accounts, setAccounts] = useState<Account[]>(INITIAL_ACCOUNTS);
  const [onlineDevices, setOnlineDevices] = useState<OnlineDevice[]>(
    INITIAL_ONLINE_DEVICES,
  );
  const [selAcc, setSelAcc] = useState<Set<string>>(new Set());
  const [selDev, setSelDev] = useState<Set<string>>(new Set());
  // 인라인 추가 중인 행들(아직 import 안 된 임시 행). 데스크톱 pstmacro처럼 "행 추가→그 자리에서 입력".
  const [drafts, setDrafts] = useState<Draft[]>([]);

  // 스테이징 계정·online 하위 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지).
  const loadAccounts = () => {
    api.accounts
      .list()
      .then((list) =>
        setAccounts(list.map((a) => ({ id: a.id, loginId: a.loginId }))),
      )
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  };
  useEffect(() => {
    loadAccounts();
    // 하단: 등록+online 하위만(§10-3). DeviceDto.connected 필터.
    api.devices
      .list()
      .then((list) =>
        setOnlineDevices(
          list
            .filter((d) => d.connected)
            .map((d) => ({ id: d.id, name: d.name, ip: d.ip ?? "—" })),
        ),
      )
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  }, []);

  // 계정 추가(스테이징) — 모달/prompt 대신 표에 빈 인라인 행을 하나 추가하고, 그 행에서 직접
  // 아이디·비밀번호를 입력하게 한다(데스크톱 pstmacro와 동일 UX).
  const addDraftRow = () => {
    draftSeq += 1;
    setDrafts((prev) => [
      ...prev,
      { key: `draft-${draftSeq}`, loginId: "", pw: "" },
    ]);
  };

  const updateDraft = (key: string, patch: Partial<Draft>) =>
    setDrafts((prev) =>
      prev.map((d) => (d.key === key ? { ...d, ...patch } : d)),
    );

  const removeDraft = (key: string) =>
    setDrafts((prev) => prev.filter((d) => d.key !== key));

  // 인라인 행 저장 — 서버 import 엔드포인트로 1건 추가(at-rest 암호화는 서버가 수행, §7). 성공하면
  // 임시 행을 지우고 스테이징 풀을 갱신한다. 오프라인 미리보기면 로컬 풀에 바로 반영한다.
  const saveDraft = async (key: string) => {
    const draft = drafts.find((d) => d.key === key);
    if (!draft) return;
    const loginId = draft.loginId.trim();
    const pw = draft.pw;
    if (loginId === "" || pw === "") {
      notifications.show({
        message: "아이디와 비밀번호를 모두 입력하세요",
        color: "red",
      });
      return;
    }
    try {
      const r = await api.accounts.import([{ loginId, pw }]);
      removeDraft(key);
      loadAccounts();
      notifications.show({
        message: `계정 추가: ${r.imported}건 (중복 ${r.skipped})`,
        color: "green",
      });
    } catch (e) {
      if (isOffline(e)) {
        draftSeq += 1;
        setAccounts((prev) => [...prev, { id: `local-${draftSeq}`, loginId }]);
        removeDraft(key);
        notifications.show({ message: "계정 추가(미리보기)", color: "green" });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "추가 실패",
          color: "red",
        });
      }
    }
  };

  const allAccChecked = accounts.length > 0 && selAcc.size === accounts.length;
  const someAccChecked = selAcc.size > 0 && !allAccChecked;

  const toggleAcc = (id: string) =>
    setSelAcc((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleAllAcc = () =>
    setSelAcc((prev) =>
      prev.size === accounts.length
        ? new Set()
        : new Set(accounts.map((a) => a.id)),
    );

  const toggleDev = (id: string) =>
    setSelDev((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const canDistribute = selAcc.size >= 1 && selDev.size >= 1;

  const distribute = async () => {
    const accountIds = [...selAcc];
    const deviceIds = [...selDev];
    try {
      // 서버가 균등+랜덤 분배(겹침 없음) + MOVE(스테이징에서 제거) + 대별 명령 push(§10-3).
      const r = await api.accounts.distribute(accountIds, deviceIds);
      const summary = r.assignments
        .map((a) => `${a.deviceName} ${a.count}`)
        .join("·");
      notifications.show({
        message: `계정 ${r.moved}개를 ${r.assignments.length}대에 분배했어요 (${summary})`,
        color: "green",
      });
      setSelAcc(new Set());
      setSelDev(new Set());
      loadAccounts(); // MOVE 반영(서버에서 제거됨 → 풀 갱신)
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 로컬에서 균등+랜덤 시연 후 풀에서 제거(MOVE, §7).
        const counts = splitCounts(selAcc.size, selDev.size);
        notifications.show({
          message: `계정 ${selAcc.size}개를 ${selDev.size}대에 분배했어요 (${counts.join("·")})`,
          color: "green",
        });
        setAccounts((prev) => prev.filter((a) => !selAcc.has(a.id)));
        setSelAcc(new Set());
        setSelDev(new Set());
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "분배 실패",
          color: "red",
        });
      }
    }
  };

  return (
    <Box
      p="lg"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 16,
        height: "100%",
      }}
    >
      {/* ── 상단 절반: 계정 풀 ── */}
      <Paper withBorder radius="md" p="lg">
        <Group justify="space-between" mb="sm">
          <Group gap="xs">
            <Text fw={800} size="lg">
              계정 풀
            </Text>
            <Badge variant="light" color="gray" radius="sm">
              총 {accounts.length}개 계정
            </Badge>
            {selAcc.size > 0 && (
              <Badge variant="light" color="blue" radius="sm">
                {selAcc.size}개 선택
              </Badge>
            )}
          </Group>
          <Group gap="xs">
            <Button
              variant="light"
              size="sm"
              leftSection={<Icon.plus size={16} />}
              onClick={addDraftRow}
            >
              계정 추가
            </Button>
            <Button
              variant="light"
              size="sm"
              leftSection={<Icon.download size={16} />}
              onClick={() =>
                notifications.show({
                  message: "엑셀 가져오기(데모)",
                  color: "gray",
                })
              }
            >
              엑셀 가져오기
            </Button>
          </Group>
        </Group>

        {/* 최대 8행 고정 높이 + 세로 스크롤(§10-3) */}
        <ScrollArea h={8 * 41} type="auto">
          <Table highlightOnHover stickyHeader verticalSpacing="xs">
            <Table.Thead>
              <Table.Tr>
                <Table.Th w={44}>
                  <Checkbox
                    checked={allAccChecked}
                    indeterminate={someAccChecked}
                    onChange={toggleAllAcc}
                    aria-label="전체 선택"
                  />
                </Table.Th>
                <Table.Th>아이디</Table.Th>
                <Table.Th>비밀번호</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {drafts.map((d) => (
                <Table.Tr key={d.key} bg="var(--mantine-color-blue-light)">
                  <Table.Td>
                    <ActionIcon
                      variant="subtle"
                      color="gray"
                      aria-label="행 취소"
                      onClick={() => removeDraft(d.key)}
                    >
                      <Icon.x size={16} />
                    </ActionIcon>
                  </Table.Td>
                  <Table.Td>
                    <TextInput
                      size="xs"
                      autoFocus
                      placeholder="아이디"
                      aria-label="새 계정 아이디"
                      value={d.loginId}
                      onChange={(e) =>
                        updateDraft(d.key, { loginId: e.currentTarget.value })
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void saveDraft(d.key);
                        if (e.key === "Escape") removeDraft(d.key);
                      }}
                    />
                  </Table.Td>
                  <Table.Td>
                    <Group gap={4} wrap="nowrap">
                      <PasswordInput
                        size="xs"
                        placeholder="비밀번호"
                        aria-label="새 계정 비밀번호"
                        value={d.pw}
                        style={{ flex: 1 }}
                        onChange={(e) =>
                          updateDraft(d.key, { pw: e.currentTarget.value })
                        }
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void saveDraft(d.key);
                          if (e.key === "Escape") removeDraft(d.key);
                        }}
                      />
                      <ActionIcon
                        variant="light"
                        color="blue"
                        aria-label="계정 저장"
                        disabled={d.loginId.trim() === "" || d.pw === ""}
                        onClick={() => void saveDraft(d.key)}
                      >
                        <Icon.check size={16} />
                      </ActionIcon>
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
              {accounts.map((a) => (
                <Table.Tr key={a.id}>
                  <Table.Td>
                    <Checkbox
                      checked={selAcc.has(a.id)}
                      onChange={() => toggleAcc(a.id)}
                      aria-label={a.loginId}
                    />
                  </Table.Td>
                  <Table.Td>{a.loginId}</Table.Td>
                  <Table.Td>
                    <Text c="dimmed">••••••</Text>
                  </Table.Td>
                </Table.Tr>
              ))}
              {accounts.length === 0 && drafts.length === 0 && (
                <Table.Tr>
                  <Table.Td colSpan={3}>
                    <Text c="dimmed" ta="center" py="md">
                      계정이 비었습니다 — 분배(MOVE)로 모두 하위에 보냈어요.
                    </Text>
                  </Table.Td>
                </Table.Tr>
              )}
            </Table.Tbody>
          </Table>
        </ScrollArea>
      </Paper>

      {/* ── 하단 절반: 연결된 하위 + 분배하기 ── */}
      <Paper
        withBorder
        radius="md"
        p="lg"
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Group justify="space-between" mb="sm">
          <Group gap="xs">
            <Text fw={800} size="lg">
              연결된 하위 컴퓨터
            </Text>
            <Text size="xs" c="dimmed">
              (online 만 표시)
            </Text>
          </Group>
          <Button
            color="blue"
            disabled={!canDistribute}
            leftSection={<Icon.send size={16} />}
            onClick={() => void distribute()}
          >
            분배하기
          </Button>
        </Group>

        <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          <Stack gap="xs">
            {onlineDevices.map((d) => (
              <Paper key={d.id} withBorder radius="md" p="sm">
                <Group gap="md" wrap="nowrap">
                  <Checkbox
                    checked={selDev.has(d.id)}
                    onChange={() => toggleDev(d.id)}
                    aria-label={d.name}
                  />
                  <ThemeIcon size={38} radius="md" variant="light" color="blue">
                    <IconDeviceDesktop size={22} />
                  </ThemeIcon>
                  <Box style={{ flex: 1 }}>
                    <Text fw={700} size="sm">
                      {d.name}
                    </Text>
                    <Group gap={7} mt={2}>
                      <Box
                        w={9}
                        h={9}
                        style={{
                          borderRadius: 999,
                          background: "var(--mantine-color-green-6)",
                        }}
                      />
                      <Text size="xs" c="gray.7" fw={600}>
                        online · IP {d.ip}
                      </Text>
                    </Group>
                  </Box>
                </Group>
              </Paper>
            ))}
          </Stack>
        </Box>

        <Text size="xs" c="dimmed" mt="sm">
          분배 = 균등+랜덤(겹침 없음, ±1) · MOVE(보낸 계정은 풀에서 사라짐,
          §7·§10-3)
        </Text>
      </Paper>
    </Box>
  );
}
