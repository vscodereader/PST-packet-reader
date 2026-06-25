import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Center,
  Container,
  Group,
  Loader,
  NumberInput,
  Paper,
  Stack,
  Text,
  ThemeIcon,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useRef, useState } from "react";

import { SubLog } from "@/features/notifications/notifications";
import { KIND, KIND_ICON } from "@/shared/data/config";
import type {
  GoFn,
  PlatformId,
  QueueLocation,
  QueueNowItem,
  QueueScheduledItem,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";
import { PlatformPill } from "@/shared/ui/platform-logo";

// 실행 중(running) 아이템은 워커가 처리 중이라 맨 앞에 고정한다. 단 실행 중인 게
// 없으면(워커 idle) 첫 대기 아이템도 자유롭게 옮길 수 있어야 하므로, index 0을 무조건
// 막지 않고 "선두 running 개수"만큼만 고정한다(백엔드 apply_reorder_now와 일치).
const pinnedCount = (list: QueueNowItem[]) =>
  list[0]?.state === "running" ? 1 : 0;

function LocSummary({
  locs,
  size = 18,
}: {
  locs: QueueLocation[];
  size?: number;
}) {
  const plats: PlatformId[] = [];
  locs.forEach((l) => {
    if (!plats.includes(l.p)) plats.push(l.p);
  });
  const label =
    locs.length === 1
      ? locs[0]?.name
      : `${locs[0]?.name} 외 ${locs.length - 1}곳`;
  return (
    <Group gap={8} wrap="nowrap" style={{ minWidth: 0 }}>
      <PlatformPill ids={plats} size={size} />
      <Text fz={12} c="dimmed" truncate>
        {label}
      </Text>
    </Group>
  );
}

/**
 * Inline re-schedule control for a "missed" item: a date/time picker seeded to
 * "now" plus a confirm button. Local state keeps the pick until the user commits
 * so we don't fire a reschedule on every adjustment.
 */
function RescheduleControl({
  onSubmit,
}: {
  onSubmit: (date: string, time: string) => void;
}) {
  const [date, setDate] = useState(() => nowParts().date);
  const [time, setTime] = useState(() => nowParts().time);
  // 사용자가 picker를 건드리지 않아 시드("지금")가 현재보다 과거가 됐으면(컨트롤이 오래
  // 열려 있던 경우) 제출 시 현재 시각으로 올려, 백엔드 과거-시각 거부를 피한다.
  const submit = () => {
    const fresh = nowParts();
    if (toEpochMs(date, time) < toEpochMs(fresh.date, fresh.time)) {
      onSubmit(fresh.date, fresh.time);
    } else {
      onSubmit(date, time);
    }
  };
  return (
    <Group gap={6} wrap="nowrap">
      <DateTimePicker
        date={date}
        time={time}
        onChange={(v) => {
          setDate(v.date);
          setTime(v.time);
        }}
      />
      <Button size="sm" color="blue" onClick={submit}>
        재예약
      </Button>
    </Group>
  );
}

export function Queue({ go }: { go: GoFn }) {
  const [now, setNow] = useState<QueueNowItem[]>([]);
  const [sched, setSched] = useState<QueueScheduledItem[]>([]);
  const [dragId, setDragId] = useState<string | null>(null);
  // 진행 중 아이템을 클릭하면 그 자리에서 대상별 상태(items)를 펼친다(#219). 보통 실행
  // 중 아이템은 1개라 단일 id로 충분하다.
  const [expandedId, setExpandedId] = useState<string | null>(null);
  // "최대 작동가능 작업 수"(now 큐 동시 작업 상한, #284). 빈 문자열 = 무제한(0). 마운트 시
  // 백엔드에서 현재 값을 읽어 채우고, 저장 시 set_now_concurrency_limit으로 영속화한다.
  const [concurrency, setConcurrency] = useState<number | "">("");

  // 폴링/이벤트 콜백에서 최신 값을 읽기 위한 ref (stale closure 회피).
  const nowRef = useRef<QueueNowItem[]>(now);
  const dragIdRef = useRef<string | null>(dragId);
  // 순서 영속화(reorderNow)가 끝나기 전에 폴링이 낙관적 순서를 덮어쓰지 않도록 막는다.
  const persistingRef = useRef(false);
  useEffect(() => {
    nowRef.current = now;
    dragIdRef.current = dragId;
  }, [now, dragId]);

  useEffect(() => {
    // in-flight Promise가 언마운트 후 resolve돼 unmounted setState가 되지 않도록
    // alive 가드를 둔다. cleanup에서 false로 만들어 이후 콜백을 무시한다.
    let alive = true;
    void ipc.queue.listNow().then((v) => {
      if (alive) setNow(v);
    });
    void ipc.queue.listScheduled().then((v) => {
      if (alive) setSched(v);
    });
    // 저장된 "최대 작동가능 작업 수"를 읽어 입력란에 채운다(0 = 무제한 → 빈 칸).
    void ipc.queue.getConcurrencyLimit().then((v) => {
      if (alive) setConcurrency(v > 0 ? v : "");
    });
    // 워커 진행률·상태를 주기적으로 반영. 단 드래그 중이거나 순서 영속화 대기 중에는
    // 사용자가 맞춘 로컬 순서를 덮어쓰지 않도록 폴링을 건너뛴다.
    const timer = window.setInterval(() => {
      // 예약 큐는 드래그 대상이 아니므로 항상 폴링한다 — 스케줄러의 자동 게시(예약→now
      // 이동)와 "놓침" 표시가 화면에 실시간 반영되게 한다.
      void ipc.queue.listScheduled().then((v) => {
        if (alive) setSched(v);
      });
      if (dragIdRef.current !== null || persistingRef.current) return;
      void ipc.queue.listNow().then((v) => {
        if (alive) setNow(v);
      });
      // 워커가 글/댓글 1건마다 진행률을 올리므로(0/N→1/N→…), 빠른 작업도 중간 진행이 보이도록
      // 비교적 촘촘히(750ms) 폴링한다. now 큐 조회는 로컬 JSON 스토어 읽기라 비용이 작다.
    }, 750);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);

  // 대기열 순서를 백엔드에 영속화한다. 응답이 올 때까지 폴링을 막아(persistingRef)
  // 진행 중인 변경이 되돌려지지 않게 한다. drag(onDragEnd)와 화살표(move) 공통 경로.
  const persistOrder = (orderedIds: string[]) => {
    persistingRef.current = true;
    void ipc.queue
      .reorderNow(orderedIds)
      .then(setNow)
      .catch(() => {
        // 영속화 실패 시 낙관적 순서가 백엔드와 어긋난 채 남지 않도록 현재
        // 순서를 다시 불러와 되돌리고, 실패를 사용자에게 알린다.
        void ipc.queue.listNow().then(setNow);
        notifications.show({
          message: "순서 변경을 저장하지 못했어요",
          color: "red",
        });
      })
      .finally(() => {
        persistingRef.current = false;
      });
  };

  const reorder = (id: string, targetId: string) => {
    setNow((list) => {
      const from = list.findIndex((x) => x.id === id);
      const to = list.findIndex((x) => x.id === targetId);
      const pinned = pinnedCount(list);
      if (from < pinned || to < pinned || from === to) return list;
      const copy = [...list];
      const [m] = copy.splice(from, 1);
      if (m) copy.splice(to, 0, m);
      return copy;
    });
  };
  const move = (id: string, dir: -1 | 1) => {
    // reorder()와 동일하게 함수형 업데이트 안에서 스왑해 빠른 연속 클릭 시 stale
    // 리스트로 동작하지 않게 한다. 스왑 결과는 바깥 변수에 잡아 persistOrder에 넘긴다.
    let swapped: QueueNowItem[] | null = null;
    setNow((list) => {
      const pinned = pinnedCount(list);
      const i = list.findIndex((x) => x.id === id);
      const j = i + dir;
      if (i < pinned || j < pinned || j >= list.length) return list;
      const copy = [...list];
      const a = copy[i];
      const b = copy[j];
      if (!a || !b) return list;
      copy[i] = b;
      copy[j] = a;
      swapped = copy;
      return copy;
    });
    if (swapped) persistOrder((swapped as QueueNowItem[]).map((x) => x.id));
  };
  const cancel = (id: string) => {
    void ipc.queue.cancelNow(id).then(setNow);
    notifications.show({ message: "대기 작업을 취소했어요", color: "blue" });
  };
  const promote = (id: string) => {
    void ipc.queue.promote(id).then((next) => {
      setNow(next);
      void ipc.queue.listScheduled().then(setSched);
    });
    notifications.show({
      message: "예약을 즉시 대기열로 옮겼어요",
      color: "green",
    });
  };
  const cancelScheduled = (id: string) => {
    void ipc.queue.cancelScheduled(id).then(setSched);
    notifications.show({ message: "예약을 취소했어요", color: "blue" });
  };
  // 놓친 예약을 새 시각으로 되살린다(또는 대기 예약의 시각 변경). 백엔드가 과거 시각을
  // 거부하면 알림으로 알린다.
  const reschedule = (id: string, date: string, time: string) => {
    const m = scheduleMoment(date, time);
    void ipc.queue
      .reschedule(id, toEpochMs(date, time), m.when, m.label)
      .then((next) => {
        setSched(next);
        notifications.show({
          message: "예약 시각을 변경했어요",
          color: "green",
        });
      })
      .catch(() => {
        notifications.show({
          message: "지난 시각으로는 예약할 수 없어요",
          color: "red",
        });
      });
  };

  // "최대 작동가능 작업 수"를 저장한다(#284). 빈 칸/0 = 무제한. 워커는 claim 시점마다
  // 다시 읽으므로 낮춰도 이미 돌고 있는 작업은 멈추지 않는다.
  const saveConcurrency = () => {
    const limit = concurrency === "" ? 0 : Math.max(0, Math.trunc(concurrency));
    void ipc.queue
      .setConcurrencyLimit(limit)
      .then(() => {
        notifications.show({
          message:
            limit === 0
              ? "최대 작동가능 작업 수를 무제한으로 저장했어요"
              : `최대 작동가능 작업 수를 ${limit}개로 저장했어요`,
          color: "green",
        });
      })
      .catch(() => {
        notifications.show({
          message: "최대 작동가능 작업 수를 저장하지 못했어요",
          color: "red",
        });
      });
  };

  // 대기 순번 계산용: 실행 중이 아닌 순수 대기 아이템만. 완료/실패/도중 차단 아이템은
  // finish_item이 큐에서 제거하므로(결과는 알림에서 확인), 큐엔 실행 중·대기만 남는다.
  const waiting = now.filter((q) => q.state !== "running");

  return (
    <Container size={980} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={24} wrap="wrap">
        <Box>
          <Title order={1} fz={25} fw={800}>
            게시 큐
          </Title>
          <Text size="sm" c="dimmed" mt={6}>
            즉시 게시 작업은 대기열에 쌓여 위에서부터 처리돼요. 드래그해서
            우선순위를 바꾸세요.
          </Text>
        </Box>
        <Button
          size="sm"
          leftSection={<Icon.pencil size={17} />}
          onClick={() => go("posts")}
        >
          새 작업 추가
        </Button>
      </Group>

      <Group gap={8} mb={12}>
        <Box w={7} h={7} bg="green" style={{ borderRadius: 999 }} />
        <Text fz={13} fw={700} c="gray.7">
          즉시 처리 대기열
        </Text>
        <Text fz={12} c="dimmed">
          {now.length}건
        </Text>
        <Group gap={8} ml="auto">
          <Icon.gripper size={14} color="var(--mantine-color-gray-5)" />
          <Text fz={12} c="dimmed">
            드래그로 순서 변경
          </Text>
        </Group>
      </Group>

      <Stack gap={8} mb={34}>
        {now.map((q) => {
          // 차단되어 멈춘 종료성 카드(#REQ1)는 state가 running이라도 더 돌지 않는다 — 스피너·
          // 큐엔 실행 중(live)·대기 아이템만 있다(완료/실패/도중 차단은 finish_item이 큐에서
          // 제거하고 결과는 알림에서 확인). 진행중 배지·드래그 잠금은 live 아이템에만 적용한다.
          const running = q.state === "running";
          const live = running;
          const kd = KIND[q.kind] ?? { t: q.kind, c: "gray" };
          const KI =
            Icon[(KIND_ICON[q.kind] ?? "fileText") as keyof typeof Icon];
          // 로그인 "전용" 아이템(plan.login만 있고 게시 타깃 전무)만 "로그인"으로 표시한다.
          // 게시 아이템도 이제 plan.login을 동봉하므로(#225, 게시 직전 계정별 로그인),
          // 게시 타깃이 있으면 글/댓글로 표시한다 — 백엔드 execute_item의 login-only 판정과 동일.
          const isLogin =
            (q.plan?.login?.length ?? 0) > 0 &&
            (q.plan?.naver?.length ?? 0) === 0 &&
            (q.plan?.forum?.length ?? 0) === 0 &&
            (q.plan?.band?.length ?? 0) === 0;
          const dragging = dragId === q.id;
          // 실행 중 카드는 대기 순번이 없다 → 순수 대기 아이템만 순번을 매긴다.
          const order = running
            ? null
            : waiting.findIndex((w) => w.id === q.id) + 1;
          // live 카드는 클릭하면 대상별 진행 상태(items)를 펼쳐 볼 수 있다.
          const expanded = live && expandedId === q.id;
          return (
            <Box key={q.id}>
              <Paper
                withBorder
                radius="md"
                draggable={!running}
                onClick={
                  live
                    ? () =>
                        setExpandedId((prev) => (prev === q.id ? null : q.id))
                    : undefined
                }
                onDragStart={(e) => {
                  setDragId(q.id);
                  // 폴링 가드(dragIdRef.current !== null)가 즉시 막도록 ref도 동기 세팅.
                  // setDragId 반영용 effect는 이번 tick 이후라 그 사이 폴링이 순서를
                  // 덮어쓰는 것을 막는다.
                  dragIdRef.current = q.id;
                  e.dataTransfer.effectAllowed = "move";
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  if (dragId && dragId !== q.id) reorder(dragId, q.id);
                }}
                onDragEnd={() => {
                  const dragged = dragId !== null;
                  setDragId(null);
                  dragIdRef.current = null;
                  // 드래그로 바뀐 최종 순서를 백엔드에 영속화한다.
                  if (dragged) persistOrder(nowRef.current.map((x) => x.id));
                }}
                title={live ? "클릭하면 대상별 진행 상태 펼치기" : undefined}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 14,
                  padding: "13px 14px 13px 10px",
                  borderColor: live
                    ? "var(--mantine-color-blue-filled)"
                    : undefined,
                  background: live
                    ? "var(--mantine-color-blue-light)"
                    : undefined,
                  opacity: dragging ? 0.5 : 1,
                  cursor: live ? "pointer" : "grab",
                }}
              >
                <Box
                  w={30}
                  style={{
                    flexShrink: 0,
                    display: "flex",
                    flexDirection: "column",
                    alignItems: "center",
                    gap: 2,
                  }}
                >
                  {live ? (
                    <Loader size={18} />
                  ) : (
                    <>
                      <Icon.gripper
                        size={18}
                        color="var(--mantine-color-gray-5)"
                      />
                      <Text fz={11} fw={800} c="dimmed" ff="monospace">
                        {order}
                      </Text>
                    </>
                  )}
                </Box>

                <ThemeIcon
                  size={36}
                  radius="md"
                  variant="light"
                  color={
                    isLogin ? "violet" : q.kind === "comment" ? "forum" : "gray"
                  }
                >
                  <KI size={18} />
                </ThemeIcon>

                <Box style={{ flex: 1, minWidth: 0 }}>
                  <Group gap={8} mb={5} wrap="nowrap">
                    <Badge
                      size="sm"
                      color={isLogin ? "violet" : kd.c}
                      variant="light"
                    >
                      {isLogin ? "로그인" : kd.t}
                    </Badge>
                    <Text fz={14} fw={700} truncate>
                      {q.title}
                    </Text>
                  </Group>
                  <LocSummary locs={q.locs} />
                </Box>

                {live ? (
                  <Group gap={8} wrap="nowrap">
                    <Badge size="sm" color="blue" variant="light">
                      {(() => {
                        // progress가 아직 없으면 "처리중 /"로 깨지지 않도록 0/0 폴백.
                        const [d, t] = q.progress ?? [0, 0];
                        return `처리중 ${d}/${t}`;
                      })()}
                    </Badge>
                    <Icon.chevronDown
                      size={17}
                      color="var(--mantine-color-blue-filled)"
                      style={{
                        transform: expanded ? "rotate(180deg)" : "none",
                        transition: "transform .18s",
                      }}
                    />
                  </Group>
                ) : (
                  <Group gap={8} wrap="nowrap">
                    <Text fz={11.5} c="dimmed">
                      {q.locs.length}곳 대기
                    </Text>
                    <Stack gap={1}>
                      <ActionIcon
                        size="sm"
                        variant="subtle"
                        color="gray"
                        title="우선순위 올리기"
                        onClick={() => move(q.id, -1)}
                      >
                        <Icon.chevronUp size={15} />
                      </ActionIcon>
                      <ActionIcon
                        size="sm"
                        variant="subtle"
                        color="gray"
                        title="우선순위 내리기"
                        onClick={() => move(q.id, 1)}
                      >
                        <Icon.chevronDown size={15} />
                      </ActionIcon>
                    </Stack>
                    <ActionIcon
                      size="md"
                      variant="subtle"
                      color="gray"
                      title="취소"
                      onClick={() => cancel(q.id)}
                    >
                      <Icon.x size={17} />
                    </ActionIcon>
                  </Group>
                )}
              </Paper>
              {/* 진행 중 아이템을 펼치면 대상별 상태(진행 전/중/완료/실패)를 알림 로그처럼
                SubLog로 보여준다(#219). 아직 항목이 없으면(막 시작) 안내 문구를 둔다. */}
              {expanded && (
                <Paper
                  withBorder
                  radius="md"
                  mt={4}
                  style={{ overflow: "hidden" }}
                >
                  {q.items.length > 0 ? (
                    q.items.map((it, i) => <SubLog key={i} item={it} />)
                  ) : (
                    <Text fz={12} c="dimmed" ta="center" py={14}>
                      진행 상태를 준비하고 있어요…
                    </Text>
                  )}
                </Paper>
              )}
            </Box>
          );
        })}
        {now.length === 0 && (
          <Paper
            withBorder
            radius="md"
            style={{ borderStyle: "dashed" }}
            py={44}
          >
            <Center>
              <Stack align="center" gap={8}>
                <Icon.check size={32} color="var(--mantine-color-gray-5)" />
                <Text size="sm" fw={600} c="dimmed">
                  대기 중인 즉시 작업이 없어요
                </Text>
              </Stack>
            </Center>
          </Paper>
        )}
      </Stack>

      {/* 최대 작동가능 작업 수(now 큐 동시 작업 상한, #284). 빈 칸/0 = 무제한. 저장하면
          즉시 set IPC로 영속화되고, 워커가 claim 시점마다 새 한도를 읽는다. */}
      <Paper withBorder radius="md" p="md" mb={34}>
        <Group justify="space-between" wrap="wrap" gap={12}>
          <Box>
            <Text fz={13} fw={700} c="gray.7">
              최대 작동가능 작업 수
            </Text>
            <Text fz={12} c="dimmed" mt={2}>
              동시에 처리할 작업 수예요. 비우거나 0이면 무제한이에요.
            </Text>
          </Box>
          <Group gap={8} wrap="nowrap">
            <NumberInput
              w={140}
              min={0}
              step={1}
              allowNegative={false}
              allowDecimal={false}
              placeholder="무제한"
              value={concurrency}
              onChange={(v) => setConcurrency(v === "" ? "" : Number(v))}
              aria-label="최대 작동가능 작업 수"
            />
            <Button size="sm" onClick={saveConcurrency}>
              저장
            </Button>
          </Group>
        </Group>
      </Paper>

      <Group gap={8} mb={12}>
        <Icon.calendar size={15} color="var(--mantine-color-gray-6)" />
        <Text fz={13} fw={700} c="gray.7">
          예약 대기
        </Text>
        <Text fz={12} c="dimmed">
          {sched.length}건
        </Text>
      </Group>
      <Stack gap={8}>
        {sched.map((q) => {
          const kd = KIND[q.kind] ?? { t: q.kind, c: "gray" };
          return (
            <Paper
              key={q.id}
              withBorder
              radius="md"
              p="md"
              style={{ display: "flex", alignItems: "center", gap: 14 }}
            >
              <Box
                w={64}
                ta="center"
                style={{
                  flexShrink: 0,
                  borderRight: "1px solid var(--mantine-color-gray-2)",
                  paddingRight: 12,
                }}
              >
                <Text fz={16} fw={800}>
                  {q.when.split(" ").pop()}
                </Text>
                <Text fz={11} c="dimmed" fw={600} mt={2}>
                  {q.rel}
                </Text>
              </Box>
              <Box style={{ flex: 1, minWidth: 0 }}>
                <Group gap={8} mb={5} wrap="nowrap">
                  <Badge size="sm" color={kd.c} variant="light">
                    {kd.t}
                  </Badge>
                  <Text fz={14} fw={700} truncate>
                    {q.title}
                  </Text>
                </Group>
                <LocSummary locs={q.locs} />
              </Box>
              {q.missed ? (
                <>
                  <Badge size="sm" color="red" variant="light">
                    놓침
                  </Badge>
                  <RescheduleControl
                    onSubmit={(date, time) => reschedule(q.id, date, time)}
                  />
                </>
              ) : (
                <>
                  <Badge size="sm" color="yellow" variant="light">
                    예약됨
                  </Badge>
                  <Button
                    size="sm"
                    variant="default"
                    leftSection={<Icon.bolt size={14} />}
                    onClick={() => promote(q.id)}
                  >
                    즉시 처리
                  </Button>
                </>
              )}
              <ActionIcon
                size="md"
                variant="subtle"
                color="gray"
                title="예약 취소"
                onClick={() => cancelScheduled(q.id)}
              >
                <Icon.x size={17} />
              </ActionIcon>
            </Paper>
          );
        })}
        {sched.length === 0 && (
          <Paper
            withBorder
            radius="md"
            style={{ borderStyle: "dashed" }}
            py={44}
          >
            <Center>
              <Stack align="center" gap={8}>
                <Icon.check size={32} color="var(--mantine-color-gray-5)" />
                <Text size="sm" fw={600} c="dimmed">
                  예약 중인 작업이 없어요
                </Text>
              </Stack>
            </Center>
          </Paper>
        )}
      </Stack>
    </Container>
  );
}
