import {
  Alert,
  Button,
  Center,
  Paper,
  PasswordInput,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../api";
import type { Screen } from "../screens";

// 비밀번호 변경(§5). forced=true면 SuperAdmin 첫 로그인 강제 변경 모드.
// 변경 후에는 다시 로그인시킨다(확정, §5).
export function ChangePassword({
  forced,
  go,
  onDone,
}: {
  forced: boolean;
  go: (s: Screen) => void;
  onDone?: () => void;
}) {
  const [currentPw, setCurrentPw] = useState(forced ? "Superadmin" : "");
  const [newPw, setNewPw] = useState("");
  const [confirmPw, setConfirmPw] = useState("");

  const submit = async () => {
    if (newPw.length < 4) {
      notifications.show({
        message: "새 비밀번호가 너무 짧습니다",
        color: "red",
      });
      return;
    }
    if (newPw !== confirmPw) {
      notifications.show({
        message: "새 비밀번호 확인이 일치하지 않습니다",
        color: "red",
      });
      return;
    }
    try {
      // 서버에 변경 요청(토큰버전 +1 → 옛 토큰 무효 → 재로그인, §5).
      await api.auth.changePassword(currentPw, newPw);
      api.auth.logout(); // 옛 토큰 폐기
      notifications.show({
        message: "비밀번호가 변경되었습니다",
        color: "green",
      });
      onDone?.();
      go("login");
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 데모 동작.
        notifications.show({
          message: "비밀번호가 변경되었습니다",
          color: "green",
        });
        onDone?.();
        go("login");
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "변경 실패",
          color: "red",
        });
      }
    }
  };

  const body = (
    <Paper withBorder radius="md" p="xl" w={400} shadow="sm">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <Stack gap="md">
          <Stack gap={4} align="center">
            <ThemeIcon
              size={46}
              radius="md"
              variant="light"
              color={forced ? "orange" : "blue"}
            >
              <Icon.settings size={24} />
            </ThemeIcon>
            <Text fw={800} size="xl">
              비밀번호 변경
            </Text>
          </Stack>

          {forced && (
            <Alert variant="light" color="orange" p="sm">
              <Text size="xs">
                기본 비밀번호(<b>Superadmin</b>)를 그대로 사용할 수 없습니다. 새
                비밀번호로 변경한 뒤 <b>다시 로그인</b>해 주세요.
              </Text>
            </Alert>
          )}

          {/* 현재 비밀번호는 미리 채워둔다(강제 변경=기본 Superadmin). 운영자는 새 비번만
            입력/재입력하면 된다 — 매번 현재 비번을 다시 타이핑할 필요 없음. */}
          <PasswordInput
            label="현재 비밀번호"
            placeholder={forced ? "Superadmin" : "현재 비밀번호"}
            value={currentPw}
            onChange={(e) => setCurrentPw(e.currentTarget.value)}
          />
          <PasswordInput
            label="새 비밀번호"
            placeholder="새 비밀번호"
            data-autofocus
            value={newPw}
            onChange={(e) => setNewPw(e.currentTarget.value)}
          />
          <PasswordInput
            label="새 비밀번호 확인"
            placeholder="새 비밀번호 다시 입력"
            value={confirmPw}
            onChange={(e) => setConfirmPw(e.currentTarget.value)}
          />

          {/* type=submit → 새 비번 입력칸에서 Enter만 쳐도 제출(버튼 클릭 불필요). */}
          <Button type="submit" fullWidth color={forced ? "orange" : "blue"}>
            변경하고 다시 로그인
          </Button>
        </Stack>
      </form>
    </Paper>
  );

  // forced 모드는 로그인 직후 전체화면. 일반 모드도 동일 카드를 가운데 정렬해 보여준다.
  return (
    <Center h="100%" mih="100dvh" bg="gray.0" p="md">
      {body}
    </Center>
  );
}
