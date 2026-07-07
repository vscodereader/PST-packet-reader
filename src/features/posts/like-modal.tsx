import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Loader,
  Modal,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useState } from "react";

import { isPostable } from "@/shared/data/config";
import type { Account } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import type { LikeOutcome } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

import { AccountRow } from "./publish-modal";

export interface LikeModalProps {
  open: boolean;
  onClose: () => void;
  /** 반응 종류 — `"good"`=좋아요(기본) / `"bad"`=싫어요. 좋아요/싫어요가 패킷상 reactionType만
   * 다르므로(URL·헤더 동일) 이 모달 하나를 공유한다. */
  reaction?: "good" | "bad";
}

/** 게시글 링크에서 사람이 읽을 postId(끝의 숫자)를 뽑는다. 칩 라벨용(없으면 링크 자체). */
function postLabel(url: string): string {
  const m = url.match(/\/discussion\/(\d+)/);
  return m ? `글 #${m[1]}` : url;
}

/** 글 관리 화면의 "좋아요"/"싫어요" 버튼이 여는 모달(`reaction` prop으로 공유).
 *
 * "특정 게시글 댓글"처럼 게시글 링크를 여러 개 넣을 수 있다(엔터/추가 → 칩으로 쌓이고 입력칸이
 * 비워진다). 아래에는 댓글 대신 로그인된 종목토론방 계정을 체크박스([`AccountRow`] 재사용)로
 * 고른다. 버튼을 누르면 선택한 계정들이 넣은 링크 글마다 반응을 누른다(페이지 이동 없이
 * reactions API 전용 — 백엔드 `like_discussion_post`/`dislike_discussion_post`). 완료 시 토스트로
 * 성공/실패 수를 알린다. 좋아요=빨강, 싫어요=남색으로만 구분(로직·화면 동일).
 */
