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
  Select,
  Stack,
  Table,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useState } from "react";

import { ACTIVE_PLATFORMS, PLATFORM } from "@/shared/data/config";
import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../../api";

// 플랫폼 종류(종토/카페/블로그/클립/밴드) — 데스크톱 pstmacro의 PLATFORMS 그대로 재사용(요구서).
// 카페(naver)로 분배한 계정은 하위가 로그인하지 않고 등록만 한다(카페는 게시 순간 로그인).
const PLATFORM_OPTS = ACTIVE_PLATFORMS.map((p) => ({ value: p.id, label: p.name }));
const DEFAULT_PLATFORM = "forum";
/** 플랫폼 id → 짧은 라벨(배지용). 미상이면 그대로. */
function platformLabel(id: string): string {
  return PLATFORM[id]?.short ?? id;
}

// 계정 분배 화면(§10-3). 상단=계정 풀(스테이징), 하단=연결된 하위. 선택 후 분배하면
// 균등+랜덤(MOVE)으로 나뉘어 전송되고, 보낸 계정은 풀에서 사라진다.
// 서버 연결 시 실데이터(스테이징·online 하위·분배), 오프라인 미리보기면 더미로 폴백.

interface Account {
  id: string;
  loginId: string;
  platform: string;
}

// 계정 추가용 인라인 편집 행. 데스크톱 pstmacro와 동일하게, "행 추가 → 그 자리에서 플랫폼/아이디/
// 비밀번호 입력 → 체크박스로 바로 선택"한다. 별도의 "저장" 확정 단계는 없고, 분배 시점에 입력값
// 그대로 전송한다(플랫폼도 함께 — 카페면 하위가 등록만).
interface Draft {
  key: string;
  platform: string;
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
  // 미리보기 더미 — 한 개는 카페로 둬 배지가 눈에 보이게 한다.
  platform: i === 1 ? "naver" : "forum",
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
  // 인라인 추가 중인 행들. 데스크톱 pstmacro처럼 "행 추가→그 자리에서 입력→체크로 바로 선택".
  const [drafts, setDrafts] = useState<Draft[]>([]);

