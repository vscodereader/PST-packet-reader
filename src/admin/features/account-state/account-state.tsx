import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Group,
  Modal,
  Paper,
  ScrollArea,
  Select,
  Table,
  Text,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconTrash } from "@tabler/icons-react";
import { useEffect, useMemo, useState } from "react";

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

export function AccountState() {
  const [devices, setDevices] = useState<OnlineDevice[]>(INITIAL_DEVICES);
  const [deviceId, setDeviceId] = useState<string | null>(null);
  // 폴링본(하위가 보고한 원래 값). 저장 diff의 기준.
  const [original, setOriginal] = useState<AccountRow[]>([]);
  // 사용자 편집 오버라이드(loginId → 바꾼 platform/status). 폴링이 original을 갱신해도 미저장 편집은 유지.
  const [edits, setEdits] = useState<
    Record<string, { platform?: string; status?: string }>
  >({});
  const [saving, setSaving] = useState(false);
  // 삭제 확인 대상(휴지통 클릭한 loginId) — null이면 확인 창 닫힘. 삭제 중이면 버튼 잠금.
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

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

  // 선택한 하위의 accountRows 4초 폴링(§3 읽기 — 신규 배선 0, 인벤토리 재사용). 하위에서 상태를
  // 바꾸면 다음 보고(≤4초)에 여기 표가 갱신된다(양방향). 오프라인/미보고면 더미로 폴백.
  useEffect(() => {
    if (deviceId === null) return;
    let cancelled = false;
    const load = () => {
      api.devices
        .inventory(deviceId)
        .then((inv) => {
          if (cancelled) return;
          setOriginal(
            (inv.accountRows ?? []).map((a) => ({
              loginId: a.loginId,
              platform: a.platform ?? "forum",
              status: a.status ?? "new",
            })),
          );
        })
        .catch(() => {
          if (cancelled) return;
          // 오프라인 미리보기: 더미 행으로 폴백(선택 하위 기준).
          setOriginal(INITIAL_ROWS[deviceId] ?? []);
        });
    };
    load();
    const id = window.setInterval(load, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [deviceId]);

  // 표시값 = 편집 오버라이드가 있으면 그 값, 없으면 폴링본.
  const rows: AccountRow[] = useMemo(
    () =>
      original.map((r) => ({
        loginId: r.loginId,
        platform: edits[r.loginId]?.platform ?? r.platform,
        status: edits[r.loginId]?.status ?? r.status,
      })),
    [original, edits],
  );

  const pending = useMemo(
    () => diffAccountRows(original, rows),
    [original, rows],
  );

  const editRow = (loginId: string, patch: { platform?: string; status?: string }) =>
    setEdits((prev) => ({
      ...prev,
      [loginId]: { ...prev[loginId], ...patch },
    }));

  const save = async () => {
    if (deviceId === null || pending.length === 0) return;
    setSaving(true);
    try {
      const r = await api.accounts.updateMeta(deviceId, pending);
      notifications.show({
        message: `계정 ${pending.length}건 변경을 하위에 전송했어요 (commandId=${r.commandId})`,
        color: "green",
      });
      // 저장 완료 → 편집 오버라이드 비우고 폴링 갱신본이 최종값이 되게 한다.
      setEdits({});
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 로컬에서 편집을 확정(original에 반영)해 시연.
        setOriginal(rows);
        setEdits({});
        notifications.show({
          message: `계정 ${pending.length}건 변경(미리보기 — 서버 미연결)`,
          color: "gray",
        });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : "변경 실패",
          color: "red",
        });
      }
    } finally {
      setSaving(false);
    }
  };

  // 계정 삭제(휴지통) — Admin과 하위 PC 양쪽에서 지운다. 성공하면 표시 목록에서 그 행을 즉시 제거
  // (낙관적 삭제)하고, ≤4초 뒤 하위 재보고로도 사라진다. 오프라인 미리보기는 로컬에서만 제거해 시연.
  const confirmDelete = async () => {
    if (deviceId === null || deleteTarget === null) return;
    const loginId = deleteTarget;
    setDeleting(true);
    try {
      const r = await api.accounts.delete(deviceId, [loginId]);
      notifications.show({
        message: `계정 ${maskId(loginId)}을(를) Admin·하위에서 삭제했어요 (commandId=${r.commandId})`,
        color: "green",
      });
      removeRowLocally(loginId);
    } catch (e) {
      if (isOffline(e)) {
        // 오프라인 미리보기: 로컬에서만 제거해 시연(서버 미연결).
        removeRowLocally(loginId);
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

  // 표시 목록에서 그 loginId를 제거 — 폴링본(original)과 미저장 편집(edits) 양쪽에서 뺀다.
  const removeRowLocally = (loginId: string) => {
    setOriginal((prev) => prev.filter((r) => r.loginId !== loginId));
    setEdits((prev) => {
      const next = { ...prev };
      delete next[loginId];
      return next;
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
              하위를 고르면 그 하위의 계정 상태·플랫폼을 원격으로 바꿉니다
            </Text>
          </Group>
          <Select
            w={220}
            placeholder="하위 COM 선택"
            data={devices.map((d) => ({ value: d.id, label: d.name }))}
            value={deviceId}
            onChange={(v) => {
              setDeviceId(v);
              setEdits({});
              setOriginal([]);
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
            {pending.length > 0 && (
              <Badge variant="light" color="blue" radius="sm">
                {pending.length}개 변경
              </Badge>
            )}
          </Group>
          <Button
            color="blue"
            disabled={deviceId === null || pending.length === 0 || saving}
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
                  <Table.Th>계정</Table.Th>
                  <Table.Th w={200}>플랫폼</Table.Th>
                  <Table.Th w={200}>상태</Table.Th>
                  <Table.Th w={60}>삭제</Table.Th>
                </Table.Tr>
              </Table.Thead>
              <Table.Tbody>
                {rows.map((r) => (
                  <Table.Tr key={r.loginId}>
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
                          v && editRow(r.loginId, { platform: v })
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
                          onChange={(v) => v && editRow(r.loginId, { status: v })}
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
                        onClick={() => setDeleteTarget(r.loginId)}
                      >
                        <IconTrash size={18} />
                      </ActionIcon>
                    </Table.Td>
                  </Table.Tr>
                ))}
                {deviceId !== null && rows.length === 0 && (
                  <Table.Tr>
                    <Table.Td colSpan={4}>
                      <Text c="dimmed" ta="center" py="md">
                        이 하위에 분배된 계정이 없습니다.
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                )}
                {deviceId === null && (
                  <Table.Tr>
                    <Table.Td colSpan={4}>
                      <Text c="dimmed" ta="center" py="md">
                        위에서 하위 COM을 선택하세요.
                      </Text>
                    </Table.Td>
                  </Table.Tr>
                )}
              </Table.Tbody>
            </Table>
          </ScrollArea>
        </Box>

        <Text size="xs" c="dimmed" mt="sm">
          상태 선택지 = 활성·대기·보류(사람이 되돌릴 수 있는 값). 차단 등 워커 판정값은 읽기 전용 ·
          하위에서 바꾸면 ≤4초 뒤 여기에도 반영(양방향).
        </Text>
      </Paper>

      <Modal
        opened={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        title="계정 삭제"
        centered
      >
        <Text size="sm">
          {deleteTarget !== null ? maskId(deleteTarget) : ""} 계정을 Admin과 하위
          PC에서 삭제합니다. 되돌릴 수 없습니다.
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
