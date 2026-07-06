import {
  Badge,
  Box,
  Button,
  Group,
  Loader,
  Paper,
  ScrollArea,
  SimpleGrid,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline, type QueueItemDto } from "../../api";

// 중지 명령(설계서 08 §10). 게시 명령처럼 하위를 선택하면 그 하위의 **실행 중 게시큐가 실시간으로**
// 보이고(하위 앱 게시큐 화면과 동일 내용), 각 큐 옆 [중지]로 그 큐를, 하위 옆 [중지]로 그 하위의
// 전 큐를 완전 종료한다. 서버 미연결(오프라인 미리보기)이면 더미로 폴백해 화면을 완성한다.

interface StopDevice {
  id: string;
  name: string;
  ip: string;
  online: boolean;
}

const POLL_MS = 1500;

// 중지 요청 페이로드 빌더(순수 — 테스트 대상). 큐 1개 / 디바이스 전체.
export function queueKillReq(deviceId: string, queueId: string) {
  return { deviceId, queueId };
}
export function deviceKillReq(deviceId: string) {
  return { deviceId, all: true };
}

// 큐 종류 → 배지 색.
export function kindColor(kind: string): string {
  switch (kind) {
    case "종토":
      return "blue";
    case "카페":
      return "green";
    case "밴드":
      return "grape";
    case "블로그":
      return "teal";
    case "클립":
      return "pink";
    case "로그인":
      return "gray";
    default:
      return "indigo";
  }
}

const DUMMY_DEVICES: StopDevice[] = [
  { id: "dev-1", name: "1번 컴퓨터", ip: "203.0.113.5", online: true },
  { id: "dev-2", name: "2번 컴퓨터", ip: "203.0.113.6", online: true },
];
function dummyQueue(): QueueItemDto[] {
  return [
    {
      id: "q-1",
      title: "오늘의 시황 브리핑",
      kind: "종토",
      state: "running",
      done: 2,
      total: 5,
      loginIds: ["abc***"],
    },
    {
      id: "q-2",
      title: "관심종목 코멘트",
      kind: "종토",
      state: "waiting",
      done: 0,
      total: 3,
      loginIds: ["def***"],
    },
  ];
}

