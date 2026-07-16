import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Group,
  Modal,
  MultiSelect,
  Paper,
  ScrollArea,
  Select,
  Table,
  Text,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconTrash } from "@tabler/icons-react";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";

import { ACTIVE_PLATFORMS } from "@/shared/data/config";

import { api, isOffline } from "../../api";
import { maskId } from "../publish-command/publish-command";

// 계정 상태 관리 화면(14-계정상태-관리). 하위 COM을 하나 골라, 그 하위가 보고한 accountRows
// (loginId·platform·status)를 4초 폴링으로 표시하고, 각 계정의 플랫폼·상태를 바꿔 저장하면
// 폴링본 대비 바뀐 행만 모아 하위에 명령을 보낸다(양방향: 읽기=accountRows 재사용, 쓰기=신규).

// 상태 선택지 = 사람이 되돌릴 수 있는 3종만(§5). 그 외(badCredentials/blocked 등)는 워커 판정값이라
// 드롭다운에서 제외하고 읽기 전용 배지로만 보여준다.
export const STATUS_OPTS: { value: string; label: string }[] = [
  { value: "active", label: "활성" },
  { value: "waiting", label: "대기" },
  { value: "onHold", label: "보류" },
];
const STATUS_LABEL: Record<string, string> = {
  active: "활성",
  waiting: "대기",
  onHold: "보류",
  new: "신규",
  timedOut: "대기초과",
  badCredentials: "비번오류",
  challenge: "추가인증",
  relogin: "재로그인",
  blocked: "차단",
  error: "오류",
};
const PLATFORM_OPTS = ACTIVE_PLATFORMS.map((p) => ({
  value: p.id,
  label: p.name,
}));

// 하위 COM별 구분 색(선택 순서대로 배정) — 표에서 어느 컴퓨터 계정인지 색·구분선으로 가른다.
const DEVICE_COLORS = [
  "orange",
  "teal",
  "grape",
  "blue",
  "pink",
  "cyan",
  "lime",
  "indigo",
] as const;

/** 상태가 사람이 되돌릴 수 있는 값(active/waiting/onHold)인지 — 드롭다운 노출 판정(§5). */
export function isReversibleStatus(status: string): boolean {
  return STATUS_OPTS.some((o) => o.value === status);
}
/** 상태 문자열 → 표시 라벨(미상이면 원문). */
export function statusLabel(status: string): string {
  return STATUS_LABEL[status] ?? status;
}

export interface AccountRow {
  loginId: string;
  platform: string;
  status: string;
}
export interface MetaUpdate {
  loginId: string;
  platform?: string;
  status?: string;
}

/**
 * 폴링본(original) 대비 편집본(edited)에서 **바뀐 행만** 추출한다(§2 저장, 순수 함수). loginId로
 * 매칭해 platform·status를 각각 비교하고, 바뀐 필드만 담아 반환한다(둘 다 안 바뀌면 제외). edited에만
 * 있고 original에 없는 loginId는 무시한다(존재하는 계정만 편집).
 */
export function diffAccountRows(
  original: AccountRow[],
  edited: AccountRow[],
): MetaUpdate[] {
  const origByLogin = new Map(original.map((r) => [r.loginId, r]));
  const updates: MetaUpdate[] = [];
  for (const row of edited) {
    const base = origByLogin.get(row.loginId);
    if (base === undefined) continue;
    const update: MetaUpdate = { loginId: row.loginId };
    let changed = false;
    if (row.platform !== base.platform) {
      update.platform = row.platform;
      changed = true;
    }
    if (row.status !== base.status) {
      update.status = row.status;
      changed = true;
    }
    if (changed) updates.push(update);
  }
  return updates;
}

interface OnlineDevice {
  id: string;
  name: string;
}

// 오프라인 미리보기 더미(서버 미기동 시 화면 무손상) — 다른 Admin 화면과 동일 방침.
const INITIAL_DEVICES: OnlineDevice[] = [
  { id: "d1", name: "하위-001" },
  { id: "d3", name: "하위-003" },
];
const INITIAL_ROWS: Record<string, AccountRow[]> = {
  d1: [
    { loginId: "invest_king7", platform: "forum", status: "waiting" },
    { loginId: "blog_press02", platform: "blog", status: "active" },
    { loginId: "onhold_user1", platform: "forum", status: "onHold" },
    { loginId: "blocked_zz9", platform: "naver", status: "blocked" },
  ],
  d3: [{ loginId: "clip_creator", platform: "clip", status: "active" }],
};

