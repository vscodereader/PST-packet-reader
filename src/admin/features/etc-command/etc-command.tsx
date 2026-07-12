import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Divider,
  Group,
  NumberInput,
  Paper,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconDeviceDesktop } from "@tabler/icons-react";
import { useEffect, useMemo, useState } from "react";

import { Icon } from "@/shared/ui/icons";

import { api, isOffline } from "../../api";
import { filterAccountsByTarget } from "../publish-command/publish-command";

// 기타 명령 페이지(15-기타명령 §2). 하위 1대를 고르고 → 행동(좋아요/싫어요/조회수/IP변경)을 골라
// 그 하위의 데스크톱 즉시 실행 엔진을 원격으로 부른다. 게시 큐를 안 타는 즉시 실행이라, 결과는
// post-report(종류 태그)로 결과 보고에, 원시 로그는 통신 로그에 뜬다. UI는 데스크톱 LikeModal/
// ViewCountModal 모양을 그대로 이식한다(엔진·데스크톱 무손상).

interface EtcDevice {
  id: string;
  name: string;
  ip: string;
}

type Action = "like" | "dislike" | "boost" | "rotate";
const ACTIONS: { value: Action; label: string }[] = [
  { value: "like", label: "좋아요" },
  { value: "dislike", label: "싫어요" },
  { value: "boost", label: "조회수" },
  { value: "rotate", label: "IP 변경" },
];

/** 반복 횟수(N) 허용 범위 — 데스크톱 ViewCountModal과 동일. */
const MIN_REPEATS = 1;
const MAX_REPEATS = 1000;

// ── 미리보기 더미(서버 프록시 배선 전) — 게시명령 화면과 동일 폴백. ──
const DUMMY_DEVICES: EtcDevice[] = [
  { id: "d1", name: "하위-001", ip: "1.2.3.4" },
  { id: "d2", name: "하위-002", ip: "1.2.3.5" },
  { id: "d3", name: "하위-003", ip: "1.2.3.6" },
];
const mockForumAccounts = (deviceId: string): string[] => [
  `forum_${deviceId}_a`,
  `forum_${deviceId}_b`,
];

/** 게시글 링크에서 사람이 읽을 라벨(끝 discussion id). 칩 표시용(없으면 링크 자체). */
function postLabel(url: string): string {
  const m = url.match(/\/discussion\/(\d+)/);
  return m ? `글 #${m[1]}` : url;
}

