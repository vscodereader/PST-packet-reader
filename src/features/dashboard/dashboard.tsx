import {
  Badge,
  Box,
  Button,
  Card,
  Container,
  Divider,
  Group,
  SimpleGrid,
  Text,
  ThemeIcon,
  Timeline,
  Title,
} from "@mantine/core";

import {
  ACCOUNTS,
  ACTIVITY,
  acctPlatforms,
  PLATFORMS,
  SCHEDULED,
  STATS,
} from "@/shared/data/mock";
import type { GoFn, ViewId } from "@/shared/data/types";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo, PlatformPill } from "@/shared/ui/platform-logo";

function greeting() {
  const h = new Date().getHours();
  if (h < 12) return "좋은 아침이에요";
  if (h < 18) return "오늘도 수고 많아요";
  return "오늘 하루 어땠나요";
}

const STAT_TARGET: Record<string, ViewId> = {
  accounts: "accounts",
  scheduled: "queue",
  today: "log",
  rate: "log",
};

export function Dashboard({ go }: { go: GoFn }) {
  return (
    <Container size={1080} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={28} wrap="wrap">
        <Box>
          <Text size="sm" fw={600} c="dimmed" mb={6}>
            {greeting()}, 지윤님 👋
          </Text>
          <Title order={1} fz={27} fw={800}>
            오늘은 무엇을 써볼까요?
          </Title>
        </Box>
        <Button
          size="md"
          leftSection={<Icon.pencil size={18} />}
          onClick={() => go("posts")}
        >
          새 글 작성
        </Button>
      </Group>

      <SimpleGrid cols={{ base: 2, md: 4 }} spacing="md" mb={30}>
        {STATS.map((s) => {
          const I = Icon[s.icon as keyof typeof Icon];
          return (
            <Card
              key={s.key}
              withBorder
              padding="lg"
              radius="md"
              onClick={() => go(STAT_TARGET[s.key] ?? "log")}
              style={{ cursor: "pointer" }}
            >
              <ThemeIcon
                variant="light"
                color={s.color}
                size={38}
                radius="md"
                mb={14}
              >
                {I && <I size={20} />}
              </ThemeIcon>
              <Text fz={28} fw={800} lh={1}>
                {s.value}
              </Text>
              <Text size="sm" fw={600} c="gray.7" mt={8}>
                {s.label}
              </Text>
              <Text size="xs" c="dimmed" mt={3}>
                {s.sub}
              </Text>
            </Card>
          );
        })}
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="lg">
        <Card withBorder padding={0} radius="md">
          <Group justify="space-between" px="lg" pt="lg" pb="sm">
            <Title order={2} fz={16.5} fw={700}>
              게시 대기열
            </Title>
            <Button
              variant="subtle"
              size="xs"
              rightSection={<Icon.chevronRight size={15} />}
              onClick={() => go("queue")}
            >
              큐 전체
            </Button>
          </Group>
          {SCHEDULED.slice(0, 4).map((s) => {
            const plats = acctPlatforms(s.accounts);
            return (
              <Group
                key={s.id}
                gap={14}
                px="lg"
                py={13}
                wrap="nowrap"
                style={{
                  borderTop: "1px solid var(--mantine-color-gray-2)",
                  cursor: "pointer",
                }}
                onClick={() => go("queue")}
              >
                <Box w={52} ta="center" style={{ flexShrink: 0 }}>
                  <Text fz={11} fw={700} c="dimmed">
                    {s.rel}
                  </Text>
                  <Text fz={14.5} fw={800}>
                    {s.when.split(" ").pop()}
                  </Text>
                </Box>
                <Divider orientation="vertical" />
                <Box style={{ flex: 1, minWidth: 0 }}>
                  <Text size="sm" fw={600} truncate>
                    {s.title}
                  </Text>
                  <Group gap={8} mt={5}>
                    <PlatformPill ids={plats} size={18} />
                    <Text fz={12} c="dimmed">
                      {s.accounts.length}개 계정
                    </Text>
                  </Group>
                </Box>
              </Group>
            );
          })}
        </Card>

        <Card withBorder padding="lg" radius="md">
          <Title order={2} fz={16.5} fw={700} mb="md">
            최근 활동
          </Title>
          <Timeline
            active={ACTIVITY.length}
            bulletSize={24}
            lineWidth={2}
            color="gray"
          >
            {ACTIVITY.map((a) => {
              const color =
                a.type === "success"
                  ? "green"
                  : a.type === "error"
                    ? "red"
                    : "blue";
              const bullet =
                a.type === "success" ? (
                  <Icon.check size={12} />
                ) : a.type === "error" ? (
                  <Icon.x size={12} />
                ) : (
                  <Icon.bell size={11} />
                );
              return (
                <Timeline.Item
                  key={a.id}
                  bullet={
                    <ThemeIcon
                      color={color}
                      radius="xl"
                      size={24}
                      variant="filled"
                    >
                      {bullet}
                    </ThemeIcon>
                  }
                >
                  <Text size="sm" c="gray.7" lh={1.5}>
                    {a.text}
                  </Text>
                  <Text fz={11.5} c="dimmed" mt={3}>
                    {a.time}
                  </Text>
                </Timeline.Item>
              );
            })}
          </Timeline>
        </Card>
      </SimpleGrid>

      <Group justify="space-between" mt={30} mb="md">
        <Title order={2} fz={16.5} fw={700}>
          플랫폼 · 계정 현황
        </Title>
        <Button
          variant="subtle"
          size="xs"
          rightSection={<Icon.chevronRight size={15} />}
          onClick={() => go("accounts")}
        >
          계정 관리
        </Button>
      </Group>
      <SimpleGrid cols={{ base: 2, md: 4 }} spacing="md">
        {PLATFORMS.map((p) => {
          const list = ACCOUNTS.filter((a) => a.platform === p.id);
          const errCount = list.filter((a) => a.status === "error").length;
          const activeCount = list.filter((a) => a.status === "active").length;
          return (
            <Card
              key={p.id}
              withBorder
              padding="lg"
              radius="md"
              onClick={p.soon ? undefined : () => go("accounts")}
              style={{
                opacity: p.soon ? 0.7 : 1,
                cursor: p.soon ? "default" : "pointer",
              }}
            >
              <Group gap={11} mb={14} wrap="nowrap">
                <PlatformLogo id={p.id} size={38} dim={p.soon} />
                <Box style={{ flex: 1, minWidth: 0 }}>
                  <Text size="sm" fw={700} truncate>
                    {p.name}
                  </Text>
                  <Text fz={12} c="dimmed">
                    {p.soon ? "연동 준비 중" : `${list.length}개 계정`}
                  </Text>
                </Box>
              </Group>
              {p.soon ? (
                <Badge color="blue" variant="light">
                  출시 예정
                </Badge>
              ) : (
                <Group gap={8}>
                  <Badge color="green" variant="light">
                    활성 {activeCount}
                  </Badge>
                  {errCount > 0 && (
                    <Badge color="red" variant="light">
                      오류 {errCount}
                    </Badge>
                  )}
                </Group>
              )}
            </Card>
          );
        })}
      </SimpleGrid>
      <Box h={64} />
    </Container>
  );
}
