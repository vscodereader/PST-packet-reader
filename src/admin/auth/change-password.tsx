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

import { Icon } from "@/shared/ui/icons";

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
  const body = (
    <Paper withBorder radius="md" p="xl" w={400} shadow="sm">
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

        <PasswordInput
          label="현재 비밀번호"
          placeholder={forced ? "Superadmin" : "현재 비밀번호"}
        />
        <PasswordInput label="새 비밀번호" placeholder="새 비밀번호" />
        <PasswordInput
          label="새 비밀번호 확인"
          placeholder="새 비밀번호 다시 입력"
        />

        <Button
          fullWidth
          color={forced ? "orange" : "blue"}
          onClick={() => {
            notifications.show({
              message: "비밀번호가 변경되었습니다",
              color: "green",
            });
            onDone?.(); // 강제 변경 완료 표시(다음 로그인은 앱으로).
            // 변경 후 재로그인(§5).
            go("login");
          }}
        >
          변경하고 다시 로그인
        </Button>
      </Stack>
    </Paper>
  );

  // forced 모드는 로그인 직후 전체화면. 일반 모드도 동일 카드를 가운데 정렬해 보여준다.
  return (
    <Center h="100%" mih="100dvh" bg="gray.0" p="md">
      {body}
    </Center>
  );
}
