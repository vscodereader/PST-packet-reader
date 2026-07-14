// 네이버 블로그 편집기 미러 — 본문을 블록 배열로 편집한다. 상단 삽입 툴바 7개(사진·스티커·링크·
// 파일·일정·소스코드·장소)와 텍스트 블록 하단 서식 툴바(B·I·U·취소선·정렬)를 제공한다. 삽입 중
// 보조 데이터가 필요한 블록(링크/스티커/장소/사진/파일)은 계정 쿠키로 백엔드 보조 API를 호출해
// 채운다. 발행 시 이 블록 배열이 documentModel components[]로 변환된다(백엔드 document_model.rs).

import {
  ActionIcon,
  Box,
  Button,
  Group,
  Image,
  Loader,
  Modal,
  Paper,
  ScrollArea,
  SegmentedControl,
  Stack,
  Text,
  TextInput,
  Textarea,
  Tooltip,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import {
  IconAlignCenter,
  IconAlignJustified,
  IconAlignLeft,
  IconAlignRight,
  IconBold,
  IconCalendarEvent,
  IconChevronDown,
  IconChevronUp,
  IconCode,
  IconItalic,
  IconLink,
  IconMapPin,
  IconMoodSmile,
  IconPaperclip,
  IconPhoto,
  IconStrikethrough,
  IconTrash,
  IconUnderline,
} from "@tabler/icons-react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { ipc } from "@/shared/ipc";
import type { PlaceResult, StickerPack } from "@/shared/ipc";

import {
  createCodeBlock,
  createFileBlock,
  createImageBlock,
  createOglinkBlock,
  createPlacesMapBlock,
  createScheduleBlock,
  createStickerBlock,
  createTextBlock,
  moveBlock,
  removeBlock,
  setAlign,
  setCode,
  setText,
  toggleMark,
  type Align,
  type Block,
  type TextBlock,
  type TextMark,
} from "./blocks";

/** 정렬 옵션(서식 툴바 세그먼트). */
const ALIGNS: { value: Align; icon: typeof IconAlignLeft; label: string }[] = [
  { value: "left", icon: IconAlignLeft, label: "왼쪽 정렬" },
  { value: "center", icon: IconAlignCenter, label: "가운데 정렬" },
  { value: "right", icon: IconAlignRight, label: "오른쪽 정렬" },
  { value: "justify", icon: IconAlignJustified, label: "양끝 정렬" },
];

/** 서식 마크 버튼(B·I·U·취소선). */
const MARKS: { key: TextMark; icon: typeof IconBold; label: string }[] = [
  { key: "bold", icon: IconBold, label: "굵게" },
  { key: "italic", icon: IconItalic, label: "기울임" },
  { key: "underline", icon: IconUnderline, label: "밑줄" },
  { key: "strikeThrough", icon: IconStrikethrough, label: "취소선" },
];

interface BlockEditorProps {
  /** 보조 API 호출에 쓰는 계정(없으면 삽입 API 비활성). */
  accountId: string | null;
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
}

type ModalKind = null | "link" | "sticker" | "schedule" | "place";

export function BlockEditor({ accountId, blocks, onChange }: BlockEditorProps) {
  const [modal, setModal] = useState<ModalKind>(null);
  const [busy, setBusy] = useState(false);

  function requireAccount(): string | null {
    if (!accountId) {
      notifications.show({ color: "red", message: "계정을 먼저 선택하세요." });
      return null;
    }
    return accountId;
  }

  function append(block: Block) {
    onChange([...blocks, block]);
  }

  async function onInsertPhoto() {
    const id = requireAccount();
    if (!id) return;
    const path = await pickFile("이미지");
    if (!path) return;
    setBusy(true);
    try {
      const img = await ipc.blog.uploadPhoto(id, path);
      append(createImageBlock(img));
    } catch (e) {
      notifications.show({
        color: "red",
        message: `사진 업로드 실패: ${String(e)}`,
      });
    } finally {
      setBusy(false);
    }
  }

  async function onInsertFile() {
    const id = requireAccount();
    if (!id) return;
    const path = await pickFile("파일");
    if (!path) return;
    setBusy(true);
    try {
      const f = await ipc.blog.uploadFile(id, path);
      append(createFileBlock(f));
    } catch (e) {
      notifications.show({
        color: "red",
        message: `파일 업로드 실패: ${String(e)}`,
      });
    } finally {
      setBusy(false);
    }
  }

  const insertButtons: {
    label: string;
    icon: typeof IconPhoto;
    onClick: () => void;
  }[] = [
    { label: "사진", icon: IconPhoto, onClick: () => void onInsertPhoto() },
    {
      label: "스티커",
      icon: IconMoodSmile,
      onClick: () => setModal("sticker"),
    },
    { label: "링크", icon: IconLink, onClick: () => setModal("link") },
    { label: "파일", icon: IconPaperclip, onClick: () => void onInsertFile() },
    {
      label: "일정",
      icon: IconCalendarEvent,
      onClick: () => setModal("schedule"),
    },
    {
      label: "소스코드",
      icon: IconCode,
      onClick: () => append(createCodeBlock()),
    },
    { label: "장소", icon: IconMapPin, onClick: () => setModal("place") },
  ];

  return (
    <Stack gap="sm">
      <Group gap="xs" wrap="wrap">
        {insertButtons.map((b) => (
          <Button
            key={b.label}
            size="xs"
            variant="light"
            leftSection={<b.icon size={16} />}
            onClick={b.onClick}
            disabled={busy}
          >
            {b.label}
          </Button>
        ))}
        {busy && <Loader size="xs" />}
      </Group>

      <Stack gap="sm">
        {blocks.length === 0 && (
          <Text size="sm" c="dimmed">
            위 툴바에서 블록을 추가하거나 아래 &quot;문단 추가&quot;로 글을
            작성하세요.
          </Text>
        )}
        {blocks.map((block) => (
          <BlockRow
            key={block.id}
            block={block}
            onChange={onChange}
            blocks={blocks}
          />
        ))}
      </Stack>

      <Group>
        <Button
          size="xs"
          variant="default"
          onClick={() => append(createTextBlock())}
        >
          문단 추가
        </Button>
      </Group>

      <LinkModal
        opened={modal === "link"}
        accountId={accountId}
        onClose={() => setModal(null)}
        onInsert={(link, meta) => {
          append(createOglinkBlock(link, meta));
          setModal(null);
        }}
      />
      <ScheduleModal
        opened={modal === "schedule"}
        onClose={() => setModal(null)}
        onInsert={(title, startAt, dateOnly) => {
          append(createScheduleBlock(title, startAt, dateOnly));
          setModal(null);
        }}
      />
      <StickerModal
        opened={modal === "sticker"}
        accountId={accountId}
        onClose={() => setModal(null)}
        onInsert={(packCode, seq) => {
          append(createStickerBlock(packCode, seq));
          setModal(null);
        }}
      />
      <PlaceModal
        opened={modal === "place"}
        accountId={accountId}
        onClose={() => setModal(null)}
        onInsert={(thumbnailSrc, place) => {
          append(createPlacesMapBlock(thumbnailSrc, place));
          setModal(null);
        }}
      />
    </Stack>
  );
}

/** Tauri 파일 선택 다이얼로그로 로컬 경로 1개를 고른다(취소 시 null). */
async function pickFile(kind: string): Promise<string | null> {
  try {
    const selected = await openFileDialog({
      multiple: false,
      directory: false,
      title: `${kind} 선택`,
    });
    if (typeof selected === "string") return selected;
    return null;
  } catch {
    return null;
  }
}

/** 블록 1개의 행 — 이동/삭제 컨트롤 + 타입별 편집/미리보기. */
function BlockRow({
  block,
  blocks,
  onChange,
}: {
  block: Block;
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
}) {
  return (
    <Paper withBorder p="xs" radius="sm">
      <Group justify="space-between" align="flex-start" gap="xs" mb={6}>
        <Text size="xs" c="dimmed">
          {blockLabel(block)}
        </Text>
        <Group gap={2}>
          <Tooltip label="위로">
            <ActionIcon
              variant="subtle"
              size="sm"
              aria-label="위로"
              onClick={() => onChange(moveBlock(blocks, block.id, -1))}
            >
              <IconChevronUp size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="아래로">
            <ActionIcon
              variant="subtle"
              size="sm"
              aria-label="아래로"
              onClick={() => onChange(moveBlock(blocks, block.id, 1))}
            >
              <IconChevronDown size={16} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="삭제">
            <ActionIcon
              variant="subtle"
              color="red"
              size="sm"
              aria-label="삭제"
              onClick={() => onChange(removeBlock(blocks, block.id))}
            >
              <IconTrash size={16} />
            </ActionIcon>
          </Tooltip>
        </Group>
      </Group>
      <BlockBody block={block} blocks={blocks} onChange={onChange} />
    </Paper>
  );
}

/** 블록 타입 라벨. */
function blockLabel(block: Block): string {
  switch (block.type) {
    case "text":
      return "문단";
    case "code":
      return "소스코드";
    case "schedule":
      return "일정";
    case "file":
      return "파일";
    case "image":
      return "사진";
    case "oglink":
      return "링크";
    case "sticker":
      return "스티커";
    case "placesMap":
      return "장소";
  }
}

function BlockBody({
  block,
  blocks,
  onChange,
}: {
  block: Block;
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
}) {
  switch (block.type) {
    case "text":
      return (
        <Stack gap={6}>
          <FormatToolbar block={block} blocks={blocks} onChange={onChange} />
          <Textarea
            aria-label="문단 내용"
            rows={3}
            value={block.text}
            styles={{
              input: {
                fontWeight: block.bold ? 700 : undefined,
                fontStyle: block.italic ? "italic" : undefined,
                textDecoration:
                  [
                    block.underline ? "underline" : "",
                    block.strikeThrough ? "line-through" : "",
                  ]
                    .filter(Boolean)
                    .join(" ") || undefined,
                textAlign: block.align,
              },
            }}
            onChange={(e) =>
              onChange(setText(blocks, block.id, e.currentTarget.value))
            }
          />
        </Stack>
      );
    case "code":
      return (
        <Textarea
          aria-label="소스코드"
          rows={4}
          styles={{ input: { fontFamily: "monospace" } }}
          value={block.code}
          onChange={(e) =>
            onChange(setCode(blocks, block.id, e.currentTarget.value))
          }
        />
      );
    case "schedule":
      return (
        <Text size="sm">
          {block.title} — {block.startAt}
          {block.dateOnly ? " (날짜만)" : ""}
        </Text>
      );
    case "file":
      return (
        <Text size="sm">
          {block.fileName} ({block.fileSize.toLocaleString()} B)
        </Text>
      );
    case "image":
      return (
        <Image
          src={block.src}
          alt={block.fileName}
          h={120}
          w="auto"
          fit="contain"
        />
      );
    case "oglink":
      return (
        <Group gap="sm" wrap="nowrap">
          {block.thumbnailSrc && (
            <Image
              src={block.thumbnailSrc}
              alt={block.title}
              h={56}
              w={56}
              fit="cover"
            />
          )}
          <Box>
            <Text size="sm" fw={600}>
              {block.title}
            </Text>
            <Text size="xs" c="dimmed">
              {block.domain}
            </Text>
          </Box>
        </Group>
      );
    case "sticker":
      return (
        <Text size="sm">
          스티커 {block.packCode} #{block.seq}
        </Text>
      );
    case "placesMap":
      return (
        <Text size="sm">{block.places.map((p) => p.name).join(", ")}</Text>
      );
  }
}

/** 텍스트 블록 하단 서식 툴바(B·I·U·취소선 + 정렬 4). */
function FormatToolbar({
  block,
  blocks,
  onChange,
}: {
  block: TextBlock;
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
}) {
  return (
    <Group gap={4}>
      {MARKS.map((m) => (
        <Tooltip key={m.key} label={m.label}>
          <ActionIcon
            variant={block[m.key] ? "filled" : "subtle"}
            size="sm"
            aria-label={m.label}
            aria-pressed={block[m.key]}
            onClick={() => onChange(toggleMark(blocks, block.id, m.key))}
          >
            <m.icon size={16} />
          </ActionIcon>
        </Tooltip>
      ))}
      <SegmentedControl
        size="xs"
        value={block.align}
        onChange={(v) => onChange(setAlign(blocks, block.id, v as Align))}
        data={ALIGNS.map((a) => ({
          value: a.value,
          label: <a.icon size={14} aria-label={a.label} />,
        }))}
      />
    </Group>
  );
}

/** 링크 삽입 모달 — URL 입력 → oglink 조회. */
function LinkModal({
  opened,
  accountId,
  onClose,
  onInsert,
}: {
  opened: boolean;
  accountId: string | null;
  onClose: () => void;
  onInsert: (
    link: string,
    meta: Awaited<ReturnType<typeof ipc.blog.oglink>>,
  ) => void;
}) {
  const [url, setUrl] = useState("");
  const [loading, setLoading] = useState(false);

  async function submit() {
    if (!accountId || !url.trim()) return;
    setLoading(true);
    try {
      const meta = await ipc.blog.oglink(accountId, url.trim());
      onInsert(url.trim(), meta);
      setUrl("");
    } catch (e) {
      notifications.show({
        color: "red",
        message: `링크 조회 실패: ${String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }

  return (
    <Modal opened={opened} onClose={onClose} title="링크 삽입" centered>
      <Stack gap="sm">
        <TextInput
          label="URL"
          placeholder="https://..."
          value={url}
          onChange={(e) => setUrl(e.currentTarget.value)}
        />
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            취소
          </Button>
          <Button loading={loading} onClick={() => void submit()}>
            삽입
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 일정 삽입 모달(순수 클라이언트) — 제목 + 시작 시각. */
function ScheduleModal({
  opened,
  onClose,
  onInsert,
}: {
  opened: boolean;
  onClose: () => void;
  onInsert: (title: string, startAt: string, dateOnly: boolean) => void;
}) {
  const [title, setTitle] = useState("");
  const [startAt, setStartAt] = useState("");

  function submit() {
    if (!title.trim() || !startAt) return;
    // datetime-local(초 없음) → 초 + KST 오프셋을 붙여 ISO8601로 만든다.
    onInsert(title.trim(), `${startAt}:00+09:00`, false);
    setTitle("");
    setStartAt("");
  }

  return (
    <Modal opened={opened} onClose={onClose} title="일정 삽입" centered>
      <Stack gap="sm">
        <TextInput
          label="제목"
          value={title}
          onChange={(e) => setTitle(e.currentTarget.value)}
        />
        <TextInput
          label="시작 시각"
          type="datetime-local"
          value={startAt}
          onChange={(e) => setStartAt(e.currentTarget.value)}
        />
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            취소
          </Button>
          <Button onClick={submit}>삽입</Button>
        </Group>
      </Stack>
    </Modal>
  );
}

/** 스티커 삽입 모달 — 팩 목록 → seq 선택. */
function StickerModal({
  opened,
  accountId,
  onClose,
  onInsert,
}: {
  opened: boolean;
  accountId: string | null;
  onClose: () => void;
  onInsert: (packCode: string, seq: number) => void;
}) {
  const [packs, setPacks] = useState<StickerPack[]>([]);
  const [seqs, setSeqs] = useState<number[]>([]);
  const [pack, setPack] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function loadPacks() {
    if (!accountId) return;
    setLoading(true);
    try {
      setPacks(await ipc.blog.stickers(accountId));
    } catch (e) {
      notifications.show({
        color: "red",
        message: `스티커 목록 실패: ${String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }

  async function selectPack(code: string) {
    if (!accountId) return;
    setPack(code);
    setLoading(true);
    try {
      setSeqs(await ipc.blog.stickerSeqs(accountId, code));
    } catch (e) {
      notifications.show({
        color: "red",
        message: `스티커 로드 실패: ${String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title="스티커 삽입"
      centered
      onEnterTransitionEnd={() => void loadPacks()}
    >
      <Stack gap="sm">
        {loading && <Loader size="sm" />}
        {!pack && (
          <ScrollArea.Autosize mah={240}>
            <Stack gap={4}>
              {packs.map((p) => (
                <Button
                  key={p.packCode}
                  variant="light"
                  size="xs"
                  onClick={() => void selectPack(p.packCode)}
                >
                  {p.packCode} ({p.stickerCount})
                </Button>
              ))}
              {packs.length === 0 && !loading && (
                <Text size="sm" c="dimmed">
                  스티커 팩이 없습니다.
                </Text>
              )}
            </Stack>
          </ScrollArea.Autosize>
        )}
        {pack && (
          <ScrollArea.Autosize mah={240}>
            <Group gap={6}>
              {seqs.map((seq) => (
                <Button
                  key={seq}
                  variant="default"
                  size="xs"
                  onClick={() => onInsert(pack, seq)}
                >
                  #{seq}
                </Button>
              ))}
            </Group>
          </ScrollArea.Autosize>
        )}
      </Stack>
    </Modal>
  );
}

/** 장소 삽입 모달 — 검색 → 선택 → staticmap 조회. */
function PlaceModal({
  opened,
  accountId,
  onClose,
  onInsert,
}: {
  opened: boolean;
  accountId: string | null;
  onClose: () => void;
  onInsert: (thumbnailSrc: string, place: PlaceResult) => void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<PlaceResult[]>([]);
  const [loading, setLoading] = useState(false);

  async function search() {
    if (!accountId || !query.trim()) return;
    setLoading(true);
    try {
      setResults(await ipc.blog.places(accountId, query.trim()));
    } catch (e) {
      notifications.show({
        color: "red",
        message: `장소 검색 실패: ${String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }

  async function pick(place: PlaceResult) {
    if (!accountId) return;
    setLoading(true);
    try {
      const map = await ipc.blog.staticmap(accountId, place.y, place.x);
      onInsert(map.src, place);
      setQuery("");
      setResults([]);
    } catch (e) {
      notifications.show({
        color: "red",
        message: `지도 조회 실패: ${String(e)}`,
      });
    } finally {
      setLoading(false);
    }
  }

  return (
    <Modal opened={opened} onClose={onClose} title="장소 삽입" centered>
      <Stack gap="sm">
        <Group gap="xs">
          <TextInput
            style={{ flex: 1 }}
            placeholder="장소 검색어"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
          />
          <Button loading={loading} onClick={() => void search()}>
            검색
          </Button>
        </Group>
        <ScrollArea.Autosize mah={260}>
          <Stack gap={4}>
            {results.map((p) => (
              <Button
                key={p.id}
                variant="light"
                size="xs"
                onClick={() => void pick(p)}
              >
                {p.name} — {p.roadAddress || p.address}
              </Button>
            ))}
          </Stack>
        </ScrollArea.Autosize>
      </Stack>
    </Modal>
  );
}
