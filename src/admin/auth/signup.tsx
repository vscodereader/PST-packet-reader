import {
  Alert,
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
import { notifications } from "@mantine/notifications";

import { Icon } from "@/shared/ui/icons";

import type { Screen } from "../screens";

// 회원가입(§5). 이메일/인증코드 없음. 가입 후 승인되어야 로그인 가능.
export function Signup({ go }: { go: (s: Screen) => void }) {
  return (
    <Center h="100dvh" bg="gray.0" p="md">
      <Paper withBorder radius="md" p="xl" w={380} shadow="sm">
        <Stack gap="md">
          <Stack gap={4} align="center">
            <ThemeIcon size={46} radius="md" variant="light" color="blue">
              <Icon.users size={24} />
            </ThemeIcon>
            <Text fw={800} size="xl">
              운영자 가입 신청
            </Text>
          </Stack>

          <TextInput label="아이디" placeholder="사용할 아이디" />
          <PasswordInput label="비밀번호" placeholder="비밀번호" />
          <PasswordInput
            label="비밀번호 확인"
            placeholder="비밀번호 다시 입력"
          />

          <Alert variant="light" color="gray" p="sm">
            <Text size="xs">
              가입 후 <b>승인된 운영자의 승인</b>을 받아야 로그인할 수 있습니다.
              이메일·인증코드는 사용하지 않습니다.
            </Text>
          </Alert>

          <Button
            fullWidth
            onClick={() => {
              notifications.show({
                message: "가입 신청 완료(데모) — 승인 대기",
                color: "blue",
              });
              go("login");
            }}
          >
            가입 신청
          </Button>

          <Group justify="center" gap={6}>
            <Text size="xs" c="dimmed">
              이미 계정이 있으신가요?
            </Text>
            <Anchor size="xs" fw={600} onClick={() => go("login")}>
              로그인
            </Anchor>
          </Group>
        </Stack>
      </Paper>
    </Center>
  );
}
