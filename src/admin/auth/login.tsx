import {
  Anchor,
  Button,
  Center,
  Group,
  Paper,
  PasswordInput,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";

import { Icon } from "@/shared/ui/icons";

import type { Screen } from "../screens";

// 운영자 로그인(§5). 비밀번호 찾기/이메일 인증은 제공하지 않는다.
// onLogin: 로그인 성공 시 다음 화면 결정(첫 로그인=강제 비번변경, 이후=앱). admin-app이 분기.
export function Login({
  go,
  onLogin,
}: {
  go: (s: Screen) => void;
  onLogin: () => void;
}) {
  return (
    <Center h="100dvh" bg="gray.0" p="md">
      <Paper withBorder radius="md" p="xl" w={380} shadow="sm">
        <Stack gap="md">
          <Stack gap={4} align="center">
            <ThemeIcon
              size={46}
              radius="md"
              variant="gradient"
              gradient={{ from: "#4dabf7", to: "#228be6", deg: 135 }}
            >
              <Icon.bolt size={24} />
            </ThemeIcon>
            <Text fw={800} size="xl">
              PLTMacro Admin
            </Text>
            <Text size="sm" c="dimmed">
              운영자 로그인
            </Text>
          </Stack>

          <TextInput label="아이디" placeholder="아이디" />
          <PasswordInput label="비밀번호" placeholder="비밀번호" />

          <Button fullWidth onClick={onLogin}>
            로그인
          </Button>

          <Group justify="space-between">
            <Text size="xs" c="dimmed">
              계정이 없으신가요?
            </Text>
            <Anchor size="xs" fw={600} onClick={() => go("signup")}>
              회원가입
            </Anchor>
          </Group>

          <Text size="xs" c="dimmed" ta="center">
            ※ 비밀번호 찾기는 제공하지 않습니다(§5). 분실 시 운영자에게 문의.
          </Text>
        </Stack>
      </Paper>
    </Center>
  );
}
