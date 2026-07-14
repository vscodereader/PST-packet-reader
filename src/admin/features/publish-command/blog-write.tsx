// 네이버 블로그 "새 글 발행" 원격 명령(16-블로그새글). 블로그 댓글(BlogConfig)과 별개로, Admin에서
// 편집기 툴바(BlockEditor 재사용)로 제목·본문 블록·발행설정을 작성해 그 하위로 `publish_blog_write`를
// 내려보낸다. 하위는 각 대상 계정의 쿠키로 자기 블로그에 같은 글을 RabbitWrite 발행하고 결과를 회신한다.
//
// 사진/파일/링크/스티커는 계정 세션이 필요한데 Admin(브라우저)엔 없다. 그래서 원격 모드(BlockEditor
// remote)에서 원본만 담아 보내고(사진/파일=base64, 링크=URL, 스티커=정적 팩) 하위 에이전트가 발행 시
// 대상 계정 세션으로 업로드/조회해 해결한다(agent resolve_blocks_for_account). 텍스트/서식/소스코드/일정은
// 그대로 동작.

import {
  Box,
  Button,
  Checkbox,
  Group,
  Select,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useMemo, useState } from "react";

import { BlockEditor } from "@/features/blog/block-editor";
import { type Block } from "@/features/blog/blocks";

import { api, isOffline } from "../../api";

/** 공개 범위 옵션(발행설정 openType 코드 — 데스크톱 OpenType 순서와 일치). */
const OPEN_TYPES: { value: string; label: string }[] = [
  { value: "0", label: "전체공개" },
  { value: "1", label: "이웃공개" },
  { value: "2", label: "서로이웃공개" },
  { value: "3", label: "비공개" },
];

/** 발행 대상 1건 — (계정 loginId, 발행할 블로그명). */
export interface BlogWriteTargetInput {
  loginId: string;
  blogId: string;
}

/** 선택된 계정과 블로그명 오버라이드로 발행 대상 목록을 만든다(순수 함수 — 테스트 대상). blogId
 *  오버라이드가 비어 있으면 loginId를 블로그명으로 쓴다(네이버 블로그명 기본=아이디). */
export function buildBlogWriteTargets(
  accts: string[],
  blogIds: Record<string, string>,
): BlogWriteTargetInput[] {
  return accts.map((loginId) => {
    const override = (blogIds[loginId] ?? "").trim();
    return { loginId, blogId: override.length > 0 ? override : loginId };
  });
}

/** 발행 가능 조건(순수 함수 — 테스트 대상). 제목이나 본문 블록 중 하나는 있어야 하고, 대상 계정이
 *  최소 1개 있어야 한다(빈 글·대상 없는 발행 방지). */
export function canSendBlogWrite(
  title: string,
  blocks: Block[],
  accts: string[],
): boolean {
  const hasContent = title.trim().length > 0 || blocks.length > 0;
  return hasContent && accts.length > 0;
}

