import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Divider,
  Group,
  Menu,
  Modal,
  NumberInput,
  Stack,
  Text,
  Textarea,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useRef, useState } from "react";

import { KIND, MODES } from "@/shared/data/config";
import type {
  CommentTarget,
  LibraryPost,
  ModeValue,
  Stock,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

import {
  clampCommentCount,
  crawlToText,
  MAX_COMMENT_COUNT,
  MIN_COMMENT_COUNT,
} from "./publish-helpers";

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
  urls,
  setUrls,
  count,
  setCount,
}: {
  comments: string[];
  setComments: (c: string[]) => void;
  onOwnPost: boolean;
  target: CommentTarget;
  setTarget: (v: CommentTarget) => void;
  urls: string[];
  setUrls: (v: string[]) => void;
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
                size="sm"
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
            // 특정 게시글: 여러 링크를 넣으면 각 링크의 글마다 댓글이 달린다(글+댓글의 '댓글
            // 추가'와 동일 UX). 기본 3칸을 깔고 '링크 추가'로 더 넣거나 x로 지운다.
            <Box mb={18}>
              <Stack gap={8}>
                {urls.map((u, i) => (
                  <Group key={i} gap={8} align="center" wrap="nowrap">
                    <Text fz={12} fw={700} c="dimmed" w={22} ta="center">
                      {i + 1}
                    </Text>
                    <TextInput
                      style={{ flex: 1 }}
                      value={u}
                      onChange={(e) =>
                        setUrls(
                          urls.map((x, idx) =>
                            idx === i ? e.currentTarget.value : x,
                          ),
                        )
                      }
                      placeholder="종목토론방·카페·밴드 글 URL (예: https://stock.naver.com/domestic/stock/005930/discussion/…)"
                      styles={{ input: { fontFamily: "monospace" } }}
                    />
                    <ActionIcon
                      variant="subtle"
                      color="gray"
                      title="삭제"
                      onClick={() =>
                        setUrls(
                          urls.length === 1
                            ? [""]
                            : urls.filter((_, idx) => idx !== i),
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
                onClick={() => setUrls([...urls, ""])}
              >
                링크 추가
              </Button>
            </Box>
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
              <Group gap={4} ml="auto" wrap="nowrap">
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
                <Divider orientation="vertical" />
                {/* 프리셋 외 임의 개수 직접 입력(1~50). 프리셋과 같은 setCount로 연결돼
                    선택값이 곧 입력칸에 반영된다. */}
                <NumberInput
                  aria-label="댓글 대상 글 개수 직접 입력"
                  value={count}
                  onChange={(v) => {
                    const n = typeof v === "number" ? v : parseInt(v, 10);
                    if (!Number.isNaN(n)) setCount(clampCommentCount(n));
                  }}
                  min={MIN_COMMENT_COUNT}
                  max={MAX_COMMENT_COUNT}
                  clampBehavior="strict"
                  size="xs"
                  w={68}
                  styles={{
                    input: { fontFamily: "monospace", textAlign: "center" },
                  }}
                />
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
  // 특정 게시글 댓글 대상 링크들. 기존 단일 commentUrl도 받아 길이 1로 펴고(하위호환),
  // 새 문서는 기본 3칸을 깐다(요구: 링크 입력칸 3개 + 링크 추가 버튼).
  const [cUrls, setCUrls] = useState<string[]>(
    doc?.commentUrls?.length
      ? [...doc.commentUrls]
      : doc?.commentUrl
        ? [doc.commentUrl, "", ""]
        : ["", "", ""],
  );
  const [cCount, setCCount] = useState(doc?.commentCount ?? 3);
  const [dirty, setDirty] = useState(false);
  const [confirmClose, setConfirmClose] = useState(false);
  const [wordCount, setWordCount] = useState(
    stripHtml(initialBody).replace(/\s/g, "").length,
  );
  const [newId] = useState(() => "p" + Date.now());
  const [stocks, setStocks] = useState<Stock[]>([]);

  useEffect(() => {
    void ipc.stocks.list().then(setStocks);
  }, []);

  const bodyRef = useRef<HTMLDivElement | null>(null);
  const titleRef = useRef<HTMLInputElement | null>(null);
  const imgInput = useRef<HTMLInputElement | null>(null);
  const lastFocus = useRef<"title" | "body">("body");
  // 변수(토큰) 메뉴를 누르면 본문(contentEditable)이 포커스를 잃어 커서 위치가 사라진다.
  // 마지막 본문 커서/선택을 저장해 두고 insertToken에서 복원해, 토큰/링크가 본문 끝에 붙지
  // 않고 사용자가 둔 자리(엔터로 만든 새 줄 포함)에 들어가게 한다(#267-1).
  const savedRange = useRef<Range | null>(null);
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
  // 본문의 현재 커서/선택 위치를 저장한다(#267-1). 본문 안의 선택일 때만 저장해, 다른 곳을
  // 클릭한 선택으로 덮어쓰지 않는다. 키 입력·클릭·blur 때마다 갱신해 항상 최신 커서를 들고 있는다.
  const saveBodySelection = () => {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0) return;
    const range = sel.getRangeAt(0);
    const el = bodyRef.current;
    if (el && el.contains(range.commonAncestorContainer)) {
      savedRange.current = range.cloneRange();
    }
  };
  const onPaste = (e: React.ClipboardEvent) => {
    e.preventDefault();
    const raw = e.clipboardData.getData("text/plain") || "";
    if (!raw) return;
    const hadUrl = /(https?:\/\/\S+|www\.\S+)/i.test(raw);
    const converted = raw.replace(/(https?:\/\/\S+|www\.\S+)/gi, (u) =>
      crawlToText(u, stocks),
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
      const el = bodyRef.current;
      if (!el) return;
      el.focus();
      // 변수 메뉴를 여는 동안 잃은 본문 커서 위치를 복원한다(#267-1: 줄바꿈 뒤 토큰/링크가 본문
      // 끝에 공백 없이 붙던 버그). 저장된 위치가 없으면(상호작용 전) 기존대로 현재 커서에 넣는다.
      const sel = window.getSelection();
      const r = savedRange.current;
      if (sel && r && el.contains(r.commonAncestorContainer)) {
        sel.removeAllRanges();
        sel.addRange(r);
      }
      try {
        document.execCommand("insertText", false, tok);
      } catch {
        /* no-op */
      }
      // 삽입 후 커서 위치도 갱신해, 연속으로 토큰을 넣어도 올바른 자리에 이어 들어가게 한다.
      saveBodySelection();
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
    // 빈 링크는 버리고(공백 제거), 단일 commentUrl은 하위호환으로 첫 링크를 채운다.
    const filledUrls = cUrls.map((u) => u.trim()).filter(Boolean);
    return {
      id: doc?.id ?? newId,
      title: title.trim() || "제목 없음",
      kind: mode,
      body: bodyHtml,
      comments: filledComments,
      commentTarget: cTarget,
      commentUrl: filledUrls[0] ?? "",
      commentUrls: filledUrls,
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
    // 특정 게시글 대상·링크들도 복원해 다중 링크 초안이 그대로 다시 열리게 한다.
    if (d.commentTarget) setCTarget(d.commentTarget);
    setCUrls(
      d.commentUrls?.length
        ? [...d.commentUrls]
        : d.commentUrl
          ? [d.commentUrl, "", ""]
          : ["", "", ""],
    );
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
              size="sm"
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
          size="sm"
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
                onKeyUp={saveBodySelection}
                onMouseUp={saveBodySelection}
                onBlur={saveBodySelection}
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
                urls={cUrls}
                setUrls={(v) => {
                  setCUrls(v);
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
            size="sm"
            variant="default"
            onClick={() => {
              setConfirmClose(false);
              onClose();
            }}
          >
            저장 안 함
          </Button>
          <Button
            size="sm"
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
