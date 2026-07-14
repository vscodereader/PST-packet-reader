// 네이버 블로그 편집기 미러 — 제목은 부모가, 본문은 이 컴포넌트가 담당한다. 본문은 "흐르는" 편집
// 영역이다: 글은 그냥 문단(Textarea)에 타이핑하고, 상단 삽입 툴바 6개(사진·스티커·링크·파일·일정·
// 소스코드) 버튼을 누르면 **커서가 있던 문단 바로 아래**에 그 블록이 삽입되고, 그 아래 새 빈 문단이
// 생겨 이어서 쓸 수 있다(네이버 편집기처럼 삽입 위치가 눈에 보인다). 서식(B·I·U·취소선·정렬)은 상단
// 서식 툴바로 현재(포커스된) 문단에 적용한다. 삽입 보조 데이터가 필요한 블록(링크/스티커/사진/파일)은
// 계정 쿠키로 백엔드 보조 API를 호출해 채운다.
//
// ⚠️ 백엔드 계약 유지: onChange로 내보내는 값은 이전과 동일한 **순서대로 나열된 Block[]**이다. 발행 시
// 이 배열이 documentModel components[]로 변환된다(백엔드 document_model.rs). UX만 재설계했다.

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
  IconMoodSmile,
  IconPaperclip,
  IconPhoto,
  IconStrikethrough,
  IconTrash,
  IconUnderline,
} from "@tabler/icons-react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { ipc } from "@/shared/ipc";
import type { OglinkMeta, StickerPack } from "@/shared/ipc";

import {
  createCodeBlock,
  createFileBlock,
  createFileUploadBlock,
  createImageBlock,
  createImageUploadBlock,
  createOglinkBlock,
  createOglinkUrlBlock,
  createScheduleBlock,
  createStickerBlock,
  createTextBlock,
  moveBlock,
  removeBlock,
  setAlign,
  setCode,
  setText,
  stripDataUrlPrefix,
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
  /** 원격(Admin) 모드 — 브라우저라 Tauri IPC·계정 세션이 없다. 미디어를 원본(사진=base64/링크=URL/
   *  스티커=정적)만 담아 raw 블록으로 만들고, 하위 에이전트가 발행 시 대상 계정 세션으로 해결한다. */
  remote?: boolean;
}

type ModalKind = null | "link" | "sticker" | "schedule";

/** 본문 불변식: 최소 1개의 문단(text)이 있고 마지막 블록은 항상 문단이어야 한다(이어쓸 자리).
 *  또한 **연속된 빈 문단은 하나로 접는다** — 삽입/이동으로 빈 칸이 계속 쌓이던 문제를 막는다. */
function normalize(list: Block[]): Block[] {
  const collapsed: Block[] = [];
  for (const b of list) {
    const prev = collapsed[collapsed.length - 1];
    const isEmptyPara = b.type === "text" && b.text.trim() === "";
    const prevEmptyPara =
      prev && prev.type === "text" && prev.text.trim() === "";
    if (isEmptyPara && prevEmptyPara) continue; // 직전도 빈 문단이면 접는다
    collapsed.push(b);
  }
  let next = collapsed.length === 0 ? [createTextBlock()] : collapsed;
  const last = next[next.length - 1];
  if (!last || last.type !== "text") next = [...next, createTextBlock()];
  return next;
}

