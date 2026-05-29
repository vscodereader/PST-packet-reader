import { Center, Stack, Text, ThemeIcon } from "@mantine/core";

import { Icon } from "@/shared/ui/icons";

export function Dashboard() {
  return (
    <Center h="100%" p="xl">
      <Stack align="center" gap="xs">
        <ThemeIcon size={48} radius="md" variant="light">
          <Icon.dashboard size={28} />
        </ThemeIcon>
        <Text fw={700}>대시보드</Text>
        <Text c="dimmed" size="sm">
          구현 예정
        </Text>
      </Stack>
    </Center>
  );
}
