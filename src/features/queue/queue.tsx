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
import { useState } from "react";

import {
  KIND,
  KIND_ICON,
  QUEUE_NOW,
  QUEUE_SCHEDULED,
} from "@/shared/data/mock";
import type {
  GoFn,
  PlatformId,
  QueueLocation,
  QueueNowItem,
} from "@/shared/data/types";
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

export function Queue({ go }: { go: GoFn }) {
  const [now, setNow] = useState<QueueNowItem[]>(QUEUE_NOW);
  const [dragId, setDragId] = useState<string | null>(null);
  const sched = QUEUE_SCHEDULED;

  const reorder = (id: string, targetId: string) => {
    setNow((list) => {
      const from = list.findIndex((x) => x.id === id);
      const to = list.findIndex((x) => x.id === targetId);
      if (from < 0 || to < 0 || from === to || to === 0) return list;
      const copy = [...list];
      const [m] = copy.splice(from, 1);
      if (m) copy.splice(to, 0, m);
      return copy;
    });
  };
  const move = (id: string, dir: -1 | 1) => {
    setNow((list) => {
      const i = list.findIndex((x) => x.id === id);
      const j = i + dir;
      if (i < 1 || j < 1 || j >= list.length) return list;
      const copy = [...list];
      const a = copy[i];
      const b = copy[j];
      if (a && b) {
        copy[i] = b;
        copy[j] = a;
      }
      return copy;
    });
  };
  const cancel = (id: string) => {
    setNow((l) => l.filter((x) => x.id !== id));
    notifications.show({ message: "대기 작업을 취소했어요", color: "blue" });
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
          size="md"
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
              onDragEnd={() => setDragId(null)}
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
              <Badge size="sm" color="yellow" variant="light">
                예약됨
              </Badge>
              <Button
                size="sm"
                variant="default"
                leftSection={<Icon.bolt size={14} />}
                onClick={() =>
                  notifications.show({
                    message: "예약을 즉시 대기열로 옮겼어요",
                    color: "green",
                  })
                }
              >
                즉시 처리
              </Button>
              <ActionIcon
                size="md"
                variant="subtle"
                color="gray"
                title="예약 취소"
                onClick={() =>
                  notifications.show({
                    message: "예약을 취소했어요",
                    color: "blue",
                  })
                }
              >
                <Icon.x size={17} />
              </ActionIcon>
            </Paper>
          );
        })}
      </Stack>
    </Container>
  );
}
