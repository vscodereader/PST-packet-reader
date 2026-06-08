import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Center,
  Container,
  Group,
  Loader,
  Paper,
  Stack,
  Text,
  ThemeIcon,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useRef, useState } from "react";

import { KIND, KIND_ICON } from "@/shared/data/config";
import type {
  GoFn,
  PlatformId,
  QueueLocation,
  QueueNowItem,
  QueueScheduledItem,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";
import { PlatformPill } from "@/shared/ui/platform-logo";

function LocSummary({
  locs,
  size = 18,
}: {
  locs: QueueLocation[];
  size?: number;
}) {
  const plats: PlatformId[] = [];
  locs.forEach((l) => {
    if (!plats.includes(l.p)) plats.push(l.p);
  });
  const label =
    locs.length === 1
      ? locs[0]?.name
      : `${locs[0]?.name} 외 ${locs.length - 1}곳`;
  return (
    <Group gap={8} wrap="nowrap" style={{ minWidth: 0 }}>
      <PlatformPill ids={plats} size={size} />
      <Text fz={12} c="dimmed" truncate>
        {label}
      </Text>
    </Group>
  );
}

/**
 * Inline re-schedule control for a "missed" item: a date/time picker seeded to
 * "now" plus a confirm button. Local state keeps the pick until the user commits
 * so we don't fire a reschedule on every adjustment.
 */
function RescheduleControl({
  onSubmit,
}: {
  onSubmit: (date: string, time: string) => void;
}) {
  const [date, setDate] = useState(() => nowParts().date);
  const [time, setTime] = useState(() => nowParts().time);
  // 사용자가 picker를 건드리지 않아 시드("지금")가 현재보다 과거가 됐으면(컨트롤이 오래
  // 열려 있던 경우) 제출 시 현재 시각으로 올려, 백엔드 과거-시각 거부를 피한다.
  const submit = () => {
    const fresh = nowParts();
    if (toEpochMs(date, time) < toEpochMs(fresh.date, fresh.time)) {
      onSubmit(fresh.date, fresh.time);
    } else {
      onSubmit(date, time);
    }
  };
  return (
    <Group gap={6} wrap="nowrap">
      <DateTimePicker
        date={date}
        time={time}
        onChange={(v) => {
          setDate(v.date);
          setTime(v.time);
        }}
      />
      <Button size="sm" color="blue" onClick={submit}>
        재예약
      </Button>
    </Group>
  );
}

export function Queue({ go }: { go: GoFn }) {
  const [now, setNow] = useState<QueueNowItem[]>([]);
  const [sched, setSched] = useState<QueueScheduledItem[]>([]);
  const [dragId, setDragId] = useState<string | null>(null);

  // 폴링/이벤트 콜백에서 최신 값을 읽기 위한 ref (stale closure 회피).
  const nowRef = useRef<QueueNowItem[]>(now);
  const dragIdRef = useRef<string | null>(dragId);
  // 순서 영속화(reorderNow)가 끝나기 전에 폴링이 낙관적 순서를 덮어쓰지 않도록 막는다.
  const persistingRef = useRef(false);
  useEffect(() => {
    nowRef.current = now;
    dragIdRef.current = dragId;
  }, [now, dragId]);

  useEffect(() => {
    void ipc.queue.listNow().then(setNow);
    void ipc.queue.listScheduled().then(setSched);
    // 워커 진행률·상태를 주기적으로 반영. 단 드래그 중이거나 순서 영속화 대기 중에는
    // 사용자가 맞춘 로컬 순서를 덮어쓰지 않도록 폴링을 건너뛴다.
    const timer = window.setInterval(() => {
      // 예약 큐는 드래그 대상이 아니므로 항상 폴링한다 — 스케줄러의 자동 게시(예약→now
      // 이동)와 "놓침" 표시가 화면에 실시간 반영되게 한다.
      void ipc.queue.listScheduled().then(setSched);
      if (dragIdRef.current !== null || persistingRef.current) return;
      void ipc.queue.listNow().then(setNow);
    }, 1500);
    return () => window.clearInterval(timer);
  }, []);

  // 대기열 순서를 백엔드에 영속화한다. 응답이 올 때까지 폴링을 막아(persistingRef)
  // 진행 중인 변경이 되돌려지지 않게 한다. drag(onDragEnd)와 화살표(move) 공통 경로.
  const persistOrder = (orderedIds: string[]) => {
    persistingRef.current = true;
    void ipc.queue
      .reorderNow(orderedIds)
      .then(setNow)
      .finally(() => {
        persistingRef.current = false;
      });
  };

  // 실행 중(running) 아이템은 워커가 처리 중이라 맨 앞에 고정한다. 단 실행 중인 게
  // 없으면(워커 idle) 첫 대기 아이템도 자유롭게 옮길 수 있어야 하므로, index 0을 무조건
  // 막지 않고 "선두 running 개수"만큼만 고정한다(백엔드 apply_reorder_now와 일치).
  const pinnedCount = (list: QueueNowItem[]) =>
    list[0]?.state === "running" ? 1 : 0;

  const reorder = (id: string, targetId: string) => {
    setNow((list) => {
      const from = list.findIndex((x) => x.id === id);
      const to = list.findIndex((x) => x.id === targetId);
      const pinned = pinnedCount(list);
      if (from < pinned || to < pinned || from === to) return list;
      const copy = [...list];
      const [m] = copy.splice(from, 1);
      if (m) copy.splice(to, 0, m);
      return copy;
    });
  };
  const move = (id: string, dir: -1 | 1) => {
    const list = nowRef.current;
    const pinned = pinnedCount(list);
    const i = list.findIndex((x) => x.id === id);
    const j = i + dir;
    if (i < pinned || j < pinned || j >= list.length) return;
    const copy = [...list];
    const a = copy[i];
    const b = copy[j];
    if (!a || !b) return;
    copy[i] = b;
    copy[j] = a;
    setNow(copy);
    persistOrder(copy.map((x) => x.id));
  };
  const cancel = (id: string) => {
    void ipc.queue.cancelNow(id).then(setNow);
    notifications.show({ message: "대기 작업을 취소했어요", color: "blue" });
  };
  const promote = (id: string) => {
    void ipc.queue.promote(id).then((next) => {
      setNow(next);
      void ipc.queue.listScheduled().then(setSched);
    });
    notifications.show({
      message: "예약을 즉시 대기열로 옮겼어요",
      color: "green",
    });
  };
  const cancelScheduled = (id: string) => {
    void ipc.queue.cancelScheduled(id).then(setSched);
    notifications.show({ message: "예약을 취소했어요", color: "blue" });
  };
  // 놓친 예약을 새 시각으로 되살린다(또는 대기 예약의 시각 변경). 백엔드가 과거 시각을
  // 거부하면 알림으로 알린다.
  const reschedule = (id: string, date: string, time: string) => {
    const m = scheduleMoment(date, time);
    void ipc.queue
      .reschedule(id, toEpochMs(date, time), m.when, m.label)
      .then((next) => {
        setSched(next);
        notifications.show({
          message: "예약 시각을 변경했어요",
          color: "green",
        });
      })
      .catch(() => {
        notifications.show({
          message: "지난 시각으로는 예약할 수 없어요",
          color: "red",
        });
      });
  };

  const waiting = now.filter((q) => q.state !== "running");

  return (
    <Container size={980} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={24} wrap="wrap">
        <Box>
          <Title order={1} fz={25} fw={800}>
            게시 큐
          </Title>
          <Text size="sm" c="dimmed" mt={6}>
            즉시 게시 작업은 대기열에 쌓여 위에서부터 처리돼요. 드래그해서
            우선순위를 바꾸세요.
          </Text>
        </Box>
        <Button
          size="sm"
          leftSection={<Icon.pencil size={17} />}
          onClick={() => go("posts")}
        >
          새 작업 추가
        </Button>
      </Group>

      <Group gap={8} mb={12}>
        <Box w={7} h={7} bg="green" style={{ borderRadius: 999 }} />
        <Text fz={13} fw={700} c="gray.7">
          즉시 처리 대기열
        </Text>
        <Text fz={12} c="dimmed">
          {now.length}건
        </Text>
        <Group gap={5} ml="auto">
          <Icon.gripper size={14} color="var(--mantine-color-gray-5)" />
          <Text fz={12} c="dimmed">
            드래그로 순서 변경
          </Text>
        </Group>
      </Group>

      <Stack gap={8} mb={34}>
        {now.map((q) => {
          const running = q.state === "running";
          const kd = KIND[q.kind] ?? { t: q.kind, c: "gray" };
          const KI =
            Icon[(KIND_ICON[q.kind] ?? "fileText") as keyof typeof Icon];
          const dragging = dragId === q.id;
          const order = running
            ? null
            : waiting.findIndex((w) => w.id === q.id) + 1;
          return (
            <Paper
              key={q.id}
              withBorder
              radius="md"
              draggable={!running}
              onClick={
                running && q.batchId
                  ? () => go("log", { logFilter: { batchId: q.batchId! } })
                  : undefined
              }
              onDragStart={(e) => {
                setDragId(q.id);
                e.dataTransfer.effectAllowed = "move";
              }}
              onDragOver={(e) => {
                e.preventDefault();
                if (dragId && dragId !== q.id) reorder(dragId, q.id);
              }}
              onDragEnd={() => {
                const dragged = dragId !== null;
                setDragId(null);
                // 드래그로 바뀐 최종 순서를 백엔드에 영속화한다.
                if (dragged) persistOrder(nowRef.current.map((x) => x.id));
              }}
              title={running ? "클릭하면 알림에서 세부 로그 보기" : undefined}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 14,
                padding: "13px 14px 13px 10px",
                borderColor: running
                  ? "var(--mantine-color-blue-filled)"
                  : undefined,
                background: running
                  ? "var(--mantine-color-blue-light)"
                  : undefined,
                opacity: dragging ? 0.5 : 1,
                cursor: running ? "pointer" : "grab",
              }}
            >
              <Box
                w={30}
                style={{
                  flexShrink: 0,
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: 2,
                }}
              >
                {running ? (
                  <Loader size={18} />
                ) : (
                  <>
                    <Icon.gripper
                      size={18}
                      color="var(--mantine-color-gray-5)"
                    />
                    <Text fz={11} fw={800} c="dimmed" ff="monospace">
                      {order}
                    </Text>
                  </>
                )}
              </Box>

              <ThemeIcon
                size={36}
                radius="md"
                variant="light"
                color={q.kind === "comment" ? "forum" : "gray"}
              >
                <KI size={18} />
              </ThemeIcon>

              <Box style={{ flex: 1, minWidth: 0 }}>
                <Group gap={8} mb={5} wrap="nowrap">
                  <Badge size="sm" color={kd.c} variant="light">
                    {kd.t}
                  </Badge>
                  <Text fz={14} fw={700} truncate>
                    {q.title}
                  </Text>
                </Group>
                <LocSummary locs={q.locs} />
              </Box>

              {running ? (
                <Group gap={8} wrap="nowrap">
                  <Badge size="sm" color="blue" variant="light">
                    처리중 {q.progress?.[0]}/{q.progress?.[1]}
                  </Badge>
                  <Icon.chevronRight
                    size={17}
                    color="var(--mantine-color-blue-filled)"
                  />
                </Group>
              ) : (
                <Group gap={8} wrap="nowrap">
                  <Text fz={11.5} c="dimmed">
                    {q.locs.length}곳 대기
                  </Text>
                  <Stack gap={1}>
                    <ActionIcon
                      size="sm"
                      variant="subtle"
                      color="gray"
                      title="우선순위 올리기"
                      onClick={() => move(q.id, -1)}
                    >
                      <Icon.chevronUp size={15} />
                    </ActionIcon>
                    <ActionIcon
                      size="sm"
                      variant="subtle"
                      color="gray"
                      title="우선순위 내리기"
                      onClick={() => move(q.id, 1)}
                    >
                      <Icon.chevronDown size={15} />
                    </ActionIcon>
                  </Stack>
                  <ActionIcon
                    size="md"
                    variant="subtle"
                    color="gray"
                    title="취소"
                    onClick={() => cancel(q.id)}
                  >
                    <Icon.x size={17} />
                  </ActionIcon>
                </Group>
              )}
            </Paper>
          );
        })}
        {now.length === 0 && (
          <Paper
            withBorder
            radius="md"
            style={{ borderStyle: "dashed" }}
            py={44}
          >
            <Center>
              <Stack align="center" gap={8}>
                <Icon.check size={32} color="var(--mantine-color-gray-5)" />
                <Text size="sm" fw={600} c="dimmed">
                  대기 중인 즉시 작업이 없어요
                </Text>
              </Stack>
            </Center>
          </Paper>
        )}
      </Stack>

      <Group gap={8} mb={12}>
        <Icon.calendar size={15} color="var(--mantine-color-gray-6)" />
        <Text fz={13} fw={700} c="gray.7">
          예약 대기
        </Text>
        <Text fz={12} c="dimmed">
          {sched.length}건
        </Text>
      </Group>
      <Stack gap={8}>
        {sched.map((q) => {
          const kd = KIND[q.kind] ?? { t: q.kind, c: "gray" };
          return (
            <Paper
              key={q.id}
              withBorder
              radius="md"
              p="md"
              style={{ display: "flex", alignItems: "center", gap: 14 }}
            >
              <Box
                w={64}
                ta="center"
                style={{
                  flexShrink: 0,
                  borderRight: "1px solid var(--mantine-color-gray-2)",
                  paddingRight: 12,
                }}
              >
                <Text fz={16} fw={800}>
                  {q.when.split(" ").pop()}
                </Text>
                <Text fz={11} c="dimmed" fw={600} mt={2}>
                  {q.rel}
                </Text>
              </Box>
              <Box style={{ flex: 1, minWidth: 0 }}>
                <Group gap={8} mb={5} wrap="nowrap">
                  <Badge size="sm" color={kd.c} variant="light">
                    {kd.t}
                  </Badge>
                  <Text fz={14} fw={700} truncate>
                    {q.title}
                  </Text>
                </Group>
                <LocSummary locs={q.locs} />
              </Box>
              {q.missed ? (
                <>
                  <Badge size="sm" color="red" variant="light">
                    놓침
                  </Badge>
                  <RescheduleControl
                    onSubmit={(date, time) => reschedule(q.id, date, time)}
                  />
                </>
              ) : (
                <>
                  <Badge size="sm" color="yellow" variant="light">
                    예약됨
                  </Badge>
                  <Button
                    size="sm"
                    variant="default"
                    leftSection={<Icon.bolt size={14} />}
                    onClick={() => promote(q.id)}
                  >
                    즉시 처리
                  </Button>
                </>
              )}
              <ActionIcon
                size="md"
                variant="subtle"
                color="gray"
                title="예약 취소"
                onClick={() => cancelScheduled(q.id)}
              >
                <Icon.x size={17} />
              </ActionIcon>
            </Paper>
          );
        })}
        {sched.length === 0 && (
          <Paper
            withBorder
            radius="md"
            style={{ borderStyle: "dashed" }}
            py={44}
          >
            <Center>
              <Stack align="center" gap={8}>
                <Icon.check size={32} color="var(--mantine-color-gray-5)" />
                <Text size="sm" fw={600} c="dimmed">
                  예약 중인 작업이 없어요
                </Text>
              </Stack>
            </Center>
          </Paper>
        )}
      </Stack>
    </Container>
  );
}