export function EtcCommand() {
  const [devices, setDevices] = useState<EtcDevice[]>(DUMMY_DEVICES);
  const [selId, setSelId] = useState<string | null>(null);
  const [action, setAction] = useState<Action | null>(null);
  // 그 하위의 종토(forum) active 계정(§6-4) — filterAccountsByTarget("forum")과 동일 규칙.
  const [forumAccounts, setForumAccounts] = useState<string[]>([]);

  // 좋아요/싫어요·조회수 공용 링크 입력(칩).
  const [links, setLinks] = useState<string[]>([]);
  const [linkInput, setLinkInput] = useState("");
  // 좋아요/싫어요 계정 선택(loginId).
  const [selAccounts, setSelAccounts] = useState<string[]>([]);
  // 조회수 반복 N(비우면 "" → 버튼 비활성).
  const [repeats, setRepeats] = useState<number | "">(30);
  const [busy, setBusy] = useState(false);

  // online 하위 로드(서버 연결 시 실데이터, 오프라인이면 더미 유지) — 게시명령 화면과 동일.
  useEffect(() => {
    api.devices
      .list()
      .then((list) =>
        setDevices(
          list
            .filter((d) => d.connected)
            .map((d) => ({ id: d.id, name: d.name, ip: d.ip ?? "-" })),
        ),
      )
      .catch(() => {
        /* 오프라인 → 더미 유지 */
      });
  }, []);

  // 고른 하위의 종토 계정 로드(인벤토리 accountRows → filterAccountsByTarget). 오프라인/미보고면 더미.
  // selId는 최초 null에서 선택 후 계속 non-null(재선택도 유지)이라 해제 리셋 분기는 없다.
  useEffect(() => {
    if (!selId) return;
    let cancelled = false;
    api.devices
      .inventory(selId)
      .then((inv) => {
        if (cancelled) return;
        const filtered = filterAccountsByTarget("forum", inv.accountRows);
        setForumAccounts(filtered ?? mockForumAccounts(selId));
      })
      .catch(() => {
        if (!cancelled) setForumAccounts(mockForumAccounts(selId));
      });
    return () => {
      cancelled = true;
    };
  }, [selId]);

  // 하위를 바꾸면 행동·입력을 초기화(다른 하위 값이 남지 않게).
  const selectDevice = (id: string) => {
    setSelId((cur) => (cur === id ? cur : id));
    if (selId !== id) {
      setAction(null);
      setLinks([]);
      setLinkInput("");
      setSelAccounts([]);
      setRepeats(30);
    }
  };

  const addLink = () => {
    const link = linkInput.trim();
    if (!link) return;
    setLinks((prev) => (prev.includes(link) ? prev : [...prev, link]));
    setLinkInput("");
  };
  const removeLink = (link: string) =>
    setLinks((prev) => prev.filter((l) => l !== link));

  const toggleAccount = (id: string) =>
    setSelAccounts((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  const allOn =
    forumAccounts.length > 0 &&
    forumAccounts.every((a) => selAccounts.includes(a));
  const toggleAll = () =>
    setSelAccounts((s) =>
      allOn ? s.filter((id) => !forumAccounts.includes(id)) : [...forumAccounts],
    );

  const repeatsNum = typeof repeats === "number" ? repeats : NaN;
  const selectedDevice = devices.find((d) => d.id === selId) ?? null;

  const canReact = links.length > 0 && selAccounts.length > 0 && !busy;
  const canBoost = links.length > 0 && repeatsNum >= MIN_REPEATS && !busy;

  // 행동 실행 — 오프라인 미리보기면 성공 토스트만(데모). 실서버면 그 하위로 SSE 명령.
  const run = async (fn: () => Promise<unknown>, label: string) => {
    if (!selId) return;
    setBusy(true);
    try {
      await fn();
      notifications.show({ message: `${label} 명령을 보냈습니다`, color: "blue" });
    } catch (e) {
      if (isOffline(e)) {
        notifications.show({
          message: `${label} 명령을 보냈습니다(미리보기)`,
          color: "blue",
        });
      } else {
        notifications.show({
          message: e instanceof Error ? e.message : `${label} 명령 실패`,
          color: "red",
        });
      }
    } finally {
      setBusy(false);
    }
  };

  const submitReact = (kind: "like" | "dislike") => {
    if (!selId || !canReact) return;
    const label = kind === "dislike" ? "싫어요" : "좋아요";
    const call =
      kind === "dislike"
        ? () => api.etc.dislike(selId, links, selAccounts)
        : () => api.etc.like(selId, links, selAccounts);
    void run(call, label);
  };
  const submitBoost = () => {
    if (!selId || !canBoost) return;
    void run(() => api.etc.boostView(selId, links, repeatsNum), "조회수");
  };
  const submitRotate = () => {
    if (!selId || busy) return;
    void run(() => api.etc.rotateIp(selId), "IP 변경");
  };

  const color = useMemo(() => {
    switch (action) {
      case "like":
        return "red";
      case "dislike":
        return "indigo";
      case "boost":
        return "teal";
      case "rotate":
        return "grape";
      default:
        return "blue";
    }
  }, [action]);

  return (
    <Stack gap="lg" p="md" h="100%">
      <Box>
        <Text fw={800} size="xl">
          기타 명령
        </Text>
        <Text size="sm" c="dimmed">
          하위 1대를 골라 좋아요·싫어요·조회수·IP 변경을 원격으로 실행합니다.
          실제 실행은 그 하위가 자기 IP로 수행하고, 결과는 결과 보고(종류 태그)와
          통신 로그에 뜹니다.
        </Text>
      </Box>

      {/* ① 하위 선택(1대) */}
      <Box>
        <Text fw={700} size="sm" mb="xs">
          ① 하위 선택{" "}
          <Text span c="dimmed" size="xs">
            (online 하위 1대)
          </Text>
        </Text>
        <SimpleGrid cols={4} spacing="sm">
          {devices.map((d) => {
            const on = selId === d.id;
            return (
              <Paper
                key={d.id}
                withBorder
                radius="md"
                p="sm"
                onClick={() => selectDevice(d.id)}
                style={{
                  cursor: "pointer",
                  borderColor: on ? "var(--mantine-color-blue-6)" : undefined,
                  borderWidth: on ? 2 : 1,
                  background: on ? "var(--mantine-color-blue-0)" : undefined,
                }}
                aria-label={`${d.name} 선택`}
              >
                <Group gap="sm" wrap="nowrap">
                  <ThemeIcon
                    size={38}
                    radius="md"
                    variant="light"
                    color={on ? "blue" : "gray"}
                  >
                    <IconDeviceDesktop size={22} />
                  </ThemeIcon>
                  <Box style={{ minWidth: 0 }}>
                    <Text fw={700} size="sm" truncate>
                      {d.name}
                    </Text>
                    <Text size="xs" c="dimmed" truncate>
                      IP {d.ip}
                    </Text>
                  </Box>
                </Group>
              </Paper>
            );
          })}
        </SimpleGrid>
      </Box>

      {/* ② 행동 선택 — 하위를 골라야 활성화 */}
      {selectedDevice && (
        <Box>
          <Text fw={700} size="sm" mb="xs">
            ② 행동을 고르세요{" "}
            <Text span c="dimmed" size="xs">
              ({selectedDevice.name})
            </Text>
          </Text>
          <SegmentedControl
            value={action ?? ""}
            onChange={(v) => setAction(v as Action)}
            data={ACTIONS}
            color={color}
            aria-label="행동 선택"
          />
        </Box>
      )}

      {/* ③ 행동별 UI */}
      {selectedDevice && action && (
        <Paper withBorder radius="md" p="md">
          {(action === "like" || action === "dislike") && (
            <ReactionPanel
              label={action === "dislike" ? "싫어요" : "좋아요"}
              color={color}
              links={links}
              linkInput={linkInput}
              onLinkInput={setLinkInput}
              onAddLink={addLink}
              onRemoveLink={removeLink}
              accounts={forumAccounts}
              selected={selAccounts}
              allOn={allOn}
              onToggle={toggleAccount}
              onToggleAll={toggleAll}
              busy={busy}
              canSubmit={canReact}
              onSubmit={() =>
                submitReact(action === "dislike" ? "dislike" : "like")
              }
            />
          )}
          {action === "boost" && (
            <BoostPanel
              links={links}
              linkInput={linkInput}
              onLinkInput={setLinkInput}
              onAddLink={addLink}
              onRemoveLink={removeLink}
              repeats={repeats}
              onRepeats={setRepeats}
              repeatsNum={repeatsNum}
              busy={busy}
              canSubmit={canBoost}
              onSubmit={submitBoost}
            />
          )}
          {action === "rotate" && (
            <Stack gap={12}>
              <Text fz={13} c="dimmed">
                이 하위 PC의 연결된 폰(ADB) 비행기모드를 껐다 켜 IP를 회전합니다.
                폰/ADB가 없으면 앱은 죽지 않고 IP 변경 실패로 결과 보고·통신
                로그에 남습니다.
              </Text>
              <Group justify="flex-end">
                <Button
                  color="grape"
                  leftSection={<Icon.globe size={16} />}
                  onClick={submitRotate}
                  disabled={busy}
                >
                  IP 변경
                </Button>
              </Group>
            </Stack>
          )}
        </Paper>
      )}
    </Stack>
  );
}

// 좋아요/싫어요 패널 — 데스크톱 LikeModal 모양(링크 입력 + 종토 계정 체크박스).
function ReactionPanel({
  label,
  color,
  links,
  linkInput,
  onLinkInput,
  onAddLink,
  onRemoveLink,
  accounts,
  selected,
  allOn,
  onToggle,
  onToggleAll,
  busy,
  canSubmit,
  onSubmit,
}: {
  label: string;
  color: string;
  links: string[];
  linkInput: string;
  onLinkInput: (v: string) => void;
  onAddLink: () => void;
  onRemoveLink: (link: string) => void;
  accounts: string[];
  selected: string[];
  allOn: boolean;
  onToggle: (id: string) => void;
  onToggleAll: () => void;
  busy: boolean;
  canSubmit: boolean;
  onSubmit: () => void;
}) {
  return (
    <Stack gap={16}>
      <Text fz={13} c="dimmed">
        게시글 링크를 넣고(여러 개 가능) 계정을 고르면, 선택한 계정들이 그
        글들에 {label}를 누릅니다.
      </Text>
      <LinkInput
        label={`${label}를 누를 게시글 링크`}
        color={color}
        links={links}
        linkInput={linkInput}
        onLinkInput={onLinkInput}
        onAddLink={onAddLink}
        onRemoveLink={onRemoveLink}
      />
      <Divider label={`${label}를 누를 계정`} labelPosition="left" />
      {accounts.length === 0 ? (
        <Text fz={13} c="orange.7">
          이 하위에 {label}를 누를 수 있는 종목토론방 로그인 계정이 없습니다.
        </Text>
      ) : (
        <Stack gap={4}>
          <Group justify="space-between" px={4}>
            <Text fz={12} c="dimmed">
              {selected.length}/{accounts.length}개 선택됨
            </Text>
            <Button
              size="compact-xs"
              variant="subtle"
              color="gray"
              onClick={onToggleAll}
            >
              {allOn ? "전체 해제" : "전체 선택"}
            </Button>
          </Group>
          <Box
            style={{
              maxHeight: 240,
              overflowY: "auto",
              border: "1px solid var(--mantine-color-gray-2)",
              borderRadius: "var(--mantine-radius-sm)",
              padding: 8,
            }}
          >
            <Stack gap={4}>
              {accounts.map((a) => (
                <Checkbox
                  key={a}
                  label={a}
                  checked={selected.includes(a)}
                  onChange={() => onToggle(a)}
                  aria-label={`${a} 선택`}
                />
              ))}
            </Stack>
          </Box>
        </Stack>
      )}
      <Group justify="flex-end">
        <Button
          color={color}
          leftSection={<Icon.heart size={16} />}
          onClick={onSubmit}
          disabled={!canSubmit}
        >
          {busy ? `${label} 누르는 중…` : label}
        </Button>
      </Group>
    </Stack>
  );
}

// 조회수 패널 — 데스크톱 ViewCountModal 모양(링크 입력 + 횟수 N). 계정 선택 없음.
function BoostPanel({
  links,
  linkInput,
  onLinkInput,
  onAddLink,
  onRemoveLink,
  repeats,
  onRepeats,
  repeatsNum,
  busy,
  canSubmit,
  onSubmit,
}: {
  links: string[];
  linkInput: string;
  onLinkInput: (v: string) => void;
  onAddLink: () => void;
  onRemoveLink: (link: string) => void;
  repeats: number | "";
  onRepeats: (v: number | "") => void;
  repeatsNum: number;
  busy: boolean;
  canSubmit: boolean;
  onSubmit: () => void;
}) {
  return (
    <Stack gap={16}>
      <Text fz={13} c="dimmed">
        게시글 링크를 넣고(여러 개 가능) 반복 횟수를 정하면, 각 링크를 시크릿창으로
        그 횟수만큼 여닫아 조회수를 올립니다.
      </Text>
      <LinkInput
        label="조회수를 올릴 게시글 링크"
        color="teal"
        links={links}
        linkInput={linkInput}
        onLinkInput={onLinkInput}
        onAddLink={onAddLink}
        onRemoveLink={onRemoveLink}
      />
      <Divider label="반복 횟수(링크마다)" labelPosition="left" />
      <Group gap={10} align="center">
        <NumberInput
          aria-label="반복 횟수"
          value={repeats}
          onChange={(v) => {
            if (v === "" || typeof v === "number") {
              onRepeats(v);
            } else {
              const n = parseInt(v, 10);
              onRepeats(Number.isNaN(n) ? "" : n);
            }
          }}
          min={MIN_REPEATS}
          max={MAX_REPEATS}
          clampBehavior="strict"
          w={110}
          styles={{ input: { fontFamily: "monospace", textAlign: "center" } }}
        />
        <Text fz={12.5} c="dimmed">
          링크 {links.length}개 × {repeatsNum || 0}회 = 총{" "}
          <b>{links.length * (repeatsNum || 0)}</b>번 여닫습니다.
        </Text>
      </Group>
      <Group justify="flex-end">
        <Button
          color="teal"
          leftSection={<Icon.eye size={16} />}
          onClick={onSubmit}
          disabled={!canSubmit}
        >
          {busy ? "조회수 올리는 중…" : "조회수"}
        </Button>
      </Group>
    </Stack>
  );
}

// 공용 링크 입력(엔터/추가 → 칩) — 데스크톱 모달의 링크 입력과 동일 UX.
function LinkInput({
  label,
  color,
  links,
  linkInput,
  onLinkInput,
  onAddLink,
  onRemoveLink,
}: {
  label: string;
  color: string;
  links: string[];
  linkInput: string;
  onLinkInput: (v: string) => void;
  onAddLink: () => void;
  onRemoveLink: (link: string) => void;
}) {
  return (
    <Stack gap={8}>
      <Group gap={8} align="flex-end" wrap="nowrap">
        <TextInput
          style={{ flex: 1 }}
          label={label}
          placeholder="https://stock.naver.com/domestic/stock/005930/discussion/424274129"
          value={linkInput}
          onChange={(e) => onLinkInput(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onAddLink();
            }
          }}
          leftSection={<Icon.link size={14} />}
          aria-label={label}
        />
        <Button
          variant="light"
          color={color}
          onClick={onAddLink}
          disabled={!linkInput.trim()}
        >
          추가
        </Button>
      </Group>
      {links.length > 0 ? (
        <Group gap={6}>
          {links.map((link) => (
            <Badge
              key={link}
              color={color}
              variant="light"
              radius="xl"
              size="lg"
              rightSection={
                <ActionIcon
                  size={15}
                  variant="transparent"
                  color={color}
                  aria-label={`${link} 제거`}
                  onClick={() => onRemoveLink(link)}
                >
                  <Icon.x size={11} />
                </ActionIcon>
              }
            >
              {postLabel(link)}
            </Badge>
          ))}
        </Group>
      ) : (
        <Text fz={12} c="orange.7">
          게시글 링크를 추가하세요.
        </Text>
      )}
    </Stack>
  );
}
