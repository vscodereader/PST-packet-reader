import {
  Badge,
  Box,
  Button,
  Group,
  Paper,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, getRole, isOffline } from "../../api";

// 운영자 관리(§5). 가입 승인(승인된 운영자 누구나) + 운영자 삭제(SuperAdmin 전용,
// SuperAdmin 계정은 삭제 불가) + 하위 운영자 비번 재설정(SuperAdmin 전용, 사수 확정 PR#324).
// 서버 연결 시 실데이터, 오프라인 미리보기면 더미로 폴백.

interface Operator {
  id: string; // = loginId
  role: "super" | "operator";
}

const INITIAL_PENDING = ["op_kim", "op_lee"];
const INITIAL_OPERATORS: Operator[] = [
  { id: "Superadmin", role: "super" },
  { id: "op_park", role: "operator" },
  { id: "op_choi", role: "operator" },
];

export function Operators() {
  const [pending, setPending] = useState<string[]>(INITIAL_PENDING);
  const [operators, setOperators] = useState<Operator[]>(INITIAL_OPERATORS);
  // 현재 로그인 권한. super(또는 오프라인 미리보기=null)일 때만 SuperAdmin 전용 동작 노출.
  const role = getRole();
  const canSuper = role === "super" || role === null;

  const reload = () => {
    api.operators
      .list()
      .then((r) => {
        setOperators(r.operators.map((o) => ({ id: o.loginId, role: o.role })));
        setPending(r.pending);
      })
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  };
  useEffect(reload, []);

  const approve = async (id: string) => {
    try {
      await api.operators.approve(id);
    } catch (e) {
      if (!isOffline(e)) {
        notifications.show({
          message: e instanceof Error ? e.message : "승인 실패",
          color: "red",
        });
        return;
      }
    }
    setPending((p) => p.filter((x) => x !== id));
    setOperators((o) => [...o, { id, role: "operator" }]);
    notifications.show({ message: `${id} 승인됨`, color: "green" });
  };
  const reject = async (id: string) => {
    try {
      await api.operators.reject(id);
    } catch (e) {
      if (!isOffline(e)) {
        notifications.show({
          message: e instanceof Error ? e.message : "거절 실패",
          color: "red",
        });
        return;
      }
    }
    setPending((p) => p.filter((x) => x !== id));
    notifications.show({ message: `${id} 거절됨`, color: "gray" });
  };
  const remove = async (id: string) => {
    try {
      await api.operators.remove(id);
    } catch (e) {
      if (!isOffline(e)) {
        notifications.show({
          message: e instanceof Error ? e.message : "삭제 실패",
          color: "red",
        });
        return;
      }
    }
    setOperators((o) => o.filter((x) => x.id !== id));
    notifications.show({ message: `${id} 삭제됨`, color: "red" });
  };
  // 사수 확정(PR#324): 하위 운영자 비번을 SuperAdmin이 새 값으로 재설정(토큰버전 +1 → 옛 토큰 무효).
  const resetPw = async (id: string) => {
    const newPw = window.prompt(`${id}의 새 비밀번호를 입력하세요`);
    if (newPw == null || newPw.length < 4) {
      if (newPw != null) {
        notifications.show({
          message: "비밀번호가 너무 짧습니다",
          color: "red",
        });
      }
      return;
    }
    try {
      await api.operators.resetPassword(id, newPw);
      notifications.show({
        message: `${id} 비밀번호를 재설정했어요`,
        color: "green",
      });
    } catch (e) {
      if (isOffline(e)) {
        notifications.show({
          message: `${id} 비밀번호 재설정(미리보기)`,
          color: "green",
        });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "재설정 실패",
          color: "red",
        });
      }
    }
  };

  return (
    <Box p="lg">
      <Group justify="space-between" mb="md">
        <Text fw={800} size="xl">
          운영자 관리
        </Text>
        <Text size="sm" c="dimmed">
          권한은 전원 동일 · 운영자 삭제·비번 재설정만 SuperAdmin 전용 (§5)
        </Text>
      </Group>

      <Stack gap="md">
        {/* 가입 승인 대기 */}
        <Paper withBorder radius="md" p="lg">
          <Group gap="xs" mb="sm">
            <Text fw={700} size="lg">
              가입 승인 대기
            </Text>
            <Badge variant="light" color="orange" radius="sm">
              {pending.length}
            </Badge>
          </Group>
          <Stack gap="xs">
            {pending.length === 0 && (
              <Text c="dimmed" size="sm">
                대기 중인 가입 신청이 없습니다.
              </Text>
            )}
            {pending.map((id) => (
              <Paper key={id} withBorder radius="md" p="sm">
                <Group justify="space-between">
                  <Group gap="sm">
                    <ThemeIcon
                      size={34}
                      radius="xl"
                      variant="light"
                      color="orange"
                    >
                      <Icon.users size={18} />
                    </ThemeIcon>
                    <Text fw={600}>{id}</Text>
                  </Group>
                  <Group gap="xs">
                    <Button
                      size="xs"
                      color="green"
                      leftSection={<Icon.check size={14} />}
                      onClick={() => void approve(id)}
                    >
                      승인
                    </Button>
                    <Button
                      size="xs"
                      variant="light"
                      color="gray"
                      onClick={() => void reject(id)}
                    >
                      거절
                    </Button>
                  </Group>
                </Group>
              </Paper>
            ))}
          </Stack>
        </Paper>

        {/* 운영자 목록 */}
        <Paper withBorder radius="md" p="lg">
          <Group gap="xs" mb="sm">
            <Text fw={700} size="lg">
              운영자 목록
            </Text>
            <Badge variant="light" color="gray" radius="sm">
              {operators.length}
            </Badge>
          </Group>
          <Stack gap="xs">
            {operators.map((op) => {
              const isSuper = op.role === "super";
              return (
                <Paper key={op.id} withBorder radius="md" p="sm">
                  <Group justify="space-between">
                    <Group gap="sm">
                      <ThemeIcon
                        size={34}
                        radius="xl"
                        variant="light"
                        color={isSuper ? "blue" : "gray"}
                      >
                        <Icon.users size={18} />
                      </ThemeIcon>
                      <Text fw={600}>{op.id}</Text>
                      {isSuper && (
                        <Badge color="blue" variant="light" radius="sm">
                          SuperAdmin
                        </Badge>
                      )}
                    </Group>
                    <Group gap="xs">
                      {/* 비번 재설정: 하위 운영자 행 + SuperAdmin에게만(사수 확정 PR#324, §5). */}
                      {!isSuper && canSuper && (
                        <Button
                          size="xs"
                          variant="light"
                          color="blue"
                          leftSection={<Icon.refresh size={14} />}
                          onClick={() => void resetPw(op.id)}
                        >
                          비번 재설정
                        </Button>
                      )}
                      <Button
                        size="xs"
                        color="red"
                        variant="light"
                        disabled={isSuper}
                        leftSection={<Icon.trash size={14} />}
                        onClick={() => void remove(op.id)}
                      >
                        {isSuper ? "삭제 불가" : "삭제"}
                      </Button>
                    </Group>
                  </Group>
                </Paper>
              );
            })}
          </Stack>
          <Text size="xs" c="dimmed" mt="sm">
            ※ ‘삭제’·‘비번 재설정’은 SuperAdmin 전용이며, SuperAdmin 계정 자체는
            삭제·재설정 대상이 아닙니다(본인이 직접 변경·종이 보관, §5).
          </Text>
        </Paper>
      </Stack>
    </Box>
  );
}
