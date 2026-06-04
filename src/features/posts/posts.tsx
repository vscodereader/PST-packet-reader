import {
  Badge,
  Box,
  Button,
  Card,
  Center,
  Container,
  Group,
  Menu,
  Pagination,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { KIND, KIND_ICON, STATUS_LABEL } from "@/shared/data/config";
import type { GoFn, LibraryPost, ModeValue } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

import { PublishModal } from "./publish-modal";
import { WriterModal } from "./writer-modal";

const PER_PAGE = 10;

function toast(message: string, color = "blue") {
  notifications.show({ message, color, autoClose: 2400 });
}

export function Posts({ go }: { go: GoFn }) {
  const [posts, setPosts] = useState<LibraryPost[]>([]);

  useEffect(() => {
    void ipc.posts.list().then(setPosts);
  }, []);
  const [filter, setFilter] = useState<"all" | ModeValue>("all");
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);
  const [writerOpen, setWriterOpen] = useState(false);
  const [writerDoc, setWriterDoc] = useState<LibraryPost | null>(null);
  const [publishDoc, setPublishDoc] = useState<LibraryPost | null>(null);

  const upsert = (doc: LibraryPost) => {
    void ipc.posts.upsert(doc).then(setPosts);
  };
  const openNew = () => {
    setWriterDoc(null);
    setWriterOpen(true);
  };
  const openEdit = (d: LibraryPost) => {
    setWriterDoc(d);
    setWriterOpen(true);
  };
  const dup = (d: LibraryPost) => {
    upsert({
      ...d,
      id: "p" + Date.now(),
      title: d.title + " (복사본)",
      // Keep the copy out of the drafts-only flow so it stays in the library.
      status: "ready",
      updated: "방금 전",
    });
    toast("복제했어요", "green");
  };
  const del = (id: string) => {
    void ipc.posts.remove(id).then(setPosts);
    toast("삭제했어요");
  };

  // Drafts are managed only in the writer's 임시저장 panel, not this library list.
  const visible = posts.filter((p) => p.status !== "draft");
  const tabs: { v: "all" | ModeValue; t: string; n: number }[] = [
    { v: "all", t: "전체", n: visible.length },
    { v: "post", t: "글", n: visible.filter((p) => p.kind === "post").length },
    {
      v: "comment",
      t: "댓글",
      n: visible.filter((p) => p.kind === "comment").length,
    },
    {
      v: "both",
      t: "글+댓글",
      n: visible.filter((p) => p.kind === "both").length,
    },
  ];
  const filtered = visible.filter(
    (p) =>
      (filter === "all" || p.kind === filter) && (!q || p.title.includes(q)),
  );
  const totalPages = Math.max(1, Math.ceil(filtered.length / PER_PAGE));
  const curPage = Math.min(page, totalPages);
  const items = filtered.slice((curPage - 1) * PER_PAGE, curPage * PER_PAGE);

  return (
    <Container size={1020} py={32} px={36}>
      <Group justify="space-between" align="flex-end" mb={22} wrap="wrap">
        <Box>
          <Title order={1} fz={25} fw={800}>
            글 관리
          </Title>
          <Text size="sm" c="dimmed" mt={6}>
            작성한 글·댓글을 모아두고, 원하는 글을 골라 여러 계정에 게시하세요.
          </Text>
        </Box>
        <Group gap="xs">
          <Button
            size="sm"
            variant="default"
            leftSection={<Icon.inbox size={16} />}
            onClick={async () => {
              const path = await open({
                multiple: false,
                filters: [{ name: "Excel", extensions: ["xlsx"] }],
              });
              if (typeof path !== "string") return;
              try {
                const summary = await ipc.excel.importPosts(path);
                setPosts(await ipc.posts.list());
                toast(
                  `${summary.imported}건 가져옴${summary.skipped ? `, ${summary.skipped}건 건너뜀` : ""}`,
                  "green",
                );
              } catch (err) {
                toast(
                  "가져오기 실패: " +
                    (err instanceof Error ? err.message : String(err)),
                  "red",
                );
                void ipc.activity.append(
                  "error",
                  "게시글 가져오기 실패 — " +
                    (err instanceof Error ? err.message : String(err)),
                );
              }
            }}
          >
            엑셀 가져오기
          </Button>
          <Button
            size="sm"
            leftSection={<Icon.pencil size={18} />}
            onClick={openNew}
          >
            글쓰기
          </Button>
        </Group>
      </Group>

      <Group justify="space-between" mb={18} wrap="wrap">
        <Group gap={6}>
          {tabs.map((t) => (
            <Button
              key={t.v}
              size="xs"
              radius="xl"
              variant={t.v === filter ? "filled" : "default"}
              color={t.v === filter ? "dark" : "gray"}
              onClick={() => {
                setFilter(t.v);
                setPage(1);
              }}
            >
              {t.t}
              <Text component="span" ml={6} fz={11} opacity={0.7}>
                {t.n}
              </Text>
            </Button>
          ))}
        </Group>
        <TextInput
          size="sm"
          w={240}
          placeholder="제목 검색"
          leftSection={<Icon.search size={17} />}
          value={q}
          onChange={(e) => {
            setQ(e.currentTarget.value);
            setPage(1);
          }}
        />
      </Group>

      <Stack gap={10}>
        {items.map((d) => {
          const st = STATUS_LABEL[d.status] ?? { t: d.status, c: "gray" };
          const kd = KIND[d.kind] ?? KIND.post!;
          const KI =
            Icon[(KIND_ICON[d.kind] ?? "fileText") as keyof typeof Icon];
          const isComment = d.kind === "comment";
          const meta = isComment
            ? `댓글 ${(d.comments ?? []).filter(Boolean).length}종`
            : `${d.words}자`;
          return (
            <Card
              key={d.id}
              withBorder
              padding="md"
              radius="md"
              onClick={() => openEdit(d)}
              style={{
                display: "flex",
                flexDirection: "row",
                alignItems: "center",
                gap: 16,
                cursor: "pointer",
              }}
            >
              <ThemeIcon
                size={44}
                radius="md"
                variant="light"
                color={isComment ? "forum" : "gray"}
              >
                <KI size={21} />
              </ThemeIcon>
              <Box style={{ flex: 1, minWidth: 0 }}>
                <Group gap={8} mb={4} wrap="nowrap">
                  <Badge size="sm" color={kd.c} variant="light">
                    {kd.t}
                  </Badge>
                  <Text fz={15} fw={700} truncate>
                    {d.title}
                  </Text>
                </Group>
                <Text fz={13} c="dimmed" truncate mb={8}>
                  {d.excerpt}
                </Text>
                <Group gap={9}>
                  <Badge size="sm" color={st.c} variant="light">
                    {st.t}
                  </Badge>
                  <Text fz={11.5} c="dimmed">
                    {meta} · {d.updated}
                  </Text>
                </Group>
              </Box>
              <Button
                size="sm"
                leftSection={<Icon.send size={15} />}
                onClick={(e) => {
                  e.stopPropagation();
                  setPublishDoc(d);
                }}
              >
                게시하기
              </Button>
              <Menu position="bottom-end" width={150}>
                <Menu.Target>
                  <Button
                    size="xs"
                    variant="subtle"
                    color="gray"
                    px={8}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Icon.dots size={18} />
                  </Button>
                </Menu.Target>
                <Menu.Dropdown onClick={(e) => e.stopPropagation()}>
                  <Menu.Item
                    leftSection={<Icon.pencil size={16} />}
                    onClick={() => openEdit(d)}
                  >
                    편집
                  </Menu.Item>
                  <Menu.Item
                    leftSection={<Icon.copy size={16} />}
                    onClick={() => dup(d)}
                  >
                    복제
                  </Menu.Item>
                  <Menu.Item
                    color="red"
                    leftSection={<Icon.trash size={16} />}
                    onClick={() => del(d.id)}
                  >
                    삭제
                  </Menu.Item>
                </Menu.Dropdown>
              </Menu>
            </Card>
          );
        })}
        {items.length === 0 && (
          <Center py={60}>
            <Stack align="center" gap={4}>
              <Icon.fileText size={40} color="var(--mantine-color-gray-5)" />
              <Text fw={600} c="gray.7">
                글이 없어요
              </Text>
              <Text size="sm" c="dimmed" mb={14}>
                새 글을 작성해 목록에 추가해보세요.
              </Text>
              <Button
                size="sm"
                leftSection={<Icon.pencil size={16} />}
                onClick={openNew}
              >
                글쓰기
              </Button>
            </Stack>
          </Center>
        )}
      </Stack>

      {filtered.length > 0 && (
        <Group justify="space-between" mt={20}>
          <Text size="xs" c="dimmed">
            총 {filtered.length}개 · {curPage}/{totalPages} 페이지
          </Text>
          <Pagination
            size="sm"
            total={totalPages}
            value={curPage}
            onChange={setPage}
          />
        </Group>
      )}

      <WriterModal
        open={writerOpen}
        doc={writerDoc}
        drafts={posts.filter((p) => p.status === "draft")}
        onClose={() => setWriterOpen(false)}
        onSave={(doc) => {
          upsert(doc);
          setWriterOpen(false);
          toast("글을 저장했어요", "green");
        }}
        onSaveDraft={upsert}
        onDeleteDraft={(d) => {
          void ipc.posts.remove(d.id).then(setPosts);
          toast("임시저장을 삭제했어요");
        }}
      />
      <PublishModal
        open={!!publishDoc}
        doc={publishDoc}
        onClose={() => setPublishDoc(null)}
        go={go}
      />
    </Container>
  );
}