export function StopCommand() {
  const [devices, setDevices] = useState<StopDevice[]>([]);
  const [sel, setSel] = useState<string | null>(null);
  const [queue, setQueue] = useState<QueueItemDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [offline, setOffline] = useState(false);

  useEffect(() => {
    let alive = true;
    void api.devices
      .list()
      .then((list) => {
        if (!alive) return;
        setDevices(
          list.map((d) => ({
            id: d.id,
            name: d.name,
            ip: d.ip ?? "-",
            online: d.state === "online",
          })),
        );
        setOffline(false);
      })
      .catch((e) => {
        if (!alive) return;
        if (isOffline(e)) {
          setDevices(DUMMY_DEVICES);
          setOffline(true);
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  // 선택 하위의 실행/대기 큐를 폴링해 실시간으로 갱신(설계서 §10-2).
  useEffect(() => {
    if (sel == null) return;
    let alive = true;
    const tick = () => {
      void api.devices
        .queueState(sel)
        .then((qs) => {
          if (!alive) return;
          setQueue(qs.items);
          setLoading(false);
          setOffline(false);
        })
        .catch((e) => {
          if (!alive) return;
          setLoading(false);
          if (isOffline(e)) {
            setQueue(dummyQueue());
            setOffline(true);
          }
        });
    };
    tick();
    const timer = window.setInterval(tick, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [sel]);

  const killOne = useCallback(
    (deviceId: string, item: QueueItemDto) => {
      void api.stop
        .kill(queueKillReq(deviceId, item.id))
        .then(() =>
          notifications.show({
            message: `"${item.title}" 중지 명령을 보냈어요`,
            color: "orange",
          }),
        )
        .catch((e) => {
          if (isOffline(e)) {
            setQueue((prev) => prev.filter((q) => q.id !== item.id));
            notifications.show({
              message: "(미리보기) 큐를 중지했어요",
              color: "orange",
            });
          } else {
            notifications.show({
              message: e instanceof Error ? e.message : "중지 실패",
              color: "red",
            });
          }
        });
    },
    [],
  );

  const killAll = useCallback((deviceId: string) => {
    void api.stop
      .kill(deviceKillReq(deviceId))
      .then(() =>
        notifications.show({
          message: "이 컴퓨터의 실행 중 큐를 모두 중지했어요",
          color: "orange",
        }),
      )
      .catch((e) => {
        if (isOffline(e)) {
          setQueue([]);
          notifications.show({
            message: "(미리보기) 전체 큐를 중지했어요",
            color: "orange",
          });
        } else {
          notifications.show({
            message: e instanceof Error ? e.message : "중지 실패",
            color: "red",
          });
        }
      });
  }, []);

  return (
    <Box p="lg">
      <Group justify="space-between" mb="xs">
        <Box>
          <Text fw={800} size="xl">
            중지 명령
          </Text>
          <Text size="sm" c="dimmed">
            하위 컴퓨터를 고르면 실행 중인 게시큐가 실시간으로 보여요. 각 큐 옆
            [중지]로 그 큐만, 컴퓨터 옆 [중지]로 그 컴퓨터의 모든 큐를 완전
            종료합니다.
          </Text>
        </Box>
        {offline && (
          <Badge color="orange" variant="light">
            미리보기(서버 미연결)
          </Badge>
        )}
      </Group>

      {/* ① 하위 컴퓨터 선택 + 컴퓨터 전체 중지 */}
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 4 }} spacing="sm" mb="lg">
        {devices.map((d) => {
          const active = d.id === sel;
          return (
            <Paper
              key={d.id}
              withBorder
              radius="md"
              p="sm"
              onClick={() => {
                if (d.id !== sel) {
                  setSel(d.id);
                  setLoading(true);
                  setQueue([]);
                }
              }}
              style={{
                cursor: "pointer",
                borderColor: active
                  ? "var(--mantine-color-blue-filled)"
                  : undefined,
                background: active
                  ? "var(--mantine-color-blue-light)"
                  : undefined,
              }}
            >
              <Group justify="space-between" wrap="nowrap">
                <Group gap={8} wrap="nowrap">
                  <ThemeIcon
                    variant="light"
                    color={d.online ? "blue" : "gray"}
                    radius="md"
                  >
                    <IconDeviceDesktop size={18} />
                  </ThemeIcon>
                  <Box>
                    <Text fw={700} size="sm" lh={1.1}>
                      {d.name}
                    </Text>
                    <Text size="xs" c="dimmed">
                      {d.ip}
                    </Text>
                  </Box>
                </Group>
                <Button
                  size="compact-xs"
                  color="red"
                  variant="light"
                  leftSection={<Icon.trash size={13} />}
                  onClick={(e) => {
                    e.stopPropagation();
                    killAll(d.id);
                  }}
                >
                  중지
                </Button>
              </Group>
            </Paper>
          );
        })}
      </SimpleGrid>

      {/* ② 선택 하위의 실행/대기 큐 실시간 목록 + 큐별 중지 */}
      {sel == null ? (
        <Text c="dimmed" ta="center" py="xl">
          하위 컴퓨터를 선택하세요.
        </Text>
      ) : (
        <Paper withBorder radius="md" p="sm">
          <Group justify="space-between" mb="xs">
            <Text fw={700} size="sm">
              실행 중인 게시큐
            </Text>
            {loading && <Loader size="xs" />}
          </Group>
          {queue.length === 0 ? (
            <Text c="dimmed" ta="center" py="lg" size="sm">
              실행 중인 게시큐가 없습니다.
            </Text>
          ) : (
            <ScrollArea.Autosize mah={460}>
              <Stack gap={6}>
                {queue.map((q) => (
                  <Paper key={q.id} withBorder radius="sm" p="xs">
                    <Group justify="space-between" wrap="nowrap">
                      <Group gap={8} wrap="nowrap" style={{ minWidth: 0 }}>
                        <Badge
                          size="sm"
                          color={kindColor(q.kind)}
                          variant="light"
                        >
                          {q.kind}
                        </Badge>
                        <Box style={{ minWidth: 0 }}>
                          <Text fw={600} size="sm" truncate>
                            {q.title}
                          </Text>
                          <Text size="xs" c="dimmed" truncate>
                            {q.loginIds.join(", ") || "-"} ·{" "}
                            {q.state === "running" ? "실행 중" : "대기"} {q.done}/
                            {q.total}
                          </Text>
                        </Box>
                      </Group>
                      <Button
                        size="compact-xs"
                        color="red"
                        variant="light"
                        leftSection={<Icon.trash size={13} />}
                        onClick={() => killOne(sel, q)}
                      >
                        중지
                      </Button>
                    </Group>
                  </Paper>
                ))}
              </Stack>
            </ScrollArea.Autosize>
          )}
        </Paper>
      )}
    </Box>
  );
}