// 네이버 블로그 새 글 발행 구성(BlogConfig의 "새 글 발행" 모드). 로그인 성공(active) 블로그 계정만
// 대상으로 쓴다(상위 accountsFor가 걸러 넘김 — 쿠키 있는 계정만 발행 가능).
export function BlogWriteConfig({
  device,
  accounts,
}: {
  device: { id: string; name: string };
  accounts: string[];
}) {
  const [title, setTitle] = useState("");
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [accts, setAccts] = useState<string[]>([]);
  const [blogIds, setBlogIds] = useState<Record<string, string>>({});
  const [openType, setOpenType] = useState("0");
  const [commentYn, setCommentYn] = useState(true);
  const [searchYn, setSearchYn] = useState(true);
  const [tags, setTags] = useState("");

  // 미디어 보조 API용 계정(BlockEditor) — 선택된 첫 계정(없으면 null). 텍스트/서식/소스코드/일정
  // 블록은 계정 없이도 동작한다(미디어 삽입만 하위 쿠키 필요 — 위 파일 주석 참고).
  const composeAccount = accts[0] ?? null;
  const valid = canSendBlogWrite(title, blocks, accts);

  const toggleAcct = (loginId: string, checked: boolean) =>
    setAccts((prev) =>
      checked ? [...prev, loginId] : prev.filter((a) => a !== loginId),
    );

  const targets = useMemo(
    () => buildBlogWriteTargets(accts, blogIds),
    [accts, blogIds],
  );
  const detail = `본문 블록 ${blocks.length}개 · 계정 ${accts.length}`;

  const runNow = () => {
    void (async () => {
      try {
        await api.blogWrite.send({
          deviceId: device.id,
          title,
          blocks,
          settings: {
            openType: Number(openType),
            commentYn,
            searchYn,
            tags: tags.trim(),
          },
          targets,
        });
        notifications.show({
          title: `${device.name} · 블로그 새 글 발행 명령 전송`,
          message: `"${title || "(제목 없음)"}" · ${detail}`,
          color: "blue",
        });
      } catch (e) {
        if (isOffline(e)) {
          notifications.show({
            title: `${device.name} · 블로그 새 글(미리보기)`,
            message: `${detail} · 서버 오프라인(전송 안 됨)`,
            color: "gray",
          });
        } else {
          notifications.show({
            title: "블로그 새 글 발행 명령 실패",
            message: e instanceof Error ? e.message : String(e),
            color: "red",
          });
        }
      }
    })();
  };

  return (
    <Box>
      {/* 제목 */}
      <Text size="xs" c="dimmed" mb={4}>
        제목
      </Text>
      <TextInput
        size="xs"
        mb="sm"
        placeholder="블로그 글 제목"
        value={title}
        onChange={(e) => setTitle(e.currentTarget.value)}
        aria-label="블로그 글 제목"
      />

      {/* 본문 — 편집기 툴바(BlockEditor 재사용). 사진/스티커/링크 등 미디어 삽입은 계정 쿠키로 백엔드
          보조 API를 호출하므로 **대상 계정을 최소 1개 선택**해야 동작한다(미선택이면 아래 안내). */}
      <Text size="xs" c="dimmed" mb={4}>
        본문 (편집기 툴바)
      </Text>
      {composeAccount == null ? (
        <Text fz={11} c="orange.7" mb={4}>
          아래에서 대상 계정을 먼저 선택하세요. 계정을 골라야 사진·스티커·링크
          등 미디어를 삽입할 수 있습니다(텍스트·서식·소스코드·일정은 계정 없이도
          작성 가능).
        </Text>
      ) : (
        <Text fz={11} c="dimmed" mb={4}>
          미디어 삽입 계정: {composeAccount}
        </Text>
      )}
      <Box mb="sm">
        <BlockEditor
          accountId={composeAccount}
          blocks={blocks}
          onChange={setBlocks}
          remote
        />
      </Box>

      {/* 발행 설정 — 공개범위·댓글·검색·태그. */}
      <Text size="xs" c="dimmed" mb={4}>
        발행 설정
      </Text>
      <Group gap="sm" mb="sm" align="flex-end">
        <Select
          size="xs"
          label="공개 범위"
          data={OPEN_TYPES}
          value={openType}
          onChange={(v) => setOpenType(v ?? "0")}
          allowDeselect={false}
          w={140}
        />
        <Checkbox
          size="xs"
          label="댓글 허용"
          checked={commentYn}
          onChange={(e) => setCommentYn(e.currentTarget.checked)}
        />
        <Checkbox
          size="xs"
          label="검색 허용"
          checked={searchYn}
          onChange={(e) => setSearchYn(e.currentTarget.checked)}
        />
      </Group>
      <TextInput
        size="xs"
        mb="sm"
        label="태그"
        placeholder="공백으로 구분 (예: 첫글 인생)"
        value={tags}
        onChange={(e) => setTags(e.currentTarget.value)}
        aria-label="태그"
      />

      {/* 대상 계정 — 로그인 성공(active) 블로그 계정. 각 계정의 블로그명(기본=아이디)을 편집할 수 있다. */}
      <Text size="xs" c="dimmed" mb={4}>
        대상 계정 (계정마다 자기 블로그에 발행 · 블로그명 기본=아이디)
      </Text>
      <Stack gap={6} mb="sm">
        {accounts.length === 0 && (
          <Text size="xs" c="dimmed">
            로그인 성공한 블로그 계정이 없습니다.
          </Text>
        )}
        {accounts.map((loginId) => {
          const checked = accts.includes(loginId);
          return (
            <Group key={loginId} gap={8} wrap="nowrap">
              <Checkbox
                size="xs"
                label={loginId}
                checked={checked}
                onChange={(e) => toggleAcct(loginId, e.currentTarget.checked)}
                style={{ minWidth: 160 }}
              />
              {checked && (
                <TextInput
                  size="xs"
                  style={{ flex: 1 }}
                  placeholder={`블로그명 (기본 ${loginId})`}
                  value={blogIds[loginId] ?? ""}
                  onChange={(e) =>
                    setBlogIds((prev) => ({
                      ...prev,
                      [loginId]: e.currentTarget.value,
                    }))
                  }
                  styles={{ input: { fontFamily: "monospace" } }}
                  aria-label={`${loginId} 블로그명`}
                />
              )}
            </Group>
          );
        })}
      </Stack>

      <Button size="xs" disabled={!valid} onClick={runNow}>
        블로그 새 글 발행 명령 전송
      </Button>
    </Box>
  );
}
