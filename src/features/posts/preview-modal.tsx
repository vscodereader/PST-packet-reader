import { Avatar, Box, Group, Modal, Text } from "@mantine/core";
import { useState } from "react";

import { PLATFORM } from "@/shared/data/config";
import { resolveTemplate } from "@/shared/data/helpers";
import type { CommentTarget, ModeValue, PublishJob } from "@/shared/data/types";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

export interface PreviewModalProps {
  open: boolean;
  onClose: () => void;
  mode: ModeValue;
  title: string;
  body: string;
  comments: string[];
  jobs: PublishJob[];
  linkOverride: string;
  commentTarget?: CommentTarget;
  commentCount?: number;
}

const FALLBACK: PublishJob = {
  key: "none",
  platform: "forum",
  loginId: "계정",
  targetName: "대상 미선택",
  board: "종목토론방",
  status: "active",
};

function PreviewModalInner({
  open,
  onClose,
  mode,
  title,
  body,
  comments,
  jobs,
  linkOverride,
  commentTarget,
  commentCount,
}: PreviewModalProps) {
  const list = jobs.length ? jobs : [FALLBACK];
  const [tab, setTab] = useState(list[0]?.key ?? "none");

  const j = list.find((x) => x.key === tab) ?? list[0]!;
  const p = PLATFORM[j.platform];
  const color = `var(--mantine-color-${p?.color ?? "gray"}-6)`;
  const isForum = j.platform === "forum";
  const showPost = mode === "post" || mode === "both";
  const showComments = mode === "comment" || mode === "both";

  const resolvedTitle = resolveTemplate(title, j, linkOverride);
  const html = resolveTemplate(body, j, linkOverride);
  const idx = list.indexOf(j);
  const sampleRaw = comments.length
    ? [comments[idx % comments.length] ?? ""]
    : [];
  const sample = sampleRaw
    .map((c) => resolveTemplate(c, j, linkOverride))
    .filter(Boolean);

  const commentLine =
    commentTarget === "url"
      ? `${j.targetName} 지정 게시글`
      : `${j.targetName} ${isForum ? "종목토론방 " : ""}${
          commentTarget === "popular" ? "인기글" : "최신글"
        }${commentCount ? ` ${commentCount}개` : ""}`;

  return (
    <Modal
      opened={open}
      onClose={onClose}
      title="미리보기"
      size={620}
      radius="lg"
    >
      <Group gap={6} mb={16} style={{ maxHeight: 96, overflowY: "auto" }}>
        {list.map((t) => {
          const on = t.key === tab;
          return (
            <TabPill key={t.key} on={on} onClick={() => setTab(t.key)}>
              <PlatformLogo id={t.platform} size={17} /> {t.targetName}
            </TabPill>
          );
        })}
      </Group>

      <Box
        style={{
          border: "1px solid var(--mantine-color-gray-2)",
          borderRadius: "var(--mantine-radius-md)",
          overflow: "hidden",
          boxShadow: "var(--mantine-shadow-sm)",
        }}
      >
        <Group
          h={46}
          px={16}
          gap={9}
          wrap="nowrap"
          style={{ background: color }}
        >
          <PlatformLogo id={j.platform} size={24} />
          <Group gap={7} wrap="nowrap">
            <Text fz={13.5} fw={700} c="white">
              {j.targetName}
            </Text>
            {isForum && j.code && (
              <Text
                fz={11}
                fw={700}
                c="white"
                ff="monospace"
                px={6}
                style={{
                  background: "rgba(255,255,255,.22)",
                  borderRadius: 4,
                }}
              >
                {j.code}
              </Text>
            )}
          </Group>
          <Text
            fz={12}
            c="white"
            px={9}
            py={3}
            ml="auto"
            style={{ background: "rgba(255,255,255,.2)", borderRadius: 999 }}
          >
            {j.board}
          </Text>
        </Group>

        <Box bg="white" px={20} pt={18} pb={22}>
          {showPost && (
            <>
              <Text fz={19} fw={800} c="dark.8" mb={10}>
                {resolvedTitle || "제목 없음"}
              </Text>
              <Group
                gap={8}
                mb={14}
                pb={14}
                style={{
                  borderBottom: "1px solid var(--mantine-color-gray-1)",
                }}
              >
                <Avatar size={28} radius="xl" color={p?.color ?? "gray"}>
                  {j.loginId.slice(0, 1).toUpperCase()}
                </Avatar>
                <Box>
                  <Text fz={12.5} fw={700} c="dark.8">
                    {j.loginId}
                  </Text>
                  <Text fz={11} c="gray.5">
                    방금 전 · 조회 0
                  </Text>
                </Box>
              </Group>
              <Box
                className="preview-body"
                fz={14.5}
                c="gray.8"
                style={{ lineHeight: 1.75 }}
                dangerouslySetInnerHTML={{
                  __html:
                    html ||
                    "<p style='color:#adb5bd'>내용 미리보기가 여기에 표시됩니다.</p>",
                }}
              />
            </>
          )}

          {showComments && (
            <Box
              mt={showPost ? 18 : 0}
              pt={showPost ? 16 : 0}
              style={
                showPost
                  ? { borderTop: "1px solid var(--mantine-color-gray-1)" }
                  : {}
              }
            >
              {!showPost && (
                <Group gap={6} mb={12}>
                  <Icon.target size={14} color="var(--mantine-color-gray-6)" />
                  <Text fz={13} c="gray.6">
                    {commentLine}
                  </Text>
                </Group>
              )}
              <Text fz={12.5} fw={700} c="gray.6" mb={12}>
                댓글 {sample.length}
              </Text>
              {sample.length === 0 && (
                <Text fz={13} c="gray.5">
                  등록할 댓글 내용을 입력하세요.
                </Text>
              )}
              {sample.map((c, i) => (
                <Group key={i} gap={9} mb={12} align="flex-start" wrap="nowrap">
                  <Avatar size={26} radius="xl" color={p?.color ?? "gray"}>
                    {j.loginId.slice(0, 1).toUpperCase()}
                  </Avatar>
                  <Box style={{ flex: 1 }}>
                    <Text fz={12} fw={700} c="dark.8" mb={2}>
                      {j.loginId}{" "}
                      <Text component="span" fw={500} c="gray.5" ml={4}>
                        방금 전
                      </Text>
                    </Text>
                    <Text fz={13.5} c="gray.8" style={{ lineHeight: 1.55 }}>
                      {c}
                    </Text>
                  </Box>
                </Group>
              ))}
              {comments.filter(Boolean).length > 1 && (
                <Text fz={11.5} c="gray.5" mt={4}>
                  그 외 {comments.filter(Boolean).length - 1}종의 댓글이 다른
                  계정에 무작위 배분됩니다.
                </Text>
              )}
            </Box>
          )}
        </Box>
      </Box>

      <Group gap={7} mt={14}>
        <Icon.eye size={15} color="var(--mantine-color-gray-5)" />
        <Text fz={12.5} c="gray.5">
          선택한 {list.length}곳에 각 게시판 서식으로{" "}
          {mode === "comment" ? "댓글이 등록" : "게시"}됩니다.
        </Text>
      </Group>
    </Modal>
  );
}

// Pill-style tab toggle used in the preview header.
function TabPill({
  on,
  onClick,
  children,
}: {
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Box
      component="button"
      onClick={onClick}
      style={{
        display: "inline-flex",
        alignItems: "center",
        gap: 7,
        height: 34,
        padding: "0 12px",
        borderRadius: 999,
        cursor: "pointer",
        border: on
          ? "1px solid transparent"
          : "1px solid var(--mantine-color-gray-3)",
        background: on
          ? "var(--mantine-color-dark-6)"
          : "var(--mantine-color-body)",
        color: on ? "#fff" : "var(--mantine-color-gray-7)",
        fontSize: 12.5,
        fontWeight: 600,
      }}
    >
      {children}
    </Box>
  );
}

export function PreviewModal(props: PreviewModalProps) {
  // Remount per open so the active tab re-seeds from the first job.
  return <PreviewModalInner key={props.open ? "open" : "closed"} {...props} />;
}
