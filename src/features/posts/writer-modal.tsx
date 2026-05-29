import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Group,
  Menu,
  Modal,
  Stack,
  Text,
  Textarea,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useRef, useState } from "react";

import { KIND, MODES, STOCK } from "@/shared/data/mock";
import type {
  CommentTarget,
  LibraryPost,
  ModeValue,
} from "@/shared/data/types";
import { Icon } from "@/shared/ui/icons";

export interface WriterModalProps {
  open: boolean;
  doc: LibraryPost | null;
  drafts: LibraryPost[];
  onClose: () => void;
  onSave: (doc: LibraryPost) => void;
  onSaveDraft: (doc: LibraryPost) => void;
  onDeleteDraft: (doc: LibraryPost) => void;
}

function toast(message: string, color = "blue") {
  notifications.show({ message, color, autoClose: 2400 });
}

function stripHtml(html: string): string {
  const el = document.createElement("div");
  el.innerHTML = html;
  return el.innerText || el.textContent || "";
}

function exec(cmd: string, body: HTMLElement | null) {
  try {
    document.execCommand(cmd);
  } catch {
    /* jsdom / unsupported — no-op */
  }
  body?.focus();
}

// URL → plain text (mock crawl). Stock board links become a price line.
function crawlToText(u: string): string {
  const m = u.match(/code=(\d{6})/) ?? u.match(/(\d{6})/);
  const s = m?.[1] ? STOCK[m[1]] : undefined;
  if (s) {
    const arrow = s.chg > 0 ? "▲" : s.chg < 0 ? "▼" : "·";
    return `${s.name}(${s.code}) · ${s.market} 현재가 ${s.price} (${arrow}${Math.abs(s.chg)}%)`;
  }
  let host = u;
  try {
    host = new URL(u.startsWith("http") ? u : "https://" + u).hostname.replace(
      /^www\./,
      "",
    );
  } catch {
    /* ignore */
  }
  return `[${host}에서 가져온 내용]`;
}

const COUNT_OPTS = [1, 3, 5, 10];
const VAR_ITEMS = [
  { tok: "#{종목명}", label: "종목명", desc: "대상 종목·카페·밴드 이름" },
  { tok: "#{종목코드}", label: "종목코드", desc: "대상 종목의 6자리 코드" },
  { tok: "#{링크}", label: "링크", desc: "대상 종목 시세 링크" },
];