export function BlockEditor({
  accountId,
  blocks,
  onChange,
  remote = false,
}: BlockEditorProps) {
  const [modal, setModal] = useState<ModalKind>(null);
  const [busy, setBusy] = useState(false);
  // 커서가 있는 문단(text) id. 삽입은 이 문단 바로 아래에, 서식은 이 문단에 적용한다.
  const [focusedId, setFocusedId] = useState<string | null>(null);

  // 본문이 비어 있으면 첫 문단을 시딩한다(항상 타이핑할 자리가 있게). 불변식은 normalize가 지킨다.
  useEffect(() => {
    if (blocks.length === 0) onChange(normalize([]));
  }, [blocks.length, onChange]);

  // 서식/삽입의 기준 문단 = 포커스된 문단, 없으면 마지막 문단.
  const textBlocks = blocks.filter((b): b is TextBlock => b.type === "text");
  const focusedText =
    focusedId != null ? textBlocks.find((b) => b.id === focusedId) : undefined;
  const activeText = focusedText ?? textBlocks[textBlocks.length - 1];
  const activeId = activeText?.id ?? null;

  function requireAccount(): string | null {
    if (!accountId) {
      notifications.show({ color: "red", message: "계정을 먼저 선택하세요." });
      return null;
    }
    return accountId;
  }

  function update(list: Block[]) {
    onChange(normalize(list));
  }

  /** 기준 문단(activeId) 바로 아래에 블록을 삽입한다. 삽입 지점 뒤에 이미 이어쓸 문단이 있으면
   *  **새 빈 문단을 만들지 않고** 그 문단으로 커서를 옮긴다(빈 칸이 매번 늘어나던 문제 해결). 뒤에
   *  이어쓸 문단이 없을 때만 하나 만든다. */
  function insertAtCursor(block: Block) {
    const idx = activeId ? blocks.findIndex((b) => b.id === activeId) : -1;
    const pos = idx >= 0 ? idx + 1 : blocks.length;
    const next = [...blocks];
    const after = next[pos];
    if (after && after.type === "text") {
      next.splice(pos, 0, block);
      update(next);
      setFocusedId(after.id);
    } else {
      const trailing = createTextBlock();
      next.splice(pos, 0, block, trailing);
      update(next);
      setFocusedId(trailing.id);
    }
  }

  async function onInsertPhoto() {
    // 원격(Admin): 계정 세션이 없으니 업로드하지 않고 브라우저에서 base64로 읽어 raw 블록만 만든다.
    // 실제 업로드는 하위가 발행 시 대상 계정으로 한다(agent resolve_blocks_for_account).
    if (remote) {
      const picked = await pickFileBrowser("image/*");
      if (picked)
        insertAtCursor(
          createImageUploadBlock(picked.dataBase64, picked.fileName),
        );
      return;
    }
    const id = requireAccount();
    if (!id) return;
    const path = await pickFile("이미지");
    if (!path) return;
    setBusy(true);
    try {
      const img = await ipc.blog.uploadPhoto(id, path);
      insertAtCursor(createImageBlock(img));
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
    if (remote) {
      const picked = await pickFileBrowser("*/*");
      if (picked)
        insertAtCursor(
          createFileUploadBlock(picked.dataBase64, picked.fileName),
        );
      return;
    }
    const id = requireAccount();
    if (!id) return;
    const path = await pickFile("파일");
    if (!path) return;
    setBusy(true);
    try {
      const f = await ipc.blog.uploadFile(id, path);
      insertAtCursor(createFileBlock(f));
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
      onClick: () => insertAtCursor(createCodeBlock()),
    },
  ];

  return (
    <Stack gap="sm">
      {/* 삽입 툴바 — 커서(현재 문단) 아래에 삽입한다. */}
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

      {/* 서식 툴바 — 현재(포커스된) 문단에 적용. */}
      <FormatToolbar
        block={activeText}
        onToggle={(mark) =>
          activeId && update(toggleMark(blocks, activeId, mark))
        }
        onAlign={(align) =>
          activeId && update(setAlign(blocks, activeId, align))
        }
      />

      {/* 본문 흐름 — 문단과 삽입 블록이 순서대로 보인다. 문단은 바로 타이핑, 삽입물은 그 사이에. */}
      <Stack gap="xs">
        {blocks.map((block) =>
          block.type === "text" ? (
            <Group key={block.id} gap={4} align="flex-start" wrap="nowrap">
              <Textarea
                aria-label="문단 내용"
                placeholder="내용을 입력하세요. 커서를 둔 자리에서 위 툴바로 사진·링크 등을 삽입할 수 있습니다."
                rows={3}
                value={block.text}
                onFocus={() => setFocusedId(block.id)}
                style={{ flex: 1 }}
                styles={{
                  input: {
                    border:
                      block.id === activeId
                        ? "1px solid var(--mantine-color-blue-4)"
                        : undefined,
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
                  update(setText(blocks, block.id, e.currentTarget.value))
                }
              />
              {/* 문단 삭제 — 삽입하다 생긴 빈 문단을 지울 수 있게(마지막 1개는 normalize가 보장). */}
              <Tooltip label="이 문단 삭제">
                <ActionIcon
                  variant="subtle"
                  color="gray"
                  size="sm"
                  mt={4}
                  aria-label="문단 삭제"
                  onClick={() => update(removeBlock(blocks, block.id))}
                >
                  <IconTrash size={15} />
                </ActionIcon>
              </Tooltip>
            </Group>
          ) : (
            <InsertedBlockRow
              key={block.id}
              block={block}
              blocks={blocks}
              onChange={update}
            />
          ),
        )}
      </Stack>

      <LinkModal
        opened={modal === "link"}
        accountId={accountId}
        remote={remote}
        onClose={() => setModal(null)}
        onInsert={(link, meta) => {
          // 원격(meta=null)이면 URL만 담은 raw 블록 — 하위가 발행 시 계정 세션으로 조회한다.
          insertAtCursor(
            meta ? createOglinkBlock(link, meta) : createOglinkUrlBlock(link),
          );
          setModal(null);
        }}
      />
      <ScheduleModal
        opened={modal === "schedule"}
        onClose={() => setModal(null)}
        onInsert={(title, startAt, dateOnly) => {
          insertAtCursor(createScheduleBlock(title, startAt, dateOnly));
          setModal(null);
        }}
      />
      <StickerModal
        opened={modal === "sticker"}
        accountId={accountId}
        remote={remote}
        onClose={() => setModal(null)}
        onInsert={(packCode, seq) => {
          insertAtCursor(createStickerBlock(packCode, seq));
          setModal(null);
        }}
      />
    </Stack>
  );
}

/** 원격(Admin) 정적 스티커 팩 — 브라우저엔 계정 세션이 없어 목록 API를 못 부른다. 무료 기본 팩만
 *  둔다(하위가 packCode+seq 그대로 발행). seq는 보수적으로 1..count. */
const STATIC_STICKER_PACKS: { packCode: string; count: number }[] = [
  { packCode: "cafe_001", count: 10 },
  { packCode: "cafe_002", count: 10 },
  { packCode: "cafe_005", count: 10 },
  { packCode: "motion2d_01", count: 10 },
];

/** 브라우저 파일 선택(Admin) — `<input type=file>` + FileReader로 (파일명, 순수 base64)를 얻는다.
 *  Tauri 다이얼로그/로컬 경로가 없는 브라우저용. 취소/실패 시 null. */
async function pickFileBrowser(
  accept: string,
): Promise<{ fileName: string; dataBase64: string } | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = () => {
      const file = input.files?.[0];
      if (!file) {
        resolve(null);
        return;
      }
      const reader = new FileReader();
      reader.onload = () =>
        resolve({
          fileName: file.name,
          dataBase64: stripDataUrlPrefix(String(reader.result ?? "")),
        });
      reader.onerror = () => resolve(null);
      reader.readAsDataURL(file);
    };
    input.click();
  });
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

/** 삽입된 non-text 블록 1개의 행 — 이동/삭제 컨트롤 + 타입별 미리보기(소스코드는 편집). */
function InsertedBlockRow({
  block,
  blocks,
  onChange,
}: {
  block: Block;
  blocks: Block[];
  onChange: (blocks: Block[]) => void;
}) {
  return (
    <Paper withBorder p="xs" radius="sm" bg="var(--mantine-color-gray-0)">
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
    case "imageUpload":
      return "사진 (업로드 대기)";
    case "fileUpload":
      return "파일 (업로드 대기)";
    case "oglinkUrl":
      return "링크 (조회 대기)";
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
      return null;
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
    case "imageUpload":
    case "fileUpload":
      // 원격(Admin): 아직 업로드 전 — 하위 발행 시 계정 세션으로 업로드된다.
      return (
        <Text size="sm">
          {block.fileName}{" "}
          <Text span c="dimmed" fz="xs">
            (발행 시 대상 계정으로 업로드)
          </Text>
        </Text>
      );
    case "oglinkUrl":
      return (
        <Text size="sm">
          {block.link}{" "}
          <Text span c="dimmed" fz="xs">
            (발행 시 링크 정보 조회)
          </Text>
        </Text>
      );
  }
}

/** 상단 서식 툴바(B·I·U·취소선 + 정렬 4) — 현재 문단에 적용. 문단이 없으면 비활성. */
function FormatToolbar({
  block,
  onToggle,
  onAlign,
}: {
  block: TextBlock | undefined;
  onToggle: (mark: TextMark) => void;
  onAlign: (align: Align) => void;
}) {
  return (
    <Group gap={4}>
      {MARKS.map((m) => (
        <Tooltip key={m.key} label={m.label}>
          <ActionIcon
            variant={block?.[m.key] ? "filled" : "subtle"}
            size="sm"
            aria-label={m.label}
            aria-pressed={block ? block[m.key] : false}
            disabled={!block}
            onClick={() => onToggle(m.key)}
          >
            <m.icon size={16} />
          </ActionIcon>
        </Tooltip>
      ))}
      <SegmentedControl
        size="xs"
        disabled={!block}
        value={block?.align ?? "left"}
        onChange={(v) => onAlign(v as Align)}
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
  remote,
  onClose,
  onInsert,
}: {
  opened: boolean;
  accountId: string | null;
  remote?: boolean;
  onClose: () => void;
  onInsert: (link: string, meta: OglinkMeta | null) => void;
}) {
  const [url, setUrl] = useState("");
  const [loading, setLoading] = useState(false);

  async function submit() {
    if (!url.trim()) return;
    // 원격(Admin): 조회하지 않고 URL만 넘긴다(하위가 발행 시 계정 세션으로 oglink 조회).
    if (remote) {
      onInsert(url.trim(), null);
      setUrl("");
      return;
    }
    if (!accountId) return;
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
  remote,
  onClose,
  onInsert,
}: {
  opened: boolean;
  accountId: string | null;
  remote?: boolean;
  onClose: () => void;
  onInsert: (packCode: string, seq: number) => void;
}) {
  const [packs, setPacks] = useState<StickerPack[]>([]);
  const [seqs, setSeqs] = useState<number[]>([]);
  const [pack, setPack] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function loadPacks() {
    // 원격(Admin): 계정 세션이 없어 목록 API를 못 부르니 정적 무료 팩을 쓴다.
    if (remote) {
      setPacks(
        STATIC_STICKER_PACKS.map((p) => ({
          packCode: p.packCode,
          stickerCount: p.count,
          isFree: true,
        })),
      );
      return;
    }
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
    setPack(code);
    // 원격: seq는 정적 팩의 1..count(계정 세션 없이 self-contained하게 발행).
    if (remote) {
      const found = STATIC_STICKER_PACKS.find((p) => p.packCode === code);
      setSeqs(Array.from({ length: found?.count ?? 0 }, (_, i) => i + 1));
      return;
    }
    if (!accountId) return;
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
