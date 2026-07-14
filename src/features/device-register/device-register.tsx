import {
  Alert,
  Badge,
  Box,
  Button,
  Card,
  Group,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useState } from "react";

import { type AgentStatus, ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

// 하위 앱 원격제어 등록 화면(설계 §6-2). 사람이 입력하는 건 ① 서버 주소 ② 기기코드 둘뿐.
// 등록하면 서버가 장기 기기토큰을 내려주고 에이전트가 자동 연결된다(토큰은 사람이 안 봄).
export function DeviceRegister() {
  const [serverUrl, setServerUrl] = useState("");
  const [code, setCode] = useState("");
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = () => {
    void ipc.agent
      .status()
      .then((s) => {
        setStatus(s);
        // 이미 등록돼 있으면 서버주소를 입력칸에 채워 둔다(재등록 편의).
        if (s.configured) setServerUrl((prev) => prev || s.serverUrl);
      })
      .catch(() => setStatus(null));
  };
  useEffect(refresh, []);

  const register = async () => {
    setBusy(true);
    try {
      const s = await ipc.agent.register(serverUrl.trim(), code.trim());
      setStatus(s);
      setCode("");
      notifications.show({
        message: "원격제어에 등록되었습니다",
        color: "green",
      });
    } catch (e) {
      notifications.show({
        message: e instanceof Error ? e.message : "등록 실패",
        color: "red",
      });
    } finally {
      setBusy(false);
    }
  };

  const unregister = async () => {
    try {
      await ipc.agent.unregister();
      refresh();
      notifications.show({ message: "등록을 해제했습니다", color: "gray" });
    } catch {
      notifications.show({ message: "해제 실패", color: "red" });
    }
  };

  return (
    <Box p={32} style={{ maxWidth: 640, margin: "0 auto" }}>
      <Title order={1} fz={25} fw={800} mb={6}>
        원격제어 등록
      </Title>
      <Text size="sm" c="dimmed" mb="lg">
        Admin이 발급한 <b>서버 주소</b>와 <b>기기코드</b>를 입력해 이 컴퓨터를
        원격제어에 연결합니다. 등록 후엔 IP가 바뀌어도 자동으로 다시 연결됩니다.
      </Text>

      {status?.configured && (
        <Alert
          variant="light"
          color="green"
          mb="md"
          icon={<Icon.check size={16} />}
        >
          <Group justify="space-between">
            <Text size="sm">
              등록됨 — <b>{status.deviceName}</b> · 서버 {status.serverUrl}
            </Text>
            <Badge color="green" variant="light">
              연결 준비
            </Badge>
          </Group>
        </Alert>
      )}

      <Card withBorder padding="lg" radius="md">
        <Stack gap="md">
          <TextInput
            label="서버 주소"
            placeholder="http://123.45.67.89:8080"
            value={serverUrl}
            onChange={(e) => setServerUrl(e.currentTarget.value)}
          />
          <TextInput
            label="기기코드"
            placeholder="Admin에서 발급한 1회용 코드"
            value={code}
            onChange={(e) => setCode(e.currentTarget.value)}
          />
          <Group justify="space-between">
            <Button
              loading={busy}
              disabled={serverUrl.trim() === "" || code.trim() === ""}
              onClick={() => void register()}
            >
              {status?.configured ? "다시 등록" : "등록하기"}
            </Button>
            {status?.configured && (
              <Button
                variant="subtle"
                color="gray"
                onClick={() => void unregister()}
              >
                등록 해제
              </Button>
            )}
          </Group>
          <Text size="xs" c="dimmed">
            ※ 기기토큰은 등록 성공 시 서버가 자동 발급·저장합니다(사람이 입력 안
            함, §6). 연결이 안 되면 Admin에서 이 기기를 삭제하고 새 코드로 다시
            등록하세요(§6-4).
          </Text>
        </Stack>
      </Card>
    </Box>
  );
}
