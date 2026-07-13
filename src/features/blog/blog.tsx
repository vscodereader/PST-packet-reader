// 네이버 블로그 새 글 발행 화면(#블로그). 저장된 로그인 쿠키로 순수 HTTP(RabbitWrite)로 글을
// 올린다(종토처럼 크롬 안 뜸). 계정 선택 → 블로그명(기존/새) → 제목·내용 → 발행설정(공개범위·
// 발행옵션·태그·발행시간·공지) → 발행. 결과 링크(logNo)나 실패 사유(원문 로그는 pstmacro.log)를
// 보여준다. Admin 연결은 이후 단계(먼저 pstmacro 단독 동작).

import {
  Alert,
  Anchor,
  Button,
  Divider,
  Group,
  NumberInput,
  Paper,
  Radio,
  SegmentedControl,
  Select,
  Stack,
  Switch,
  Text,
  TextInput,
  Textarea,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useMemo, useState } from "react";

import type { Account } from "@/shared/bindings/Account";
import type { ViewId } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";

/** 공개설정 UI 순서(전체공개>이웃>서로이웃>비공개) = openType 0/1/2/3. */
const OPEN_TYPES = [
  { value: "0", label: "전체공개" },
  { value: "1", label: "이웃공개" },
  { value: "2", label: "서로이웃공개" },
  { value: "3", label: "비공개" },
];