interface DisplayRow extends AccountRow {
  deviceId: string;
  deviceName: string;
}

export function AccountState() {
  const [devices, setDevices] = useState<OnlineDevice[]>(INITIAL_DEVICES);
  // 여러 하위 COM 동시 선택(2026-07-16). 선택한 모든 하위의 계정을 한 표에 함께 보여주고, 저장 시
  // 하위별로 각자의 변경만 따로 명령을 보낸다(B를 바꿨는데 A로 명령이 가는 일 없음).
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  // 폴링본(하위별 원래 값, deviceId → rows). 저장 diff의 기준.
  const [original, setOriginal] = useState<Record<string, AccountRow[]>>({});
  // 편집 오버라이드(하위별·계정별: deviceId → loginId → 바꾼 platform/status). loginId가 하위 간
  // 겹쳐도 섞이지 않게 deviceId로 먼저 나눈다.
  const [edits, setEdits] = useState<
    Record<string, Record<string, { platform?: string; status?: string }>>
  >({});
  const [saving, setSaving] = useState(false);
  // 삭제 확인 대상(어느 하위의 어느 계정인지) — null이면 확인 창 닫힘.
  const [deleteTarget, setDeleteTarget] = useState<{
    deviceId: string;
    loginId: string;
  } | null>(null);
  const [deleting, setDeleting] = useState(false);
  // 삭제 진행 중인 계정(`deviceId::loginId`). 낙관적 삭제 후에도 하위가 아직 그 계정을 인벤토리에
  // 보고하면(삭제 미처리), 폴링이 되살리는 것을 막기 위해 이 목록의 계정을 폴링 결과에서 걸러낸다.
  // 하위가 실제로 반영해 더 이상 보고하지 않으면 목록에서 해제한다(재등장 종료). state가 아니라 ref로
  // 두어 폴링 인터벌을 재구독시키지 않는다.
  const pendingDeletesRef = useRef<Set<string>>(new Set());

  const deviceName = (id: string) =>
    devices.find((d) => d.id === id)?.name ?? id;

  // 선택 순서대로 색을 배정(하위별 고정). 표의 그룹 헤더·테두리·배지에 같은 색을 쓴다.
  const colorForDevice = (id: string) => {
    const i = selectedIds.indexOf(id);
    return DEVICE_COLORS[(i < 0 ? 0 : i) % DEVICE_COLORS.length]!;
  };

  // online 하위 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지).
  useEffect(() => {
    api.devices
      .list()
      .then((list) =>
        setDevices(
          list
            .filter((d) => d.connected)
            .map((d) => ({ id: d.id, name: d.name })),
        ),
      )
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  }, []);

  // 선택한 **모든** 하위의 accountRows 4초 폴링. 각 하위를 따로 조회해 original[deviceId]에 담는다.
  // 하위에서 상태를 바꾸면 다음 보고(≤4초)에 그 하위 행만 갱신된다(양방향). 오프라인/미보고면 더미 폴백.
  useEffect(() => {
    // 선택이 비면 폴링만 멈춘다. original 정리는 MultiSelect onChange가 이미 처리한다(effect 안에서
    // 동기 setState를 하지 않아 불필요한 연쇄 렌더를 피한다).
    if (selectedIds.length === 0) return;
    let cancelled = false;
    const load = () => {
      selectedIds.forEach((did) => {
        api.devices
          .inventory(did)
          .then((inv) => {
            if (cancelled) return;
            const raw = (inv.accountRows ?? []).map((a) => ({
              loginId: a.loginId,
              platform: a.platform ?? "forum",
              status: a.status ?? "new",
            }));
            // 삭제 진행 중이던 계정이 이제 인벤토리에 없으면(하위가 삭제 반영) pending에서 해제한다.
            const prefix = `${did}::`;
            const rawLogins = new Set(raw.map((r) => r.loginId));
            for (const key of [...pendingDeletesRef.current]) {
              if (
                key.startsWith(prefix) &&
                !rawLogins.has(key.slice(prefix.length))
              )
                pendingDeletesRef.current.delete(key);
            }
            // 아직 삭제 미반영이라 하위가 보고하는 계정은 화면에서 걸러 되살아나지 않게 한다.
            setOriginal((prev) => ({
              ...prev,
              [did]: raw.filter(
                (r) => !pendingDeletesRef.current.has(`${did}::${r.loginId}`),
              ),
            }));
          })
          .catch(() => {
            if (cancelled) return;
            setOriginal((prev) => ({
              ...prev,
              [did]: (INITIAL_ROWS[did] ?? []).filter(
                (r) => !pendingDeletesRef.current.has(`${did}::${r.loginId}`),
              ),
            }));
          });
      });
    };
    load();
    const id = window.setInterval(load, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [selectedIds]);

  // 표시 행 = 선택한 모든 하위의 행을 이어붙이고, 하위·계정별 편집 오버라이드를 적용한다.
  const rows: DisplayRow[] = useMemo(
    () =>
      selectedIds.flatMap((did) =>
        (original[did] ?? []).map((r) => ({
          deviceId: did,
          deviceName: deviceName(did),
          loginId: r.loginId,
          platform: edits[did]?.[r.loginId]?.platform ?? r.platform,
          status: edits[did]?.[r.loginId]?.status ?? r.status,
        })),
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selectedIds, original, edits, devices],
  );

  // 하위별 변경 목록(저장 시 각자 따로 보낼 것). diffAccountRows를 하위별로 돌린다.
  const pendingByDevice: Record<string, MetaUpdate[]> = useMemo(() => {
    const out: Record<string, MetaUpdate[]> = {};
    for (const did of selectedIds) {
      const orig = original[did] ?? [];
      const edited = orig.map((r) => ({
        loginId: r.loginId,
        platform: edits[did]?.[r.loginId]?.platform ?? r.platform,
        status: edits[did]?.[r.loginId]?.status ?? r.status,
      }));
      const diff = diffAccountRows(orig, edited);
      if (diff.length > 0) out[did] = diff;
    }
    return out;
  }, [selectedIds, original, edits]);

  const pendingCount = useMemo(
    () => Object.values(pendingByDevice).reduce((s, arr) => s + arr.length, 0),
    [pendingByDevice],
  );

  const editRow = (
    deviceId: string,
    loginId: string,
    patch: { platform?: string; status?: string },
  ) =>
    setEdits((prev) => ({
      ...prev,
      [deviceId]: {
        ...prev[deviceId],
        [loginId]: { ...prev[deviceId]?.[loginId], ...patch },
      },
    }));

  const save = async () => {
    const entries = Object.entries(pendingByDevice);
    if (entries.length === 0) return;
    setSaving(true);
    let ok = 0;
    let offline = false;
    const errs: string[] = [];
    // 하위별로 **각자 따로** 전송 — B의 변경이 A로 가지 않는다(하위별 독립 명령).
    for (const [did, updates] of entries) {
      try {
        await api.accounts.updateMeta(did, updates);
        ok += updates.length;
      } catch (e) {
        if (isOffline(e)) offline = true;
        else
          errs.push(
            `${deviceName(did)}: ${e instanceof Error ? e.message : "실패"}`,
          );
      }
    }
    if (offline) {
      // 오프라인 미리보기: 편집을 로컬 확정(선택 하위별로 original에 반영).
      setOriginal((prev) => {
        const next = { ...prev };
        for (const did of selectedIds) {
          next[did] = (prev[did] ?? []).map((r) => ({
            loginId: r.loginId,
            platform: edits[did]?.[r.loginId]?.platform ?? r.platform,
            status: edits[did]?.[r.loginId]?.status ?? r.status,
          }));
        }
        return next;
      });
      setEdits({});
      notifications.show({
        message: `계정 변경(미리보기 — 서버 미연결)`,
        color: "gray",
      });
    } else if (errs.length > 0) {
      notifications.show({ message: errs.join(" / "), color: "red" });
    } else {
      setEdits({});
      notifications.show({
        message: `계정 ${ok}건 변경을 ${entries.length}대에 각각 전송했어요`,
        color: "green",
      });
    }
    setSaving(false);
  };

  // 계정 삭제(휴지통) — 그 계정이 속한 하위 1대에만 삭제 명령. 성공하면 그 행을 즉시 제거(낙관적).
  const confirmDelete = async () => {
    if (deleteTarget === null) return;
    const { deviceId, loginId } = deleteTarget;
    setDeleting(true);
    try {
      const r = await api.accounts.delete(deviceId, [loginId]);
      notifications.show({
        message: `계정 ${maskId(loginId)}을(를) Admin·하위에서 삭제했어요 (commandId=${r.commandId})`,
        color: "green",
      });
      removeRowLocally(deviceId, loginId);
    } catch (e) {
      if (isOffline(e)) {
        removeRowLocally(deviceId, loginId);
        notifications.show({
          message: `계정 ${maskId(loginId)} 삭제(미리보기 — 서버 미연결)`,
          color: "gray",
        });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "삭제 실패",
          color: "red",
        });
      }
    } finally {
      setDeleting(false);
      setDeleteTarget(null);
    }
  };

  // 표시 목록에서 그 하위의 그 loginId를 제거 — 폴링본·편집 양쪽에서 뺀다. 아울러 삭제 진행 목록에
  // 넣어, 하위가 삭제를 반영하기 전 폴링이 이 계정을 되살리는 것을 막는다.
  const removeRowLocally = (deviceId: string, loginId: string) => {
    pendingDeletesRef.current.add(`${deviceId}::${loginId}`);
    setOriginal((prev) => ({
      ...prev,
      [deviceId]: (prev[deviceId] ?? []).filter((r) => r.loginId !== loginId),
    }));
    setEdits((prev) => {
      const dev = { ...prev[deviceId] };
      delete dev[loginId];
      return { ...prev, [deviceId]: dev };
    });
  };

  return (
    <Box
      p="lg"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 16,
        height: "100%",
      }}
    >
      <Paper withBorder radius="md" p="lg">
        <Group justify="space-between" mb="sm">
          <Group gap="xs">
            <Text fw={800} size="lg">
              계정 상태 관리
            </Text>
            <Text size="xs" c="dimmed">
              하위를 여러 대 고르면 각 하위의 계정이 함께 표시되고, 저장은
              하위별로 따로 적용됩니다
            </Text>
          </Group>
          <MultiSelect
            w={320}
            placeholder="하위 COM 선택"
            data={devices.map((d) => ({ value: d.id, label: d.name }))}
            value={selectedIds}
            onChange={(vals) => {
              setSelectedIds(vals);
              // 선택 해제된 하위의 폴링본·편집을 정리(남은 것만 유지).
              setEdits((prev) => {
                const next: typeof prev = {};
                for (const id of vals) if (prev[id]) next[id] = prev[id]!;
                return next;
              });
              setOriginal((prev) => {
                const next: typeof prev = {};
                for (const id of vals) if (prev[id]) next[id] = prev[id]!;
                return next;
              });
            }}
            comboboxProps={{ withinPortal: true }}
            aria-label="하위 COM 선택"
          />
        </Group>
      </Paper>

      <Paper
        withBorder
        radius="md"
        p="lg"
        style={{
          flex: 1,
          minHeight: 0,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <Group justify="space-between" mb="sm">
          <Group gap="xs">
            <Text fw={800} size="lg">
              계정
            </Text>
            <Badge variant="light" color="gray" radius="sm">
              총 {rows.length}개
            </Badge>
            {selectedIds.length > 0 && (
              <Badge variant="light" color="gray" radius="sm">
                하위 {selectedIds.length}대
              </Badge>
            )}
            {pendingCount > 0 && (
              <Badge variant="light" color="blue" radius="sm">
                {pendingCount}개 변경
              </Badge>
            )}
          </Group>
          <Button
            color="blue"
            disabled={pendingCount === 0 || saving}
            loading={saving}
            onClick={() => void save()}
          >
            저장
          </Button>
        </Group>

        <Box style={{ flex: 1, minHeight: 0 }}>
          <ScrollArea h="100%" type="auto">
            <Table highlightOnHover stickyHeader verticalSpacing="xs">
              <Table.Thead>
                <Table.Tr>
                  <Table.Th w={160}>하위(컴퓨터)</Table.Th>
                  <Table.Th>계정</Table.Th>
                  <Table.Th w={200}>플랫폼</Table.Th>
                  <Table.Th w={200}>상태</Table.Th>
                  <Table.Th w={60}>삭제</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {selectedIds.map((did) => {
                  const color = colorForDevice(did);
                  const devRows = rows.filter((r) => r.deviceId === did);
                  return (
                    <Fragment key={did}>
                      {/* 그룹 구분 헤더 — 여기부터 이 하위 COM의 계정(색·상단선으로 구분). */}
                      <Table.Tr
                        style={{
                          backgroundColor: `var(--mantine-color-${color}-light)`,
                          borderTop: `2px solid var(--mantine-color-${color}-filled)`,
                        }}
                      >
                        <Table.Td colSpan={5}>
                          <Group gap="xs">
                            <Badge color={color} variant="filled" radius="sm">
                              {deviceName(did)}
                            </Badge>
                            <Text size="xs" c="dimmed">
                              계정 {devRows.length}개
                            </Text>
                          </Group>
                        </Table.Td>
                      </Table.Tr>
                      {devRows.map((r) => (
                        <Table.Tr
                          key={`${r.deviceId}::${r.loginId}`}
                          style={{
                            borderLeft: `3px solid var(--mantine-color-${color}-filled)`,
                          }}
                        >
                          <Table.Td>
                            <Badge variant="light" color={color} radius="sm">
                              {r.deviceName}
                            </Badge>
                          </Table.Td>
                          <Table.Td>{maskId(r.loginId)}</Table.Td>
                          <Table.Td>
                            <Select
                              size="xs"
                              data={PLATFORM_OPTS}
                              value={r.platform}
                              allowDeselect={false}
                              comboboxProps={{ withinPortal: true }}
                              aria-label={`${maskId(r.loginId)} 플랫폼`}
                              onChange={(v) =>
                                v &&
                                editRow(r.deviceId, r.loginId, { platform: v })
                              }
                            />
                          </Table.Td>
                          <Table.Td>
                            {isReversibleStatus(r.status) ? (
                              <Select
                                size="xs"
                                data={STATUS_OPTS}
                                value={r.status}
                                allowDeselect={false}
                                comboboxProps={{ withinPortal: true }}
                                aria-label={`${maskId(r.loginId)} 상태`}
                                onChange={(v) =>
                                  v &&
                                  editRow(r.deviceId, r.loginId, { status: v })
                                }
                              />
                            ) : (
                              // 워커 판정값(차단 등)은 읽기 전용 배지(§5).
                              <Badge variant="light" color="gray" radius="sm">
                                {statusLabel(r.status)}
                              </Badge>
                            )}
                          </Table.Td>
                          <Table.Td>
                            <ActionIcon
                              variant="subtle"
                              color="red"
                              title="계정 삭제"
                              aria-label={`${maskId(r.loginId)} 삭제`}
                              onClick={() =>
                                setDeleteTarget({
                                  deviceId: r.deviceId,
                                  loginId: r.loginId,
                                })
                              }
                            >
                              <IconTrash size={18} />
                            </ActionIcon>
                          </Table.Td>
                        </Table.Tr>
                      ))}
                      {devRows.length === 0 && (
                        <Table.Tr>
                          <Table.Td colSpan={5}>
                            <Text c="dimmed" size="sm" pl="md" py="xs">
                              이 하위에 분배된 계정이 없습니다.
                            </Text>
                          </Table.Td>
                        </Table.Tr>
                      )}
                    </Fragment>
                  );
                })}
                {selectedIds.length === 0 && (
                  <Table.Tr>
                    <Table.Td colSpan={5}>
                      <Text c="dimmed" ta="center" py="md">
                        위에서 하위 COM을 선택하세요(여러 대 선택 가능).
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                )}
              </Table.Tbody>
            </Table>
          </ScrollArea>
        </Box>

        <Text size="xs" c="dimmed" mt="sm">
          상태 선택지 = 활성·대기·보류(사람이 되돌릴 수 있는 값). 차단 등 워커
          판정값은 읽기 전용 · 하위에서 바꾸면 ≤4초 뒤 여기에도 반영(양방향).
        </Text>
      </Paper>

      <Modal
        opened={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        title="계정 삭제"
        centered
      >
        <Text size="sm">
          {deleteTarget !== null ? maskId(deleteTarget.loginId) : ""} 계정을
          Admin과 하위 PC에서 삭제합니다. 되돌릴 수 없습니다.
        </Text>
        <Group justify="flex-end" mt="lg">
          <Button
            variant="default"
            disabled={deleting}
            onClick={() => setDeleteTarget(null)}
          >
            취소
          </Button>
          <Button
            color="red"
            loading={deleting}
            onClick={() => void confirmDelete()}
          >
            삭제
          </Button>
        </Group>
      </Modal>
    </Box>
  );
}