function CommentComposer({
  comments,
  setComments,
  onOwnPost,
  target,
  setTarget,
  url,
  setUrl,
  count,
  setCount,
}: {
  comments: string[];
  setComments: (c: string[]) => void;
  onOwnPost: boolean;
  target: CommentTarget;
  setTarget: (v: CommentTarget) => void;
  url: string;
  setUrl: (v: string) => void;
  count: number;
  setCount: (n: number) => void;
}) {
  const filled = comments.filter((c) => c.trim()).length;
  const targetOpts: { v: CommentTarget; t: string; ic: React.ReactNode }[] = [
    { v: "latest", t: "최신글", ic: <Icon.clock size={15} /> },
    { v: "popular", t: "인기글", ic: <Icon.trendingUp size={15} /> },
    { v: "url", t: "특정 게시글", ic: <Icon.link size={15} /> },
  ];
  return (
    <Box>
      {onOwnPost ? (
        <Group
          gap={9}
          p="sm"
          mb={16}
          style={{
            background: "var(--mantine-color-forum-light)",
            borderRadius: "var(--mantine-radius-md)",
          }}
        >
          <Icon.target size={16} color="var(--mantine-color-forum-filled)" />
          <Text fz={12.5} fw={600} c="gray.7">
            위에서 작성한 글에 바로 댓글이 달립니다.
          </Text>
        </Group>
      ) : (
        <>
          <Group gap={7} mb={10}>
            <Icon.target size={16} color="var(--mantine-color-gray-6)" />
            <Text fz={13.5} fw={700}>
              댓글 대상
            </Text>
          </Group>
          <Group gap={8} mb={10} grow>
            {targetOpts.map((o) => (
              <Button
                key={o.v}
                variant={target === o.v ? "light" : "default"}
                color={target === o.v ? "blue" : "gray"}
                leftSection={o.ic}
                onClick={() => setTarget(o.v)}
              >
                {o.t}
              </Button>
            ))}
          </Group>
          {target === "url" ? (
            <TextInput
              mb={18}
              value={url}
              onChange={(e) => setUrl(e.currentTarget.value)}
              placeholder="https://finance.naver.com/item/board_read…"
              styles={{ input: { fontFamily: "monospace" } }}
            />
          ) : (
            <Group
              gap={10}
              p="sm"
              mb={18}
              style={{
                border: "1px solid var(--mantine-color-gray-2)",
                borderRadius: "var(--mantine-radius-md)",
                background: "var(--mantine-color-gray-0)",
              }}
            >
              <Text fz={12.5} fw={700} c="gray.7">
                대상마다 {target === "popular" ? "인기글" : "최신글"}
              </Text>
              <Group gap={4} ml="auto">
                {COUNT_OPTS.map((n) => (
                  <Button
                    key={n}
                    size="compact-sm"
                    variant={count === n ? "filled" : "default"}
                    color={count === n ? "blue" : "gray"}
                    onClick={() => setCount(n)}
                    styles={{ label: { fontFamily: "monospace" } }}
                  >
                    {n}
                  </Button>
                ))}
              </Group>
              <Text fz={12.5} fw={700} c="gray.7">
                개에 댓글
              </Text>
            </Group>
          )}
        </>
      )}

      <Group justify="space-between" mb={10}>
        <Group gap={7}>
          <Icon.comment size={16} color="var(--mantine-color-gray-6)" />
          <Text fz={13.5} fw={700}>
            댓글 내용
          </Text>
          <Text fz={12} c="dimmed">
            {filled}종
          </Text>
        </Group>
        <Button
          size="compact-xs"
          radius="xl"
          variant="light"
          color="forum"
          leftSection={<Icon.sparkles size={14} />}
          onClick={() =>
            toast("AI가 자연스러운 댓글 변형을 만들었어요 ✨", "green")
          }
        >
          변형 생성
        </Button>
      </Group>
      <Box
        p="sm"
        style={{
          background: "var(--mantine-color-gray-0)",
          border: "1px solid var(--mantine-color-gray-2)",
          borderRadius: "var(--mantine-radius-md)",
        }}
      >
        <Stack gap={8}>
          {comments.map((c, i) => (
            <Group key={i} gap={8} align="flex-start" wrap="nowrap">
              <Text fz={12} fw={700} c="dimmed" w={22} ta="center" mt={9}>
                {i + 1}
              </Text>
              <Textarea
                minRows={2}
                style={{ flex: 1 }}
                value={c}
                placeholder="자연스러운 댓글을 입력하세요"
                onChange={(e) =>
                  setComments(
                    comments.map((x, idx) =>
                      idx === i ? e.currentTarget.value : x,
                    ),
                  )
                }
              />
              <ActionIcon
                variant="subtle"
                color="gray"
                mt={6}
                title="삭제"
                onClick={() =>
                  setComments(
                    comments.length === 1
                      ? [""]
                      : comments.filter((_, idx) => idx !== i),
                  )
                }
              >
                <Icon.x size={16} />
              </ActionIcon>
            </Group>
          ))}
        </Stack>
        <Button
          variant="subtle"
          size="compact-sm"
          ml={30}
          mt={8}
          leftSection={<Icon.plus size={15} />}
          onClick={() => setComments([...comments, ""])}
        >
          댓글 추가
        </Button>
      </Box>
      <Group gap={7} mt={12}>
        <Icon.refresh size={14} color="var(--mantine-color-gray-5)" />
        <Text fz={12} c="dimmed">
          여러 댓글을 등록하면 계정마다 다른 댓글이 무작위로 게시돼 더
          자연스러워요.
        </Text>
      </Group>
    </Box>
  );
}

