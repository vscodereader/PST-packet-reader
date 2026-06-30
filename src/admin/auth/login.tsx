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
import { useState } from "react";

import { Icon } from "@/shared/ui/icons";

import type { Screen } from "../screens";

// 운영자 로그인(§5). 비밀번호 찾기/이메일 인증은 제공하지 않는다.
// onLogin(loginId, pw): admin-app이 서버 인증 시도 + 다음 화면 분기(첫 로그인=강제 비번변경).
export function Login({
  go,
  onLogin,
}: {
  go: (s: Screen) => void;
  onLogin: (loginId: string, pw: string) => void | Promise<void>;
}) {
  const [loginId, setLoginId] = useState("");
  const [pw, setPw] = useState("");
  return (
    <Center h="100dvh" bg="gray.0" p="md">
      <Paper withBorder radius="md" p="xl" w={380} shadow="sm">
        {/* form으로 감싸 ID/PW 칸에서 Enter만 쳐도 로그인되게 한다(버튼 클릭 불필요). */}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void onLogin(loginId, pw);
          }}
        >
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

            <TextInput
              label="아이디"
              placeholder="아이디"
              data-autofocus
              value={loginId}
              onChange={(e) => setLoginId(e.currentTarget.value)}
            />
            <PasswordInput
              label="비밀번호"
              placeholder="비밀번호"
              value={pw}
              onChange={(e) => setPw(e.currentTarget.value)}
            />

            {/* type=submit → Enter 또는 클릭 둘 다 form onSubmit을 발동 */}
            <Button type="submit" fullWidth>
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
        </form>
      </Paper>
    </Center>
  );
}
