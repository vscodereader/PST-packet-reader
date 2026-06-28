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
import { useState } from "react";

import { Icon } from "@/shared/ui/icons";

// 운영자 관리(§5). 가입 승인(승인된 운영자 누구나) + 운영자 삭제(SuperAdmin 전용,
// SuperAdmin 계정은 삭제 불가). 권한은 전원 동일(차등 없음).

interface Operator {
  id: string;
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

  const approve = (id: string) => {
    setPending((p) => p.filter((x) => x !== id));
    setOperators((o) => [...o, { id, role: "operator" }]);
    notifications.show({ message: `${id} 승인됨`, color: "green" });
  };
  const reject = (id: string) => {
    setPending((p) => p.filter((x) => x !== id));
    notifications.show({ message: `${id} 거절됨`, color: "gray" });
  };
  const remove = (id: string) => {
    setOperators((o) => o.filter((x) => x.id !== id));
    notifications.show({ message: `${id} 삭제됨`, color: "red" });
  };

  return (
    <Box p="lg">
      <Group justify="space-between" mb="md">
        <Text fw={800} size="xl">
          운영자 관리
        </Text>
        <Text size="sm" c="dimmed">
          권한은 전원 동일 · 운영자 삭제만 SuperAdmin 전용 (§5)
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
                      onClick={() => approve(id)}
                    >
                      승인
                    </Button>
                    <Button
                      size="xs"
                      variant="light"
                      color="gray"
                      onClick={() => reject(id)}
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
                    <Button
                      size="xs"
                      color="red"
                      variant="light"
                      disabled={isSuper}
                      leftSection={<Icon.trash size={14} />}
                      onClick={() => remove(op.id)}
                    >
                      {isSuper ? "삭제 불가" : "삭제"}
                    </Button>
                  </Group>
                </Paper>
              );
            })}
          </Stack>
          <Text size="xs" c="dimmed" mt="sm">
            ※ ‘삭제’ 버튼은 SuperAdmin에게만 보이는 동작이며, SuperAdmin 계정
            자체는 삭제할 수 없습니다(§5).
          </Text>
        </Paper>
      </Stack>
    </Box>
  );
}