function WriterModalInner({
  open,
  doc,
  drafts,
  onClose,
  onSave,
  onSaveDraft,
  onDeleteDraft,
}: WriterModalProps) {
  const initialBody =
    doc?.body ?? (doc?.excerpt ? `<p>${doc.excerpt}</p>` : "");
  const [mode, setMode] = useState<ModeValue>(doc?.kind ?? "post");
  const [title, setTitle] = useState(doc?.title ?? "");
  const [comments, setComments] = useState<string[]>(
    doc?.comments?.length ? [...doc.comments] : ["", ""],
  );
  const [cTarget, setCTarget] = useState<CommentTarget>(
    doc?.commentTarget ?? "latest",
  );
  const [cUrl, setCUrl] = useState(doc?.commentUrl ?? "");
  const [cCount, setCCount] = useState(doc?.commentCount ?? 3);
  const [dirty, setDirty] = useState(false);
  const [confirmClose, setConfirmClose] = useState(false);
  const [wordCount, setWordCount] = useState(
    stripHtml(initialBody).replace(/\s/g, "").length,
  );
  const [newId] = useState(() => "p" + Date.now());

  const bodyRef = useRef<HTMLDivElement | null>(null);
  const titleRef = useRef<HTMLInputElement | null>(null);
  const imgInput = useRef<HTMLInputElement | null>(null);
  const lastFocus = useRef<"title" | "body">("body");
  const seeded = useRef(false);

  const setBodyRef = (el: HTMLDivElement | null) => {
    bodyRef.current = el;
    if (el && !seeded.current) {
      el.innerHTML = initialBody;
      seeded.current = true;
    }
  };

  const showEditor = mode === "post" || mode === "both";
  const showComments = mode === "comment" || mode === "both";
  const filledComments = comments.filter((c) => c.trim());

  const onBodyInput = () => {
    setWordCount((bodyRef.current?.innerText ?? "").replace(/\s/g, "").length);
    setDirty(true);
  };
  const onPaste = (e: React.ClipboardEvent) => {
    e.preventDefault();
    const raw = e.clipboardData.getData("text/plain") || "";
    if (!raw) return;
    const hadUrl = /(https?:\/\/\S+|www\.\S+)/i.test(raw);
    const converted = raw.replace(/(https?:\/\/\S+|www\.\S+)/gi, (u) =>
      crawlToText(u),
    );
    try {
      document.execCommand("insertText", false, converted);
    } catch {
      /* no-op */
    }
    onBodyInput();
    if (hadUrl) toast("링크를 읽어 본문 텍스트로 변환했어요", "green");
  };
  const insertImage = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.currentTarget.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      bodyRef.current?.focus();
      try {
        document.execCommand("insertImage", false, String(reader.result));
      } catch {
        /* no-op */
      }
      onBodyInput();
    };
    reader.readAsDataURL(file);
    e.currentTarget.value = "";
  };
  const insertToken = (tok: string) => {
    if (lastFocus.current === "title" && titleRef.current) {
      const el = titleRef.current;
      const s = el.selectionStart ?? title.length;
      const en = el.selectionEnd ?? title.length;
      setTitle(title.slice(0, s) + tok + title.slice(en));
      setDirty(true);
    } else {
      bodyRef.current?.focus();
      try {
        document.execCommand("insertText", false, tok);
      } catch {
        /* no-op */
      }
      onBodyInput();
    }
  };

  const buildDoc = (status: LibraryPost["status"]): LibraryPost => {
    const bodyHtml = bodyRef.current?.innerHTML ?? initialBody;
    const bodyText = (bodyRef.current?.innerText ?? "").trim();
    const excerpt = bodyText
      ? bodyText.slice(0, 70)
      : (filledComments[0] ?? "");
    const words =
      mode === "comment"
        ? filledComments.join("").replace(/\s/g, "").length
        : bodyText.replace(/\s/g, "").length;
    return {
      id: doc?.id ?? newId,
      title: title.trim() || "제목 없음",
      kind: mode,
      body: bodyHtml,
      comments: filledComments,
      commentTarget: cTarget,
      commentUrl: cUrl,
      commentCount: cCount,
      status,
      words,
      updated: "방금 전",
      excerpt,
    };
  };

  const hasContent =
    !!title.trim() || wordCount > 0 || filledComments.length > 0;
  const handleX = () => {
    if (dirty && hasContent) setConfirmClose(true);
    else onClose();
  };
  const loadDraft = (d: LibraryPost) => {
    setMode(d.kind);
    setTitle(d.title);
    setComments(d.comments?.length ? [...d.comments] : ["", ""]);
    seeded.current = false;
    if (bodyRef.current) {
      bodyRef.current.innerHTML =
        d.body ?? (d.excerpt ? `<p>${d.excerpt}</p>` : "");
      seeded.current = true;
    }
    setDirty(false);
    toast(`‘${(d.title || "제목 없음").slice(0, 12)}…’ 불러왔어요`, "green");
  };

  const modeMeta = MODES.find((m) => m.v === mode);

  return (
    <Modal
      opened={open}
      onClose={handleX}
      withCloseButton={false}
      padding={0}
      size="xl"
      radius="lg"
      styles={{ body: { display: "flex", flexDirection: "column" } }}
    >
      {/* header */}
      <Group
        h={60}
        px={16}
        gap={12}
        style={{
          flexShrink: 0,
          borderBottom: "1px solid var(--mantine-color-gray-2)",
        }}
      >
        <Group gap={6}>
          {MODES.map((m) => {
            const MI = Icon[m.icon as keyof typeof Icon];
            return (
              <Button
                key={m.v}
                size="xs"
                radius="xl"
                variant={m.v === mode ? "filled" : "default"}
                color={m.v === mode ? "dark" : "gray"}
                leftSection={<MI size={15} />}
                title={m.s}
                onClick={() => {
                  setMode(m.v);
                  setDirty(true);
                }}
              >
                {m.t}
              </Button>
            );
          })}
        </Group>
        <Box style={{ flex: 1 }} />
        <Menu position="bottom-end" width={280} closeOnItemClick={false}>
          <Menu.Target>
            <Button
              variant="default"
              rightSection={<Icon.chevronDown size={15} />}
            >
              임시저장
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item
              leftSection={<Icon.save size={16} />}
              onClick={() => {
                onSaveDraft(buildDoc("draft"));
                setDirty(false);
                toast("임시저장 했어요", "green");
              }}
            >
              임시저장하기
            </Menu.Item>
            <Menu.Label>임시저장 목록 ({drafts.length})</Menu.Label>
            {drafts.length === 0 && (
              <Text fz={12.5} c="dimmed" px="sm" py={6}>
                저장된 초안이 없어요
              </Text>
            )}
            {drafts.map((d) => {
              const kd = KIND[d.kind] ?? KIND.post!;
              return (
                <Group key={d.id} gap={9} px="sm" py={6} wrap="nowrap">
                  <Badge size="sm" color={kd.c} variant="light">
                    {kd.t}
                  </Badge>
                  <Box style={{ flex: 1, minWidth: 0 }}>
                    <Text fz={13} fw={600} truncate>
                      {d.title || "제목 없음"}
                    </Text>
                    <Text fz={11} c="dimmed">
                      {d.updated}
                    </Text>
                  </Box>
                  <Button
                    size="compact-xs"
                    variant="light"
                    radius="xl"
                    onClick={() => loadDraft(d)}
                  >
                    불러오기
                  </Button>
                  <ActionIcon
                    size="sm"
                    variant="subtle"
                    color="gray"
                    title="삭제"
                    onClick={() => onDeleteDraft(d)}
                  >
                    <Icon.trash size={15} />
                  </ActionIcon>
                </Group>
              );
            })}
          </Menu.Dropdown>
        </Menu>
        <Button
          leftSection={<Icon.check size={16} />}
          onClick={() => onSave(buildDoc("ready"))}
        >
          저장
        </Button>
        <ActionIcon
          size="lg"
          variant="subtle"
          color="gray"
          title="닫기"
          onClick={handleX}
        >
          <Icon.x size={20} />
        </ActionIcon>
      </Group>

      {/* body */}
      <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }} py={26}>
        <Box maw={680} mx="auto" px={40}>
          <Group gap={7} mb={16}>
            <Icon.pencil size={14} color="var(--mantine-color-gray-5)" />
            <Text fz={12.5} c="dimmed">
              {modeMeta?.s}
            </Text>
          </Group>

          {showEditor && (
            <Box mb={showComments ? 26 : 0}>
              <TextInput
                ref={titleRef}
                variant="unstyled"
                size="xl"
                placeholder="제목을 입력하세요"
                value={title}
                onFocus={() => (lastFocus.current = "title")}
                onChange={(e) => {
                  setTitle(e.currentTarget.value);
                  setDirty(true);
                }}
                styles={{ input: { fontWeight: 800, fontSize: 26 } }}
              />
              <Box
                my={14}
                style={{ height: 1, background: "var(--mantine-color-gray-2)" }}
              />
              <Group gap={2} mb={12}>
                <ActionIcon
                  variant="subtle"
                  color="gray"
                  title="굵게"
                  onClick={() => {
                    exec("bold", bodyRef.current);
                    setDirty(true);
                  }}
                >
                  <Icon.bold size={17} />
                </ActionIcon>
                <ActionIcon
                  variant="subtle"
                  color="gray"
                  title="기울임"
                  onClick={() => {
                    exec("italic", bodyRef.current);
                    setDirty(true);
                  }}
                >
                  <Icon.italic size={17} />
                </ActionIcon>
                <ActionIcon
                  variant="subtle"
                  color="gray"
                  title="밑줄"
                  onClick={() => {
                    exec("underline", bodyRef.current);
                    setDirty(true);
                  }}
                >
                  <Icon.underline size={17} />
                </ActionIcon>
                <Box
                  mx={6}
                  style={{
                    width: 1,
                    height: 18,
                    background: "var(--mantine-color-gray-3)",
                  }}
                />
                <ActionIcon
                  variant="subtle"
                  color="gray"
                  title="이미지 추가"
                  onClick={() => imgInput.current?.click()}
                >
                  <Icon.image size={17} />
                </ActionIcon>
                <input
                  ref={imgInput}
                  type="file"
                  accept="image/*"
                  onChange={insertImage}
                  style={{ display: "none" }}
                />
                <Menu position="bottom-start" width={244}>
                  <Menu.Target>
                    <Button
                      size="compact-sm"
                      variant="default"
                      leftSection={<Icon.hash size={15} />}
                    >
                      변수
                    </Button>
                  </Menu.Target>
                  <Menu.Dropdown>
                    {VAR_ITEMS.map((it) => (
                      <Menu.Item
                        key={it.tok}
                        onClick={() => insertToken(it.tok)}
                      >
                        <Group gap={10} wrap="nowrap">
                          <Badge
                            size="sm"
                            variant="light"
                            color="blue"
                            styles={{ label: { fontFamily: "monospace" } }}
                          >
                            {it.tok}
                          </Badge>
                          <Box>
                            <Text fz={12.5} fw={700}>
                              {it.label}
                            </Text>
                            <Text fz={11} c="dimmed">
                              {it.desc}
                            </Text>
                          </Box>
                        </Group>
                      </Menu.Item>
                    ))}
                    <Text fz={11} c="dimmed" px="sm" pt={6}>
                      게시할 때 대상마다 자동으로 채워집니다.
                    </Text>
                  </Menu.Dropdown>
                </Menu>
                <Group gap={5} ml="auto">
                  <Icon.link size={13} color="var(--mantine-color-gray-5)" />
                  <Text fz={11.5} c="dimmed">
                    링크를 붙여넣으면 자동으로 텍스트로 바뀝니다
                  </Text>
                </Group>
              </Group>
              <Box
                ref={setBodyRef}
                contentEditable
                suppressContentEditableWarning
                onInput={onBodyInput}
                onPaste={onPaste}
                onDrop={(e) => {
                  e.preventDefault();
                  toast("본문에는 텍스트·이미지만 넣을 수 있어요");
                }}
                onFocus={() => (lastFocus.current = "body")}
                data-placeholder="여기에 내용을 작성하세요. 링크를 붙여넣으면 자동으로 읽어 텍스트로 치환돼요."
                style={{
                  minHeight: showComments ? 180 : 300,
                  outline: "none",
                  fontSize: 16,
                  lineHeight: 1.75,
                  color: "var(--mantine-color-gray-7)",
                }}
              />
            </Box>
          )}

          {showComments && (
            <>
              {mode === "both" && (
                <Box
                  mb={22}
                  style={{
                    height: 1,
                    background: "var(--mantine-color-gray-2)",
                  }}
                />
              )}
              <CommentComposer
                comments={comments}
                setComments={(u) => {
                  setComments(u);
                  setDirty(true);
                }}
                onOwnPost={mode === "both"}
                target={cTarget}
                setTarget={(v) => {
                  setCTarget(v);
                  setDirty(true);
                }}
                url={cUrl}
                setUrl={(v) => {
                  setCUrl(v);
                  setDirty(true);
                }}
                count={cCount}
                setCount={(v) => {
                  setCCount(v);
                  setDirty(true);
                }}
              />
            </>
          )}
        </Box>
      </Box>

      {/* footer */}
      <Group
        h={44}
        px={20}
        gap={14}
        style={{
          flexShrink: 0,
          borderTop: "1px solid var(--mantine-color-gray-2)",
          background: "var(--mantine-color-gray-0)",
        }}
      >
        {showEditor && (
          <Text fz={12} c="dimmed">
            {wordCount}자
          </Text>
        )}
        {showComments && (
          <Text fz={12} c="dimmed">
            댓글 {filledComments.length}종
          </Text>
        )}
        <Group gap={5}>
          <Box
            w={6}
            h={6}
            style={{
              borderRadius: 999,
              background: dirty
                ? "var(--mantine-color-yellow-6)"
                : "var(--mantine-color-green-6)",
            }}
          />
          <Text fz={12} c="dimmed">
            {dirty ? "저장되지 않은 변경" : "저장됨"}
          </Text>
        </Group>
        <Text fz={12} c="dimmed" ml="auto">
          저장하면 글 목록에 추가돼요. 게시는 목록에서 진행합니다.
        </Text>
      </Group>

      <Modal
        opened={confirmClose}
        onClose={() => setConfirmClose(false)}
        withCloseButton={false}
        radius="lg"
        size={380}
        centered
      >
        <ThemeIcon size={48} radius="xl" variant="light" color="yellow" mb={14}>
          <Icon.save size={24} />
        </ThemeIcon>
        <Text fz={18} fw={800} mb={6}>
          작성 중인 글을 임시저장할까요?
        </Text>
        <Text fz={13.5} c="dimmed" mb={20}>
          저장하지 않으면 변경 내용이 사라집니다.
        </Text>
        <Group gap={9} grow>
          <Button
            variant="default"
            onClick={() => {
              setConfirmClose(false);
              onClose();
            }}
          >
            저장 안 함
          </Button>
          <Button
            leftSection={<Icon.save size={16} />}
            onClick={() => {
              onSaveDraft(buildDoc("draft"));
              setConfirmClose(false);
              onClose();
              toast("임시저장 후 닫았어요", "green");
            }}
          >
            임시저장
          </Button>
        </Group>
      </Modal>
    </Modal>
  );
}

export function WriterModal(props: WriterModalProps) {
  // Remount the form per open / per document so its useState initializers
  // re-seed from the doc (no setState-in-effect).
  return (
    <WriterModalInner
      key={props.open ? (props.doc?.id ?? "new") : "closed"}
      {...props}
    />
  );
}