  // 스테이징 계정·online 하위 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지).
  const loadAccounts = () => {
    api.accounts
      .list()
      .then((list) =>
        setAccounts(
          list.map((a) => ({
            id: a.id,
            loginId: a.loginId,
            platform: a.platform ?? "forum",
          })),
        ),
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
  // 아이디·비밀번호를 입력하게 한다(데스크톱 pstmacro와 동일 UX). 추가 즉시 체크박스로 선택 가능.
  const addDraftRow = () => {
    draftSeq += 1;
    setDrafts((prev) => [
      ...prev,
      { key: `draft-${draftSeq}`, platform: DEFAULT_PLATFORM, loginId: "", pw: "" },
    ]);
  };

  const updateDraft = (key: string, patch: Partial<Draft>) =>
    setDrafts((prev) =>
      prev.map((d) => (d.key === key ? { ...d, ...patch } : d)),
    );

  // 인라인 행 삭제. 선택돼 있었으면 선택 집합에서도 함께 뺀다.
  const removeDraft = (key: string) => {
    setDrafts((prev) => prev.filter((d) => d.key !== key));
    setSelAcc((prev) => {
      if (!prev.has(key)) return prev;
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  };

  // 선택 가능한 계정 = 스테이징 계정(id) + 인라인 입력행(key). 입력행도 "저장" 없이 바로 체크로 선택.
  const selectableAccIds = [
    ...accounts.map((a) => a.id),
    ...drafts.map((d) => d.key),
  ];
  const allAccChecked =
    selectableAccIds.length > 0 &&
    selectableAccIds.every((id) => selAcc.has(id));
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
      selectableAccIds.every((id) => prev.has(id))
        ? new Set()
        : new Set(selectableAccIds),
    );

  const toggleDev = (id: string) =>
    setSelDev((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  // 연결된 하위 컴퓨터 전체 선택.
  const allDevChecked =
    onlineDevices.length > 0 && selDev.size === onlineDevices.length;
  const someDevChecked = selDev.size > 0 && !allDevChecked;
  const toggleAllDev = () =>
    setSelDev((prev) =>
      prev.size === onlineDevices.length
        ? new Set()
        : new Set(onlineDevices.map((d) => d.id)),
    );

  const canDistribute = selAcc.size >= 1 && selDev.size >= 1;

  const distribute = async () => {
    const selectedAccountIds = accounts
      .filter((a) => selAcc.has(a.id))
      .map((a) => a.id);
    // 인라인 입력행(임시 계정) — "저장" 확정 없이 입력한 값 그대로 함께 전송한다.
    const selectedDrafts = drafts.filter((d) => selAcc.has(d.key));
    const total = selectedAccountIds.length + selectedDrafts.length;
    const deviceIds = [...selDev];
    try {
      let poolIds = selectedAccountIds;
      // 입력행(draft)은 분배 시점에 입력값 그대로 import한다. import 응답엔 새 계정 ID가 없으므로,
      // 목록을 다시 받아 방금 넣은 loginId로 서버 ID를 찾아 분배 대상에 합친다. (예전엔 draft를
      // import만 하고 그 ID를 분배에 안 넣어, 첫 클릭은 "계정을 선택하세요"로 거부되고 다른 페이지를
      // 갔다 와야(재조회) 반영되던 버그를 고침.)
      if (selectedDrafts.length > 0) {
        await api.accounts.import(
          selectedDrafts.map((d) => ({
            loginId: d.loginId,
            pw: d.pw,
            platform: d.platform,
          })),
        );
        const fresh = await api.accounts.list();
        setAccounts(
          fresh.map((a) => ({
            id: a.id,
            loginId: a.loginId,
            platform: a.platform ?? "forum",
          })),
        );
        const wantLogins = new Set(
          selectedDrafts.map((d) => d.loginId.trim()).filter((s) => s !== ""),
        );
        const newIds = fresh
          .filter((a) => wantLogins.has(a.loginId))
          .map((a) => a.id);
        poolIds = Array.from(new Set([...selectedAccountIds, ...newIds]));
      }
      // 서버가 균등+랜덤 분배(겹침 없음) + MOVE(스테이징에서 제거) + 대별 명령 push(§10-3).
      const r = await api.accounts.distribute(poolIds, deviceIds);
      const summary = r.assignments
        .map((a) => `${a.deviceName} ${a.count}`)
        .join("·");
      notifications.show({
        message: `계정 ${r.moved}개를 ${r.assignments.length}대에 분배했어요 (${summary})`,
        color: "green",
      });
      setSelAcc(new Set());
      setSelDev(new Set());
      setDrafts((prev) => prev.filter((d) => !selAcc.has(d.key)));
      loadAccounts(); // MOVE 반영(서버에서 제거됨 → 풀 갱신)
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 로컬에서 균등+랜덤 시연 후 풀에서 제거(MOVE, §7). 입력행도 함께 처리.
        const counts = splitCounts(total, selDev.size);
        notifications.show({
          message: `계정 ${total}개를 ${selDev.size}대에 분배했어요 (${counts.join("·")})`,
          color: "green",
        });
        setAccounts((prev) => prev.filter((a) => !selAcc.has(a.id)));
        setDrafts((prev) => prev.filter((d) => !selAcc.has(d.key)));
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
              총 {accounts.length + drafts.length}개 계정
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
                {/* 좌측부터: 플랫폼 · 아이디 · 비밀번호 · 휴지통(요구서) */}
                <Table.Th w={140}>플랫폼</Table.Th>
                <Table.Th>아이디</Table.Th>
                <Table.Th>비밀번호</Table.Th>
                <Table.Th w={44} />
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {drafts.map((d) => (
                <Table.Tr key={d.key} bg="var(--mantine-color-blue-light)">
                  <Table.Td>
                    <Checkbox
                      checked={selAcc.has(d.key)}
                      onChange={() => toggleAcc(d.key)}
                      aria-label={d.loginId || "새 계정"}
                    />
                  </Table.Td>
                  <Table.Td>
                    <Select
                      size="xs"
                      data={PLATFORM_OPTS}
                      value={d.platform}
                      allowDeselect={false}
                      comboboxProps={{ withinPortal: true }}
                      aria-label="새 계정 플랫폼"
                      onChange={(v) =>
                        updateDraft(d.key, { platform: v ?? DEFAULT_PLATFORM })
                      }
                    />
                  </Table.Td>
                  <Table.Td>
                    <TextInput
                      size="xs"
                      autoFocus
                      placeholder="아이디"
                      aria-label="새 계정 아이디"
                      value={d.loginId}
                      style={{ width: "100%" }}
                      onChange={(e) =>
                        updateDraft(d.key, { loginId: e.currentTarget.value })
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Escape") removeDraft(d.key);
                      }}
                    />
                  </Table.Td>
                  <Table.Td>
                    <PasswordInput
                      size="xs"
                      placeholder="비밀번호"
                      aria-label="새 계정 비밀번호"
                      value={d.pw}
                      style={{ width: "100%" }}
                      onChange={(e) =>
                        updateDraft(d.key, { pw: e.currentTarget.value })
                      }
                      onKeyDown={(e) => {
                        if (e.key === "Escape") removeDraft(d.key);
                      }}
                    />
                  </Table.Td>
                  {/* 삭제 버튼은 우측 */}
                  <Table.Td>
                    <ActionIcon
                      variant="subtle"
                      color="red"
                      aria-label="행 삭제"
                      onClick={() => removeDraft(d.key)}
                    >
                      <Icon.trash size={16} />
                    </ActionIcon>
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
                  <Table.Td>
                    <Badge
                      variant="light"
                      color={a.platform === "naver" ? "teal" : "gray"}
                      radius="sm"
                    >
                      {platformLabel(a.platform)}
                    </Badge>
                  </Table.Td>
                  <Table.Td>{a.loginId}</Table.Td>
                  <Table.Td>
                    <Text c="dimmed">••••••</Text>
                  </Table.Td>
                  <Table.Td />
                </Table.Tr>
              ))}
              {accounts.length === 0 && drafts.length === 0 && (
                <Table.Tr>
                  <Table.Td colSpan={5}>
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
            {onlineDevices.length > 0 && (
              <Checkbox
                size="xs"
                label="전체 선택"
                checked={allDevChecked}
                indeterminate={someDevChecked}
                onChange={toggleAllDev}
              />
            )}
            {selDev.size > 0 && (
              <Badge variant="light" color="blue" radius="sm">
                {selDev.size}개 선택
              </Badge>
            )}
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
