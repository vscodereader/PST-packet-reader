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
import { useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../api";
import type { Screen } from "../screens";

// 회원가입(§5). 이메일/인증코드 없음. 가입 후 승인되어야 로그인 가능.
export function Signup({ go }: { go: (s: Screen) => void }) {
  const [loginId, setLoginId] = useState("");
  const [pw, setPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");

  const submit = async () => {
    if (loginId.trim() === "" || pw === "") {
      notifications.show({
        message: "아이디·비밀번호를 입력하세요",
        color: "red",
      });
      return;
    }
    if (pw !== confirmPw) {
      notifications.show({
        message: "비밀번호 확인이 일치하지 않습니다",
        color: "red",
      });
      return;
    }
    try {
      await api.auth.signup(loginId, pw);
      notifications.show({
        message: "가입 신청 완료 — 승인 대기",
        color: "blue",
      });
      go("login");
    } catch (e) {
      if (isOffline(e)) {
        notifications.show({
          message: "가입 신청 완료(미리보기) — 승인 대기",
          color: "blue",
        });
        go("login");
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "가입 실패",
          color: "red",
        });
      }
    }
  };

  return (
    <Center h="100dvh" bg="gray.0" p="md">
      <Paper withBorder radius="md" p="xl" w={380} shadow="sm">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <Stack gap="md">
            <Stack gap={4} align="center">
              <ThemeIcon size={46} radius="md" variant="light" color="blue">
                <Icon.users size={24} />
              </ThemeIcon>
              <Text fw={800} size="xl">
                운영자 가입 신청
              </Text>
            </Stack>

            <TextInput
              label="아이디"
              placeholder="사용할 아이디"
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
            <PasswordInput
              label="비밀번호 확인"
              placeholder="비밀번호 다시 입력"
              value={confirmPw}
              onChange={(e) => setConfirmPw(e.currentTarget.value)}
            />

            <Alert variant="light" color="gray" p="sm">
              <Text size="xs">
                가입 후 <b>승인된 운영자의 승인</b>을 받아야 로그인할 수
                있습니다. 이메일·인증코드는 사용하지 않습니다.
              </Text>
            </Alert>

            <Button type="submit" fullWidth>
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
        </form>
      </Paper>
    </Center>
  );
}