export function Blog(_props: { go?: (view: ViewId) => void }) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [isNewBlog, setIsNewBlog] = useState(false);
  const [blogId, setBlogId] = useState("");
  const [nameChecking, setNameChecking] = useState(false);
  const [nameResult, setNameResult] = useState<null | boolean>(null);

  const [title, setTitle] = useState("");
  const [content, setContent] = useState("");

  const [openType, setOpenType] = useState("0");
  const [commentYn, setCommentYn] = useState(true);
  const [searchYn, setSearchYn] = useState(true);
  const [sympathyYn, setSympathyYn] = useState(true);
  const [noticePostYn, setNoticePostYn] = useState(false);
  const [tags, setTags] = useState("");

  const [timeType, setTimeType] = useState<"now" | "reserve">("now");
  const now = new Date();
  const [rYear, setRYear] = useState<number>(now.getFullYear());
  const [rMonth, setRMonth] = useState<number>(now.getMonth() + 1);
  const [rDate, setRDate] = useState<number>(now.getDate());
  const [rHour, setRHour] = useState<number>(now.getHours());
  const [rMinute, setRMinute] = useState<number>(now.getMinutes());

  const [publishing, setPublishing] = useState(false);
  const [result, setResult] = useState<null | { ok: boolean; msg: string; url?: string }>(null);

  useEffect(() => {
    // 밴드는 별도 쿠키(cookies-band)라 제외 — 블로그는 네이버 저장 쿠키(cookies/{id})를 쓴다.
    ipc.accounts
      .list()
      .then((list) => setAccounts(list.filter((a) => a.platform !== "band")))
      .catch(() => setAccounts([]));
  }, []);

  const accountOptions = useMemo(
    () => accounts.map((a) => ({ value: a.loginId, label: `${a.loginId} (${a.platform})` })),
    [accounts],
  );

  // 계정 선택 시 기존 블로그명 기본값 = 로그인 아이디(기본 블로그 주소). 형님이 바꿀 수 있다.
  function onSelectAccount(v: string | null) {
    setAccountId(v);
    setNameResult(null);
    if (v && !isNewBlog) setBlogId(v);
  }

  async function onCheckName() {
    if (!accountId || !blogId.trim()) return;
    setNameChecking(true);
    setNameResult(null);
    try {
      const ok = await ipc.blog.checkName(accountId, blogId.trim());
      setNameResult(ok);
    } catch (e) {
      notifications.show({ color: "red", message: `블로그명 확인 실패: ${String(e)}` });
    } finally {
      setNameChecking(false);
    }
  }

  async function onPublish() {
    if (!accountId) {
      notifications.show({ color: "red", message: "계정을 선택하세요." });
      return;
    }
    if (!blogId.trim() || !title.trim()) {
      notifications.show({ color: "red", message: "블로그명과 제목을 입력하세요." });
      return;
    }
    setPublishing(true);
    setResult(null);
    try {
      const r = await ipc.blog.publish({
        accountId,
        blogId: blogId.trim(),
        title,
        content,
        openType: Number(openType),
        commentYn,
        searchYn,
        sympathyYn,
        noticePostYn,
        tags: tags.trim(),
        reserve:
          timeType === "reserve"
            ? { year: rYear, month: rMonth, date: rDate, hour: rHour, minute: rMinute }
            : undefined,
      });
      const url = r.logNo
        ? `https://blog.naver.com/${blogId.trim()}/${r.logNo}`
        : r.redirectUrl;
      setResult({ ok: true, msg: r.logNo ? `발행 성공 (logNo ${r.logNo})` : "예약 발행 등록됨", url });
      notifications.show({ color: "teal", message: "블로그 글 발행 성공" });
    } catch (e) {
      setResult({ ok: false, msg: String(e) });
      notifications.show({ color: "red", message: `발행 실패: ${String(e)}` });
    } finally {
      setPublishing(false);
    }
  }

  return (
    <Stack p="md" gap="md" maw={720}>
      <Title order={3}>네이버 블로그</Title>
      <Text size="sm" c="dimmed">
        선택 로그인으로 저장된 쿠키로 새 글을 발행합니다(재로그인 없음). 실패 시 원문 로그는
        pstmacro.log 의 [BLOG] 항목에서 확인하세요.
      </Text>

      <Paper withBorder p="md" radius="md">
        <Stack gap="sm">
          <Select
            label="계정"
            placeholder="발행할 계정 선택"
            data={accountOptions}
            value={accountId}
            onChange={onSelectAccount}
            searchable
          />

          <Switch
            label="새 블로그 만들기(블로그명 신규)"
            checked={isNewBlog}
            onChange={(e) => {
              setIsNewBlog(e.currentTarget.checked);
              setNameResult(null);
              if (!e.currentTarget.checked && accountId) setBlogId(accountId);
            }}
          />

          <Group align="flex-end" gap="sm">
            <TextInput
              label={isNewBlog ? "새 블로그명(주소)" : "블로그명(기존)"}
              placeholder="예: myblog123"
              value={blogId}
              onChange={(e) => {
                setBlogId(e.currentTarget.value);
                setNameResult(null);
              }}
              style={{ flex: 1 }}
            />
            {isNewBlog && (
              <Button variant="light" loading={nameChecking} onClick={onCheckName}>
                사용 가능 확인
              </Button>
            )}
          </Group>
          {isNewBlog && nameResult !== null && (
            <Text size="sm" c={nameResult ? "teal" : "red"}>
              {nameResult ? "✓ 사용 가능한 블로그명입니다." : "✗ 이미 사용 중인 블로그명입니다."}
            </Text>
          )}
        </Stack>
      </Paper>

      <Paper withBorder p="md" radius="md">
        <Stack gap="sm">
          <TextInput
            label="제목"
            value={title}
            onChange={(e) => setTitle(e.currentTarget.value)}
          />
          <Textarea
            label="내용"
            rows={8}
            value={content}
            onChange={(e) => setContent(e.currentTarget.value)}
          />
        </Stack>
      </Paper>

      <Paper withBorder p="md" radius="md">
        <Stack gap="sm">
          <Text fw={600}>발행 설정</Text>
          <div>
            <Text size="sm" mb={4}>
              공개 설정
            </Text>
            <SegmentedControl
              data={OPEN_TYPES}
              value={openType}
              onChange={setOpenType}
              fullWidth
            />
          </div>
          <Group gap="lg">
            <Switch
              label="댓글 허용"
              checked={commentYn}
              onChange={(e) => setCommentYn(e.currentTarget.checked)}
            />
            <Switch
              label="검색 허용"
              checked={searchYn}
              onChange={(e) => setSearchYn(e.currentTarget.checked)}
            />
            <Switch
              label="공감 허용"
              checked={sympathyYn}
              onChange={(e) => setSympathyYn(e.currentTarget.checked)}
            />
            <Switch
              label="공지 등록"
              checked={noticePostYn}
              onChange={(e) => setNoticePostYn(e.currentTarget.checked)}
            />
          </Group>
          <TextInput
            label="태그 (# 없이 공백으로 구분)"
            placeholder="첫글 인생 일상"
            value={tags}
            onChange={(e) => setTags(e.currentTarget.value)}
          />
          <Divider />
          <Radio.Group
            label="발행 시간"
            value={timeType}
            onChange={(v) => setTimeType(v as "now" | "reserve")}
          >
            <Group mt="xs">
              <Radio value="now" label="현재 발행" />
              <Radio value="reserve" label="예약 발행" />
            </Group>
          </Radio.Group>
          {timeType === "reserve" && (
            <Group gap="xs">
              <NumberInput label="년" value={rYear} onChange={(v) => setRYear(Number(v))} w={90} />
              <NumberInput label="월" value={rMonth} onChange={(v) => setRMonth(Number(v))} min={1} max={12} w={70} />
              <NumberInput label="일" value={rDate} onChange={(v) => setRDate(Number(v))} min={1} max={31} w={70} />
              <NumberInput label="시" value={rHour} onChange={(v) => setRHour(Number(v))} min={0} max={23} w={70} />
              <NumberInput label="분" value={rMinute} onChange={(v) => setRMinute(Number(v))} min={0} max={59} w={70} />
            </Group>
          )}
        </Stack>
      </Paper>

      <Group>
        <Button size="md" loading={publishing} onClick={onPublish}>
          게시하기
        </Button>
      </Group>

      {result && (
        <Alert color={result.ok ? "teal" : "red"} title={result.ok ? "성공" : "실패"}>
          <Text size="sm">{result.msg}</Text>
          {result.url && (
            <Anchor href={result.url} target="_blank" size="sm">
              {result.url}
            </Anchor>
          )}
        </Alert>
      )}
    </Stack>
  );
}
