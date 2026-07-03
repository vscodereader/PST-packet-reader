import {
  ActionIcon,
  Badge,
  Box,
  Group,
  Paper,
  Stack,
  Text,
  ThemeIcon,
} from "@mantine/core";
import { IconTrash } from "@tabler/icons-react";
import { useCallback, useEffect, useState } from "react";

import { scheduleMoment } from "@/shared/schedule";
import { Icon } from "@/shared/ui/icons";

import { api } from "../../api";

// 예약된 글 1건. 게시 명령 화면에서 '예약'하면 여기에 쌓이고, 예약 시각이 되면(AdminApp 타이머)
// **게시되어 즉시 목록에서 사라진다**(게시 큐처럼 — 다른 화면 다녀올 필요 없음). 휴지통=예약 취소.
export interface ScheduledItem {
  id: string;
  deviceName: string;
  postTitle: string;
  targetLabel: string;
  detail: string;
  at: number; // 예약 시각(epoch ms)
}

const pad2 = (n: number) => String(n).padStart(2, "0");
function fmtAt(at: number): string {
  const d = new Date(at);
  const date = `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
  const time = `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  return `${scheduleMoment(date, time).when} (${date})`;
}

export function ScheduledPosts({
  items,
  onRemove,
}: {
  items: ScheduledItem[];
  onRemove: (id: string) => void;
}) {
  // 서버가 예약을 보관·발송한다(4단계). 서버 목록을 2초마다 폴링해 실데이터로 렌더하고, 시각이
  // 되면 서버 스케줄러가 발송·제거하므로 목록에서 자동으로 사라진다. 서버 오프라인이면 로컬 폴백
  // (미리보기 무손상): server=null 이면 상위가 준 로컬 items/onRemove를 그대로 쓴다.
  const [server, setServer] = useState<ScheduledItem[] | null>(null);

  const refresh = useCallback(() => {
    api.scheduled
      .list()
      .then((list) => setServer(list))
      .catch(() => setServer(null)); // 오프라인 → 로컬 폴백
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [refresh]);

  const online = server != null;
  const list = server ?? items;
  const remove = (id: string) => {
    if (online) {
      api.scheduled
        .remove(id)
        .then(refresh)
        .catch(() => {
          /* 실패해도 다음 폴링이 상태를 맞춘다 */
        });
    } else {
      onRemove(id);
    }
  };

  const sorted = [...list].sort((a, b) => a.at - b.at);
  return (
    <Stack gap="md" p="md" h="100%">
      <Box>
        <Text fw={800} size="xl">
          예약된 글
        </Text>
        <Text size="sm" c="dimmed">
          예약 시각이 되면 게시되어 목록에서 바로 사라집니다. 휴지통으로 예약을
          취소할 수 있어요.
        </Text>
      </Box>

      {sorted.length === 0 ? (
        <Text c="dimmed" size="sm">
          예약된 글이 없습니다. ‘게시 명령’에서 예약하면 여기 쌓입니다.
        </Text>
      ) : (
        <Stack gap="xs">
          {sorted.map((it) => (
            <Paper key={it.id} withBorder radius="md" p="sm">
              <Group justify="space-between" wrap="nowrap">
                <Group gap="sm" wrap="nowrap" style={{ minWidth: 0 }}>
                  <ThemeIcon
                    size={34}
                    radius="md"
                    variant="light"
                    color="grape"
                  >
                    <Icon.calendar size={18} />
                  </ThemeIcon>
                  <Box style={{ minWidth: 0 }}>
                    <Group gap={6} wrap="nowrap">
                      <Text fw={700} size="sm" truncate>
                        {it.postTitle}
                      </Text>
                      <Badge size="xs" variant="light">
                        {it.targetLabel}
                      </Badge>
                    </Group>
                    <Text size="xs" c="dimmed" truncate>
                      {it.deviceName} · {it.detail}
                    </Text>
                  </Box>
                </Group>
                <Group gap="sm" wrap="nowrap">
                  <Badge color="grape" variant="light" radius="sm">
                    {fmtAt(it.at)}
                  </Badge>
                  <ActionIcon
                    variant="subtle"
                    color="red"
                    title="예약 취소"
                    onClick={() => remove(it.id)}
                  >
                    <IconTrash size={18} />
                  </ActionIcon>
                </Group>
              </Group>
            </Paper>
          ))}
        </Stack>
      )}
    </Stack>
  );
}