export function LikeModal({
  open,
  onClose,
  reaction = "good",
}: LikeModalProps) {
  const isDislike = reaction === "bad";
  const label = isDislike ? "싫어요" : "좋아요";
  const color = isDislike ? "indigo" : "red";

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [links, setLinks] = useState<string[]>([]);
  const [linkInput, setLinkInput] = useState("");
  const [flow, setFlow] = useState<null | "running" | LikeOutcome[]>(null);

  useEffect(() => {
    if (!open) return;
    void ipc.accounts.list().then(setAccounts);
  }, [open]);

  // 입력칸의 링크를 목록에 추가한다(중복 제거, trim). 추가 후 입력칸을 비운다.
  const addLink = () => {
    const link = linkInput.trim();
    if (!link) return;
    setLinks((prev) => (prev.includes(link) ? prev : [...prev, link]));
    setLinkInput("");
  };
  const removeLink = (link: string) =>
    setLinks((prev) => prev.filter((l) => l !== link));

  // 반응은 종목토론방(네이버 증권) 로그인 계정으로만 누른다 — 게시 가능한 상태(active/new)만.
  const forumAccounts = accounts.filter(
    (a) => a.platform === "forum" && isPostable(a.status),
  );
  const toggle = (id: string) =>
    setSelected((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  const allOn =
    forumAccounts.length > 0 &&
    forumAccounts.every((a) => selected.includes(a.id));
  const toggleAll = () =>
    setSelected((s) =>
      allOn
        ? s.filter((id) => !forumAccounts.some((a) => a.id === id))
        : [...new Set([...s, ...forumAccounts.map((a) => a.id)])],
    );

  // 체크박스는 계정 id로 다루고(AccountRow 규약), 백엔드에는 쿠키 키인 loginId로 넘긴다.
  const selectedLoginIds = selected
    .map((id) => accounts.find((a) => a.id === id))
    .filter((a): a is Account => !!a)
    .map((a) => a.loginId);

  const running = flow === "running";
  const results = Array.isArray(flow) ? flow : [];
  const canSubmit = links.length > 0 && selectedLoginIds.length > 0 && !running;

  const submit = async () => {
    if (!canSubmit) return;
    setFlow("running");
    try {
      const outcomes = isDislike
        ? await ipc.forum.dislike(links, selectedLoginIds)
        : await ipc.forum.like(links, selectedLoginIds);
      setFlow(outcomes);
      const ok = outcomes.filter((o) => o.success).length;
      notifications.show({
        message: `${label} ${outcomes.length}건 중 ${ok}건 성공`,
        color: ok === outcomes.length ? "green" : ok === 0 ? "red" : "yellow",
      });
    } catch (err) {
      notifications.show({
        message:
          `${label} 실패: ` +
          (err instanceof Error ? err.message : String(err)),
        color: "red",
      });
      setFlow(null);
    }
  };

  const close = () => {
    if (running) return;
    setLinks([]);
    setLinkInput("");
    setSelected([]);
    setFlow(null);
    onClose();
  };

  const okCount = results.filter((r) => r.success).length;

  return (
    <Modal
      opened={open}
      onClose={close}
      title={
        <Group gap={8}>
          <ThemeIcon size={26} radius="xl" variant="light" color={color}>
            <Icon.heart size={16} />
          </ThemeIcon>
          <Text fw={800} fz={17}>
            {label}
          </Text>
        </Group>
      }
      size={520}
      radius="lg"
      centered
    >
      <Stack gap={16}>
        <Text fz={13} c="dimmed">
          게시글 링크를 넣고(여러 개 가능) 계정을 고르면, 선택한 계정들이 그
          글들에 {label}를 누릅니다(페이지 이동 없이 바로 처리).
        </Text>

        {/* "특정 게시글 댓글"처럼 링크를 여러 개 추가 — 엔터/추가 → 칩으로 쌓이고 입력칸 비움. */}
        <Stack gap={8}>
          <Group gap={8} align="flex-end" wrap="nowrap">
            <TextInput
              style={{ flex: 1 }}
              label="게시글 링크"
              placeholder="https://stock.naver.com/domestic/stock/005930/discussion/424274129"
              value={linkInput}
              onChange={(e) => setLinkInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addLink();
                }
              }}
              leftSection={<Icon.link size={14} />}
              aria-label={`${label}를 누를 게시글 링크`}
            />
            <Button
              variant="light"
              color={color}
              onClick={addLink}
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
                      onClick={() => removeLink(link)}
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
              {label}를 누를 게시글 링크를 추가하세요.
            </Text>
          )}
        </Stack>

        <Divider label={`${label}를 누를 계정`} labelPosition="left" />

        {/* 종목선택 화면과 같은 계정 체크박스(AccountRow) 재사용. */}
        <Stack gap={4}>
          {forumAccounts.length === 0 ? (
            <Group gap={8} px={4} py={10}>
              <PlatformLogo id="forum" size={22} />
              <Text fz={13} c="orange.7">
                {label}를 누를 수 있는 종목토론방 로그인 계정이 없습니다. 먼저
                로그인하세요.
              </Text>
            </Group>
          ) : (
            <>
              <Group justify="space-between" px={4}>
                <Text fz={12} c="dimmed">
                  {selectedLoginIds.length}/{forumAccounts.length}개 선택됨
                </Text>
                <Button
                  size="compact-xs"
                  variant="subtle"
                  color="gray"
                  onClick={toggleAll}
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
                }}
              >
                {forumAccounts.map((a) => (
                  <AccountRow
                    key={a.id}
                    a={a}
                    selected={selected.includes(a.id)}
                    onToggle={toggle}
                  />
                ))}
              </Box>
            </>
          )}
        </Stack>

        {/* 결과 패널: (계정×링크)별 성공/실패를 보여준다. */}
        {results.length > 0 && (
          <Stack gap={6}>
            <Text fz={13} fw={700}>
              {results.length}개 중 {okCount}개 성공
            </Text>
            <Stack gap={4} style={{ maxHeight: 160, overflowY: "auto" }}>
              {results.map((r, i) => (
                <Group
                  key={`${r.accountId}-${r.postUrl}-${i}`}
                  gap={8}
                  px={10}
                  py={7}
                  wrap="nowrap"
                  style={{
                    borderRadius: "var(--mantine-radius-sm)",
                    border: "1px solid var(--mantine-color-gray-2)",
                    background: "var(--mantine-color-gray-0)",
                  }}
                >
                  <ThemeIcon
                    size={20}
                    radius="xl"
                    variant="light"
                    color={r.success ? "green" : "red"}
                  >
                    {r.success ? (
                      <Icon.checkCircle size={13} />
                    ) : (
                      <Icon.alert size={13} />
                    )}
                  </ThemeIcon>
                  <Text
                    fz={12.5}
                    fw={700}
                    ff="monospace"
                    style={{ flexShrink: 0 }}
                  >
                    {r.accountId}
                  </Text>
                  <Badge size="xs" variant="default" radius="sm">
                    {postLabel(r.postUrl)}
                  </Badge>
                  <Text fz={11.5} c={r.success ? "dimmed" : "red"} truncate>
                    {r.message}
                  </Text>
                </Group>
              ))}
            </Stack>
          </Stack>
        )}

        <Group justify="flex-end" gap={9}>
          <Button variant="default" onClick={close} disabled={running}>
            닫기
          </Button>
          <Button
            color={color}
            leftSection={
              running ? (
                <Loader size={14} color="white" />
              ) : (
                <Icon.heart size={16} />
              )
            }
            onClick={submit}
            disabled={!canSubmit}
          >
            {running ? `${label} 누르는 중…` : label}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
