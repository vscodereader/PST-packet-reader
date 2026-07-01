import {
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
}

/** 글 관리 화면의 "좋아요" 버튼이 여는 모달.
 *
 * "특정 게시글 댓글"과 같은 링크 입력 UI를 재사용하되, 아래에는 댓글 대신 로그인된
 * 종목토론방 계정을 체크박스([`AccountRow`] 재사용)로 고르게 한다. 링크(게시글)와 계정을
 * 고른 뒤 "좋아요"를 누르면, 선택한 계정들이 각각 그 글에 좋아요를 누른다(페이지 이동 없이
 * reactions API 전용 — 백엔드 `like_discussion_post`). 계정별 성공/실패를 결과 패널에 보여준다.
 */
export function LikeModal({ open, onClose }: LikeModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [link, setLink] = useState("");
  const [flow, setFlow] = useState<null | "running" | LikeOutcome[]>(null);

  useEffect(() => {
    if (!open) return;
    void ipc.accounts.list().then(setAccounts);
  }, [open]);

  // 좋아요는 종목토론방(네이버 증권) 로그인 계정으로만 누른다 — 게시 가능한 상태(active/new)만.
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
  const canSubmit =
    link.trim().length > 0 && selectedLoginIds.length > 0 && !running;

  const submit = async () => {
    if (!canSubmit) return;
    setFlow("running");
    try {
      const outcomes = await ipc.forum.like(link.trim(), selectedLoginIds);
      setFlow(outcomes);
    } catch (err) {
      notifications.show({
        message:
          "좋아요 실패: " + (err instanceof Error ? err.message : String(err)),
        color: "red",
      });
      setFlow(null);
    }
  };

  const close = () => {
    if (running) return;
    setLink("");
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
          <ThemeIcon size={26} radius="xl" variant="light" color="red">
            <Icon.heart size={16} />
          </ThemeIcon>
          <Text fw={800} fz={17}>
            좋아요
          </Text>
        </Group>
      }
      size={520}
      radius="lg"
      centered
    >
      <Stack gap={16}>
        <Text fz={13} c="dimmed">
          게시글 링크를 넣고 계정을 고르면, 선택한 계정들이 그 글에 좋아요를
          누릅니다(페이지 이동 없이 바로 처리).
        </Text>

        {/* "특정 게시글 댓글"과 동일한 링크 입력 UI 재사용. */}
        <TextInput
          label="게시글 링크"
          placeholder="https://stock.naver.com/domestic/stock/005930/discussion/424274129"
          value={link}
          onChange={(e) => setLink(e.currentTarget.value)}
          leftSection={<Icon.link size={14} />}
          aria-label="좋아요를 누를 게시글 링크"
        />

        <Divider label="좋아요를 누를 계정" labelPosition="left" />

        {/* 종목선택 화면과 같은 계정 체크박스(AccountRow) 재사용. */}
        <Stack gap={4}>
          {forumAccounts.length === 0 ? (
            <Group gap={8} px={4} py={10}>
              <PlatformLogo id="forum" size={22} />
              <Text fz={13} c="orange.7">
                좋아요를 누를 수 있는 종목토론방 로그인 계정이 없습니다. 먼저
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

        {/* 결과 패널: 좋아요를 누른 뒤 계정별 성공/실패를 보여준다. */}
        {results.length > 0 && (
          <Stack gap={6}>
            <Text fz={13} fw={700}>
              {results.length}개 중 {okCount}개 성공
            </Text>
            <Stack gap={4} style={{ maxHeight: 160, overflowY: "auto" }}>
              {results.map((r) => (
                <Group
                  key={r.accountId}
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
            color="red"
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
            {running ? "좋아요 누르는 중…" : "좋아요"}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
