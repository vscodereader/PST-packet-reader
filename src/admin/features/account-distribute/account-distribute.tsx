import {
  Badge,
  Box,
  Button,
  Checkbox,
  Group,
  Paper,
  ScrollArea,
  Stack,
  Table,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useState } from "react";

import { Icon } from "@/shared/ui/icons";

// 계정 분배 화면(§10-3). 상단=계정 풀(스테이징), 하단=연결된 하위. 선택 후 분배하면
// 균등+랜덤(MOVE)으로 나뉘어 전송되고, 보낸 계정은 풀에서 사라진다(미리보기 시연).

interface Account {
  id: string;
  loginId: string;
}

interface OnlineDevice {
  id: string;
  name: string;
  ip: string;
}

const INITIAL_ACCOUNTS: Account[] = Array.from({ length: 12 }, (_, i) => ({
  id: `a${i + 1}`,
  loginId: `stock_id${String(i + 1).padStart(3, "0")}`,
}));

const ONLINE_DEVICES: OnlineDevice[] = [
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
  const [selAcc, setSelAcc] = useState<Set<string>>(new Set());
  const [selDev, setSelDev] = useState<Set<string>>(new Set());

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

  const distribute = () => {
    const counts = splitCounts(selAcc.size, selDev.size);
    notifications.show({
      message: `계정 ${selAcc.size}개를 ${selDev.size}대에 분배했어요 (${counts.join("·")})`,
      color: "green",
    });
    // MOVE: 보낸 계정은 풀에서 제거(§7).
    setAccounts((prev) => prev.filter((a) => !selAcc.has(a.id)));
    setSelAcc(new Set());
    setSelDev(new Set());
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
              onClick={() =>
                notifications.show({
                  message: "계정 추가(데모)",
                  color: "gray",
                })
              }
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
              {accounts.length === 0 && (
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
            onClick={distribute}
          >
            분배하기
          </Button>
        </Group>

        <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
          <Stack gap="xs">
            {ONLINE_DEVICES.map((d) => (
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
