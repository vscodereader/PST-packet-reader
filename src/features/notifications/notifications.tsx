import { Center, Stack, Text, ThemeIcon } from "@mantine/core";

import { Icon } from "@/shared/ui/icons";

export function Notifications() {
  return (
    <Center h="100%" p="xl">
      <Stack align="center" gap="xs">
        <ThemeIcon size={48} radius="md" variant="light">
          <Icon.bell size={28} />
        </ThemeIcon>
        <Text fw={700}>알림</Text>
        <Text c="dimmed" size="sm">
          구현 예정
        </Text>
      </Stack>
    </Center>
  );
}
