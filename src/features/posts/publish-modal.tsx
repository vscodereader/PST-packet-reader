import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Group,
  Loader,
  Modal,
  Radio,
  SegmentedControl,
  Select,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useState } from "react";

import type { BandTarget } from "@/shared/bindings/BandTarget";
import type { BlogTarget } from "@/shared/bindings/BlogTarget";
import type { ClipTarget } from "@/shared/bindings/ClipTarget";
import type { CommentTargetSpec } from "@/shared/bindings/CommentTargetSpec";
import type { ForumTarget } from "@/shared/bindings/ForumTarget";
import type { LoginTarget } from "@/shared/bindings/LoginTarget";
import type { NaverTarget } from "@/shared/bindings/NaverTarget";
import type { PostJob } from "@/shared/bindings/PostJob";
import {
  isHiddenFromPublish,
  isPostable,
  KIND,
  STATUS_ACCOUNT,
} from "@/shared/data/config";
import {
  acctPlatforms,
  hasToken,
  resolveTemplate,
} from "@/shared/data/helpers";
import type {
  Account,
  GoFn,
  LibraryPost,
  PlatformId,
  PublishJob,
  PublishPlan,
  PublishResult,
  QueueLocation,
  QueueNowItem,
  QueueScheduledItem,
  Stock,
} from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { nowParts, scheduleMoment, toEpochMs } from "@/shared/schedule";
import { DateTimePicker } from "@/shared/ui/date-time-picker";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo, PlatformPill } from "@/shared/ui/platform-logo";

import {
  parseBandPostUrl,
  parseCafeArticleUrl,
  parseForumArticleUrl,
} from "./comment-jobs";
import { PreviewModal } from "./preview-modal";
import {
  clampCommentCount,
  distributeStocksEvenly,
  htmlToText,
  parseBlogLink,
  parseBlogPostLink,
  parseCafeBoardLink,
  parseClipLink,
} from "./publish-helpers";
import { StockCrawlModal } from "./stock-crawl-modal";

export interface PublishModalProps {
  open: boolean;
  doc: LibraryPost | null;
  onClose: () => void;
  go: GoFn;
}

/** 붙여넣은 게시판 링크 하나 → 파싱된 카페·게시판 식별자. 게시 대상 목록(밴드 미러)을
 *  이룬다. boardType은 쿠키 없이 못 받으므로 게시 시점 백엔드가 해결한다. */
interface ResolvedCafe {
  cafeId: number;
  menuId: number;
  link: string;
}

/** 게시 대상 목록에서 카페 항목의 고유 키(cafeId+menuId). */
function cafeKey(cafeId: number, menuId: number): string {
  return `${cafeId}-${menuId}`;
}

/** 붙여넣은 블로그 글 링크 하나 → 파싱된 블로그 글 식별자(#271). 블로그는 댓글 전용이라
 *  그 글 하나가 곧 대상이다. blogId는 문자열(예: "press02"), logNo는 숫자 문자열. */
interface ResolvedBlog {
  blogId: string;
  logNo: string;
  link: string;
}

/** 게시 대상 목록에서 블로그 글 항목의 고유 키(blogId+logNo). */
function blogKey(blogId: string, logNo: string): string {
  return `${blogId}/${logNo}`;
}

/** "최신 N개" 모드(#279)의 블로그 대상 — 블로그 홈 링크에서 뽑은 blogId(+카테고리). 특정 글이
 *  아니라 블로그 자체가 대상이라 게시 시점 워커가 최신 글 상위 N개를 조회해 댓글을 단다. */
interface BlogHomeTarget {
  blogId: string;
  categoryNo?: number;
  link: string;
}

/** "최신 N개" 모드(#클립)의 클립 대상 — 창작자 링크에서 뽑은 handle(+탭). 특정 영상이 아니라
 *  창작자 자체가 대상이라 게시 시점 워커가 최신 미디어 상위 N개를 조회해 댓글을 단다. */
interface ClipHomeTarget {
  handle: string;
  mediaType?: "all" | "video";
  link: string;
}

/** 저장된 밴드 링크 하나 → 조회된 실제 밴드명. 게시 대상 목록(드롭다운)을 이룬다. */
interface ResolvedBand {
  bandNo: string;
  name: string;
  link: string;
}

/** 밴드 링크에서 band_no를 뽑는다. 숫자만/`/band/{no}`/실패 시 원문 trim. */
function bandNoFromLink(link: string): string {
  const t = link.trim();
  if (/^\d+$/.test(t)) return t;
  const m = t.match(/\/band\/(\d+)/);
  return m ? m[1]! : t;
}

export function AccountRow({
  a,
  selected,
  onToggle,
}: {
  a: Account;
  selected: boolean;
  onToggle: (id: string) => void;
}) {
  const st = STATUS_ACCOUNT[a.status] ?? { t: a.status, c: "gray" };
  // 로그인 실패 계열(error/badCredentials/challenge/blocked)은 게시 대상에서 막는다.
  // active(정상)와 new(아직 미로그인, 게시 시 로그인 시도)만 선택 가능.
  const disabled = !isPostable(a.status);
  return (
    <Group
      gap={9}
      px={10}
      py={7}
      wrap="nowrap"
      onClick={() => !disabled && onToggle(a.id)}
      style={{
        borderRadius: "var(--mantine-radius-sm)",
        cursor: disabled ? "not-allowed" : "pointer",
        opacity: disabled ? 0.55 : 1,
        background: selected
          ? "var(--mantine-color-blue-light)"
          : "transparent",
      }}
    >
      <Checkbox
        checked={selected}
        onChange={() => !disabled && onToggle(a.id)}
        onClick={(e) => e.stopPropagation()}
        size="sm"
        disabled={disabled}
      />
      <PlatformLogo id={a.platform} size={24} />
      <Text fz={13} fw={700} ff="monospace" style={{ flexShrink: 0 }}>
        {a.loginId}
      </Text>
      <Group
        gap={4}
        wrap="nowrap"
        style={{ flex: 1, minWidth: 0, overflow: "hidden" }}
      >
        {(a.tags ?? []).map((t) => (
          <Badge key={t} size="xs" variant="default" radius="xl">
            # {t}
          </Badge>
        ))}
      </Group>
      {a.status !== "active" && (
        <Badge size="sm" color={st.c} variant="light">
          {st.t}
        </Badge>
      )}
    </Group>
  );
}

function DestinationPicker({
  selPlatforms,
  stockCodes,
  stockNames,
  openStockModal,
  removeStock,
  bandLink,
  setBandLink,
  bandResolving,
  resolvedBands,
  selectedBands,
  onSaveBandLink,
  onSelectBand,
  onRemoveBand,
  stocks,
  cafeLink,
  setCafeLink,
  resolvedCafes,
  selectedCafes,
  onSaveCafeLink,
  onSelectCafe,
  onRemoveCafe,
  blogLink,
  setBlogLink,
  resolvedBlogs,
  selectedBlogs,
  onSaveBlogLink,
  onSelectBlog,
  onRemoveBlog,
  blogIsListTarget,
  blogHomes,
  blogHomeLink,
  setBlogHomeLink,
  onSaveBlogHomeLink,
  onRemoveBlogHome,
  clipHomes,
  clipHomeLink,
  setClipHomeLink,
  onSaveClipHomeLink,
  onRemoveClipHome,
}: {
  selPlatforms: PlatformId[];
  stockCodes: string[];
  stockNames: Record<string, string>;
  openStockModal: () => void;
  removeStock: (code: string) => void;
  bandLink: string;
  setBandLink: (v: string) => void;
  bandResolving: boolean;
  resolvedBands: ResolvedBand[];
  selectedBands: string[];
  onSaveBandLink: () => void;
  onSelectBand: (bandNo: string) => void;
  onRemoveBand: (bandNo: string) => void;
  stocks: Stock[];
  cafeLink: string;
  setCafeLink: (v: string) => void;
  resolvedCafes: ResolvedCafe[];
  selectedCafes: string[];
  onSaveCafeLink: () => void;
  onSelectCafe: (key: string) => void;
  onRemoveCafe: (key: string) => void;
  blogLink: string;
  setBlogLink: (v: string) => void;
  resolvedBlogs: ResolvedBlog[];
  selectedBlogs: string[];
  onSaveBlogLink: () => void;
  onSelectBlog: (key: string) => void;
  onRemoveBlog: (key: string) => void;
  // 댓글 작성(commentTarget)이 최신글/인기글이면 true(블로그 링크 입력), url이면 false(글 링크 입력).
  blogIsListTarget: boolean;
  blogHomes: BlogHomeTarget[];
  blogHomeLink: string;
  setBlogHomeLink: (v: string) => void;
  onSaveBlogHomeLink: () => void;
  onRemoveBlogHome: (blogId: string, categoryNo?: number) => void;
  clipHomes: ClipHomeTarget[];
  clipHomeLink: string;
  setClipHomeLink: (v: string) => void;
  onSaveClipHomeLink: () => void;
  onRemoveClipHome: (handle: string, mediaType?: "all" | "video") => void;
}) {
  const card = {
    border: "1px solid var(--mantine-color-gray-2)",
    borderRadius: "var(--mantine-radius-md)",
    overflow: "hidden",
  };
  const head = {
    background: "var(--mantine-color-gray-0)",
    borderBottom: "1px solid var(--mantine-color-gray-2)",
  };
  return (
    <Stack gap={10}>
      {selPlatforms.includes("forum") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} wrap="nowrap" style={head}>
            <PlatformLogo id="forum" size={22} />
            <Text fz={13} fw={700} style={{ flex: 1 }}>
              종목토론방
            </Text>
            <Button
              size="compact-xs"
              radius="xl"
              variant="light"
              color="forum"
              leftSection={<Icon.globe size={13} />}
              onClick={openStockModal}
            >
              종목 선택
            </Button>
          </Group>
          <Box p={10}>
            {stockCodes.length === 0 ? (
              <Text fz={12} c="gray.5" px={2} py={4}>
                크롤링으로 게시할 종목토론방을 선택하세요.
              </Text>
            ) : (
              <Group gap={6}>
                {stockCodes.map((code) => (
                  <Group
                    key={code}
                    gap={6}
                    h={28}
                    pl={10}
                    pr={6}
                    wrap="nowrap"
                    style={{
                      borderRadius: 999,
                      background: "var(--mantine-color-forum-light)",
                    }}
                  >
                    <Text fz={12} fw={700} c="forum">
                      {stockNames[code] ??
                        stocks.find((s) => s.code === code)?.name ??
                        code}
                    </Text>
                    <ActionIcon
                      size={17}
                      radius="xl"
                      variant="transparent"
                      color="forum"
                      onClick={() => removeStock(code)}
                    >
                      <Icon.x size={11} />
                    </ActionIcon>
                  </Group>
                ))}
              </Group>
            )}
          </Box>
        </Box>
      )}
      {selPlatforms.includes("naver") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="naver" size={22} />
            <Text fz={13} fw={700}>
              네이버 카페
            </Text>
          </Group>
          <Stack gap={8} p={10}>
            {/* 게시판 목록은 쿠키 필수라 시드 로그인 없이 못 받는다(401). 그래서 올릴
                게시판의 URL을 붙여넣으면 cafeId+menuId를 파싱해 대상으로 추가한다(밴드
                링크 흐름과 동일). boardType은 게시 시점 백엔드가 해결한다. */}
            <Group gap={8} align="flex-end" wrap="nowrap">
              <TextInput
                style={{ flex: 1 }}
                label="게시판 링크"
                placeholder="https://cafe.naver.com/f-e/cafes/31732304/menus/1"
                value={cafeLink}
                onChange={(e) => setCafeLink(e.currentTarget.value)}
                leftSection={<Icon.link size={14} />}
                aria-label="카페 게시판 링크"
              />
              <Button
                variant="light"
                color="naver"
                onClick={onSaveCafeLink}
                disabled={!cafeLink.trim()}
              >
                추가
              </Button>
            </Group>
            <Select
              placeholder={
                resolvedCafes.length
                  ? "게시할 게시판 선택"
                  : "게시판 링크를 추가하면 여기 표시됩니다"
              }
              data={resolvedCafes.map((c) => ({
                value: cafeKey(c.cafeId, c.menuId),
                label: `카페 ${c.cafeId} · 게시판 ${c.menuId}`,
              }))}
              value={null}
              disabled={resolvedCafes.length === 0}
              onChange={(k) => {
                if (k) onSelectCafe(k);
              }}
            />
            {selectedCafes.length > 0 ? (
              <Group gap={6}>
                {selectedCafes.map((k) => {
                  const c = resolvedCafes.find(
                    (x) => cafeKey(x.cafeId, x.menuId) === k,
                  );
                  const label = c ? `카페 ${c.cafeId} · 게시판 ${c.menuId}` : k;
                  return (
                    <Badge
                      key={k}
                      color="naver"
                      variant="light"
                      rightSection={
                        <ActionIcon
                          size={14}
                          variant="transparent"
                          color="naver"
                          aria-label={`${label} 제거`}
                          onClick={() => onRemoveCafe(k)}
                        >
                          <Icon.x size={10} />
                        </ActionIcon>
                      }
                    >
                      {label}
                    </Badge>
                  );
                })}
              </Group>
            ) : (
              <Text fz={12} c="orange.7">
                게시할 게시판을 선택하세요.
              </Text>
            )}
          </Stack>
        </Box>
      )}
      {selPlatforms.includes("blog") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="blog" size={22} />
            <Text fz={13} fw={700}>
              네이버블로그
            </Text>
          </Group>
          <Stack gap={8} p={10}>
            {/* 블로그는 댓글 전용(#271/#279). 별도 토글 없이 댓글 작성에서 고른 대상을 따른다
                (카페와 동일): 특정 게시글=글 링크에 댓글, 최신글/인기글=블로그 링크의 최신 N개에
                댓글. 그래서 보이는 입력은 commentTarget에 따라 글 링크 / 블로그 링크 중 하나다. */}
            {blogIsListTarget ? (
              <>
                {/* 최신 N개: 블로그 홈 링크에서 blogId(+카테고리)만 뽑아 대상으로 추가한다.
                    개수는 댓글 템플릿(writer)에서 정한 commentCount를 그대로 쓴다. */}
                <Group gap={8} align="flex-end" wrap="nowrap">
                  <TextInput
                    style={{ flex: 1 }}
                    label="블로그 링크"
                    placeholder="https://blog.naver.com/press02"
                    value={blogHomeLink}
                    onChange={(e) => setBlogHomeLink(e.currentTarget.value)}
                    leftSection={<Icon.link size={14} />}
                    aria-label="블로그 링크"
                  />
                  <Button
                    variant="light"
                    color="blog"
                    onClick={onSaveBlogHomeLink}
                    disabled={!blogHomeLink.trim()}
                  >
                    추가
                  </Button>
                </Group>
                {blogHomes.length > 0 ? (
                  <Group gap={6}>
                    {blogHomes.map((b) => {
                      const label =
                        b.categoryNo !== undefined
                          ? `${b.blogId} · 카테고리 ${b.categoryNo}`
                          : b.blogId;
                      return (
                        <Badge
                          key={`${b.blogId}/${b.categoryNo ?? ""}`}
                          color="blog"
                          variant="light"
                          rightSection={
                            <ActionIcon
                              size={14}
                              variant="transparent"
                              color="blog"
                              aria-label={`${label} 제거`}
                              onClick={() =>
                                onRemoveBlogHome(b.blogId, b.categoryNo)
                              }
                            >
                              <Icon.x size={10} />
                            </ActionIcon>
                          }
                        >
                          {label}
                        </Badge>
                      );
                    })}
                  </Group>
                ) : (
                  <Text fz={12} c="orange.7">
                    댓글을 달 블로그를 추가하세요.
                  </Text>
                )}
              </>
            ) : (
              <>
                {/* 특정 글 URL: 링크에서 blogId(문자열)+logNo를 파싱해 대상으로 추가한다(#271). */}
                <Group gap={8} align="flex-end" wrap="nowrap">
                  <TextInput
                    style={{ flex: 1 }}
                    label="글 링크"
                    placeholder="https://blog.naver.com/press02/224311392458"
                    value={blogLink}
                    onChange={(e) => setBlogLink(e.currentTarget.value)}
                    leftSection={<Icon.link size={14} />}
                    aria-label="블로그 글 링크"
                  />
                  <Button
                    variant="light"
                    color="blog"
                    onClick={onSaveBlogLink}
                    disabled={!blogLink.trim()}
                  >
                    추가
                  </Button>
                </Group>
                <Select
                  placeholder={
                    resolvedBlogs.length
                      ? "게시할 블로그 글 선택"
                      : "글 링크를 추가하면 여기 표시됩니다"
                  }
                  data={resolvedBlogs.map((b) => ({
                    value: blogKey(b.blogId, b.logNo),
                    label: `${b.blogId} · 글 ${b.logNo}`,
                  }))}
                  value={null}
                  disabled={resolvedBlogs.length === 0}
                  onChange={(k) => {
                    if (k) onSelectBlog(k);
                  }}
                />
                {selectedBlogs.length > 0 ? (
                  <Group gap={6}>
                    {selectedBlogs.map((k) => {
                      const b = resolvedBlogs.find(
                        (x) => blogKey(x.blogId, x.logNo) === k,
                      );
                      const label = b ? `${b.blogId} · 글 ${b.logNo}` : k;
                      return (
                        <Badge
                          key={k}
                          color="blog"
                          variant="light"
                          rightSection={
                            <ActionIcon
                              size={14}
                              variant="transparent"
                              color="blog"
                              aria-label={`${label} 제거`}
                              onClick={() => onRemoveBlog(k)}
                            >
                              <Icon.x size={10} />
                            </ActionIcon>
                          }
                        >
                          {label}
                        </Badge>
                      );
                    })}
                  </Group>
                ) : (
                  <Text fz={12} c="orange.7">
                    게시할 블로그 글을 선택하세요.
                  </Text>
                )}
              </>
            )}
          </Stack>
        </Box>
      )}
      {selPlatforms.includes("clip") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="clip" size={22} />
            <Text fz={13} fw={700}>
              네이버 클립
            </Text>
          </Group>
          <Stack gap={8} p={10}>
            {/* 클립은 댓글 전용(#클립)·항상 "최신 N개". 창작자 링크(@아이디)를 추가하면 그 창작자의
                최신 미디어 상위 N개(개수=댓글 템플릿 commentCount)에 댓글을 단다. ?tab=video면 영상만. */}
            <Group gap={8} align="flex-end" wrap="nowrap">
              <TextInput
                style={{ flex: 1 }}
                label="클립 링크"
                placeholder="https://clip.naver.com/@dongzzi_chef"
                value={clipHomeLink}
                onChange={(e) => setClipHomeLink(e.currentTarget.value)}
                leftSection={<Icon.link size={14} />}
                aria-label="클립 링크"
              />
              <Button
                variant="light"
                color="green"
                onClick={onSaveClipHomeLink}
                disabled={!clipHomeLink.trim()}
              >
                추가
              </Button>
            </Group>
            {clipHomes.length > 0 ? (
              <Group gap={6}>
                {clipHomes.map((c) => {
                  const label =
                    c.mediaType === "video"
                      ? `@${c.handle} · 영상만`
                      : `@${c.handle}`;
                  return (
                    <Badge
                      key={`${c.handle}/${c.mediaType ?? ""}`}
                      color="green"
                      variant="light"
                      rightSection={
                        <ActionIcon
                          size={14}
                          variant="transparent"
                          color="green"
                          aria-label={`${label} 제거`}
                          onClick={() =>
                            onRemoveClipHome(c.handle, c.mediaType)
                          }
                        >
                          <Icon.x size={10} />
                        </ActionIcon>
                      }
                    >
                      {label}
                    </Badge>
                  );
                })}
              </Group>
            ) : (
              <Text fz={12} c="orange.7">
                댓글을 달 클립 창작자를 추가하세요.
              </Text>
            )}
          </Stack>
        </Box>
      )}
      {selPlatforms.includes("band") && (
        <Box style={card}>
          <Group gap={9} px={11} py={9} style={head}>
            <PlatformLogo id="band" size={22} />
            <Text fz={13} fw={700}>
              밴드
            </Text>
          </Group>
          <Stack gap={8} p={10}>
            {/* 사수 요구 흐름: 가입할 밴드 링크를 한 줄씩 입력→저장하면 실제 밴드명을
                조회해 아래 드롭다운(사수 UI)에 누적. 거기서 게시할 밴드를 다중 선택→칩. */}
            <Group gap={8} align="flex-end" wrap="nowrap">
              <TextInput
                style={{ flex: 1 }}
                label="가입할 밴드 링크"
                placeholder="https://band.us/band/103043410"
                value={bandLink}
                onChange={(e) => setBandLink(e.currentTarget.value)}
                leftSection={<Icon.link size={14} />}
                aria-label="밴드 링크"
              />
              <Button
                variant="light"
                color="band"
                onClick={onSaveBandLink}
                disabled={!bandLink.trim() || bandResolving}
              >
                저장
              </Button>
            </Group>
            {bandResolving && (
              <Group gap={6} wrap="nowrap">
                <Loader size="xs" />
                <Text fz={12} c="dimmed">
                  밴드 정보를 확인하는 중…
                </Text>
              </Group>
            )}
            {/* 사수의 드롭다운: 저장으로 누적된 실제 밴드명 목록에서 게시할 밴드 선택.
                옵션 value는 고유한 band_no, label은 표시용 밴드명 — 이름이 같은 밴드가
                둘 이상이어도 value가 겹치지 않아 Select가 깨지지(흰 화면) 않는다. */}
            <Select
              placeholder={
                resolvedBands.length
                  ? "게시할 밴드 선택"
                  : "링크를 저장하면 밴드가 여기 표시됩니다"
              }
              data={resolvedBands.map((b) => ({
                value: b.bandNo,
                label: b.name,
              }))}
              value={null}
              disabled={resolvedBands.length === 0}
              onChange={(no) => {
                if (no) onSelectBand(no);
              }}
            />
            {selectedBands.length > 0 ? (
              <Group gap={6}>
                {selectedBands.map((no) => {
                  const b = resolvedBands.find((x) => x.bandNo === no);
                  return (
                    <Badge
                      key={no}
                      color="band"
                      variant="light"
                      rightSection={
                        <ActionIcon
                          size={14}
                          variant="transparent"
                          color="band"
                          aria-label={`${b?.name ?? no} 제거`}
                          onClick={() => onRemoveBand(no)}
                        >
                          <Icon.x size={10} />
                        </ActionIcon>
                      }
                    >
                      {b?.name ?? no}
                    </Badge>
                  );
                })}
              </Group>
            ) : (
              <Text fz={12} c="orange.7">
                게시할 밴드를 선택하세요.
              </Text>
            )}
          </Stack>
        </Box>
      )}
    </Stack>
  );
}

function PublishFlow({
  state,
  mode,
  when,
  date,
  time,
  count,
  onClose,
  onKeepWriting,
  go,
}: {
  state: null | "running" | PublishResult[];
  mode: LibraryPost["kind"];
  when: "now" | "schedule";
  date: string;
  time: string;
  count: number;
  onClose: () => void;
  // '계속작성': 게시설정창을 닫지 않고 결과 패널만 닫은 뒤 계정 목록을 새로고침한다(#5).
  onKeepWriting: () => void;
  go: GoFn;
}) {
  if (!state) return null;
  const running = state === "running";
  const results = Array.isArray(state) ? state : [];
  const okCount = results.filter((r) => r.ok).length;
  const allOk = results.length > 0 && okCount === results.length;
  const actionWord =
    mode === "comment" ? "댓글" : mode === "both" ? "글·댓글" : "글";

  return (
    <Modal
      opened
      onClose={running ? () => {} : onClose}
      withCloseButton={false}
      size={480}
      radius="lg"
      centered
    >
      <Stack align="center" gap={0} py={14} px={8}>
        {running ? (
          <>
            <ThemeIcon
              size={64}
              radius="xl"
              variant="light"
              color="blue"
              mb={18}
            >
              <Loader size="md" />
            </ThemeIcon>
            <Text fz={19} fw={800} mb={6}>
              {when === "schedule"
                ? `${actionWord} 예약하는 중…`
                : `${actionWord} 게시하는 중…`}
            </Text>
            <Text fz={14} c="dimmed">
              선택한 {count}곳에 차례로 업로드하고 있어요.
            </Text>
          </>
        ) : (
          <>
            <ThemeIcon
              size={64}
              radius="xl"
              variant="light"
              color={allOk ? "green" : "yellow"}
              mb={16}
            >
              {allOk ? (
                <Icon.checkCircle size={36} />
              ) : (
                <Icon.alert size={34} />
              )}
            </ThemeIcon>
            <Text fz={20} fw={800} mb={6}>
              {allOk
                ? when === "schedule"
                  ? "예약 완료!"
                  : "등록 완료!"
                : `${results.length}곳 중 ${okCount}곳 성공`}
            </Text>
            <Text fz={14} c="dimmed" mb={20} ta="center">
              {when === "schedule"
                ? `${date} ${time}에 자동 ${actionWord} 게시됩니다`
                : allOk
                  ? "게시큐에 정상적으로 등록되었어요!"
                  : "일부 위치는 다시 시도해 주세요"}
            </Text>
            <Stack
              gap={8}
              w="100%"
              mb={22}
              style={{ maxHeight: 280, overflowY: "auto" }}
            >
              {results.map((r) => (
                <Group
                  key={r.key}
                  gap={11}
                  px={13}
                  py={11}
                  wrap="nowrap"
                  style={{
                    borderRadius: "var(--mantine-radius-md)",
                    border: "1px solid var(--mantine-color-gray-2)",
                    background: "var(--mantine-color-gray-0)",
                  }}
                >
                  <PlatformLogo id={r.platform} size={30} />
                  <Box style={{ flex: 1, minWidth: 0 }}>
                    <Group gap={6} wrap="nowrap">
                      <Text fz={13.5} fw={700} truncate>
                        {r.targetName}
                      </Text>
                      {r.code && (
                        <Badge size="xs" color="forum" variant="light">
                          {r.code}
                        </Badge>
                      )}
                    </Group>
                    <Text fz={11.5} c={r.ok ? "dimmed" : "red"}>
                      {r.loginId} · {r.msg}
                    </Text>
                  </Box>
                  {r.ok ? (
                    <ThemeIcon variant="transparent" color="green">
                      <Icon.checkCircle size={22} />
                    </ThemeIcon>
                  ) : (
                    <Button size="compact-xs" variant="light" color="red">
                      재시도
                    </Button>
                  )}
                </Group>
              ))}
            </Stack>
            <Group gap={9} grow w="100%">
              <Button size="sm" variant="default" onClick={onKeepWriting}>
                계속 작성
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  onClose();
                  // 즉시 게시도 게시 큐(즉시 처리 대기열)에 적재되므로(#198), 예약과 마찬가지로
                  // 큐로 보내 진행률을 보게 한다(예전엔 즉시 게시가 곧장 알림 로그로 갔다).
                  go("queue");
                }}
              >
                {when === "schedule" ? "큐 보기" : "게시큐 보기"}
              </Button>
            </Group>
          </>
        )}
      </Stack>
    </Modal>
  );
}

// 큐 아이템 ID 일련번호. "나눠서 게시"는 계정마다 큐 아이템을 같은 동기 tick에 만드는데,
// 예전 "qn"+Date.now()는 밀리초 해상도라 그것들이 *전부 같은 ID*가 됐다 — 백엔드가 ID로
// 일괄 처리(mark_running/apply_cancel_now가 같은 ID 전부에 적용)하므로 N개 계정 중 1개만
// 살고 나머지는 post 0으로 증발했다(#6). 모듈 전역 카운터로 같은 tick에도 반드시 달라지게 한다.
let queueIdSeq = 0;

/** 충돌 불가능한 큐 아이템 ID 접미사. 같은 tick에도 카운터로 항상 고유하고, 시간+랜덤을
 *  섞어 앱 재시작 뒤에도 겹치지 않는다(crypto.randomUUID 있으면 그걸 섞는다). */
function freshIdSuffix(): string {
  queueIdSeq += 1;
  const rand =
    globalThis.crypto?.randomUUID?.() ??
    Math.random().toString(36).slice(2, 10);
  return `${Date.now()}-${queueIdSeq}-${rand}`;
}

/** Fresh id for a newly scheduled queue item (kept out of render per purity). */
function newScheduledId(): string {
  return "qs" + freshIdSuffix();
}

/** Fresh id for an item appended to the immediate-processing queue (now 큐). */
function newNowId(): string {
  return "qn" + freshIdSuffix();
}

function PublishModalInner({ open, doc, onClose, go }: PublishModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [stocks, setStocks] = useState<Stock[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  // 기본 선택 종목 없음(#267-11: 삼성전자 기본 선택 해제). 사용자가 직접 골라야 한다.
  const [stockCodes, setStockCodes] = useState<string[]>([]);
  // 라이브 검색으로 고른 종목의 이름(시드 목록에 없을 수 있어 onConfirm에서 받아둠).
  const [stockNames, setStockNames] = useState<Record<string, string>>({});
  const [stockModal, setStockModal] = useState(false);
  // 밴드: 링크를 한 줄씩 저장하면 그 링크의 실제 밴드명을 조회해 resolvedBands에 누적하고,
  // 사수의 드롭다운에서 게시할 밴드(selectedBands=bandNo[])를 다중 선택한다.
  const [resolvedBands, setResolvedBands] = useState<ResolvedBand[]>([]);
  const [selectedBands, setSelectedBands] = useState<string[]>([]);
  const [bandLink, setBandLink] = useState("");
  const [bandResolving, setBandResolving] = useState(false);

  // 링크 저장: 링크에서 band_no를 뽑고, 선택된 밴드 계정의 쿠키로 실제 밴드명을 조회해
  // resolvedBands에 추가한다(같은 band_no는 중복 제거). 조회 실패 시 링크를 이름으로 폴백.
  const saveBandLink = () => {
    const link = bandLink.trim();
    if (!link) return;
    const bandNo = bandNoFromLink(link);
    const bandAcct = selected
      .map((id) => accounts.find((a) => a.id === id))
      .find((a): a is Account => !!a && a.platform === "band");
    setBandLink("");
    const add = (name: string) =>
      setResolvedBands((prev) =>
        prev.some((b) => b.bandNo === bandNo)
          ? prev
          : [...prev, { bandNo, name, link }],
      );
    if (!bandAcct) {
      add(link);
      return;
    }
    setBandResolving(true);
    ipc.band
      .resolveName(bandAcct.loginId, link)
      .then((name) => add(name))
      .catch(() => add(link))
      .finally(() => setBandResolving(false));
  };
  const selectBand = (no: string) =>
    setSelectedBands((s) => (s.includes(no) ? s : [...s, no]));
  const removeBand = (no: string) =>
    setSelectedBands((s) => s.filter((x) => x !== no));
  // 카페: 게시판 링크를 추가하면 cafeId+menuId를 파싱해 resolvedCafes에 누적하고,
  // selectedCafes(키 cafeId-menuId)에서 게시할 게시판을 다중 선택한다(밴드 미러).
  // 게시판 목록은 쿠키 필수라 시드 로그인 없이 못 받으므로 링크로만 받는다.
  const [resolvedCafes, setResolvedCafes] = useState<ResolvedCafe[]>([]);
  const [selectedCafes, setSelectedCafes] = useState<string[]>([]);
  const [cafeLink, setCafeLink] = useState("");
  // 블로그(#271): 댓글 전용이라 댓글 달 글 링크를 추가하면 blogId+logNo를 파싱해 resolvedBlogs에
  // 누적하고, selectedBlogs(키 blogId/logNo)에서 게시할 글을 다중 선택한다(카페 미러).
  const [resolvedBlogs, setResolvedBlogs] = useState<ResolvedBlog[]>([]);
  const [selectedBlogs, setSelectedBlogs] = useState<string[]>([]);
  const [blogLink, setBlogLink] = useState("");
  // 블로그 댓글 대상은 별도 토글 없이 댓글 작성(writer-modal)의 commentTarget을 따른다(카페와
  // 동일). url=특정 글 링크(selectedBlogs), latest/popular=블로그 링크의 최신 N개(blogHomes).
  // 블로그는 '인기글' 목록 API가 없어 popular도 latest와 동일하게 최신 N개로 처리한다.
  const [blogHomes, setBlogHomes] = useState<BlogHomeTarget[]>([]);
  const [blogHomeLink, setBlogHomeLink] = useState("");
  // 클립(#클립)도 댓글 전용·항상 최신 N개. 창작자 링크(@handle)를 추가하면 clipHomes에 누적한다.
  const [clipHomes, setClipHomes] = useState<ClipHomeTarget[]>([]);
  const [clipHomeLink, setClipHomeLink] = useState("");
  const [when, setWhen] = useState<"now" | "schedule">("now");
  const [date, setDate] = useState(() => nowParts().date);
  const [time, setTime] = useState(() => nowParts().time);
  const [acctFilter, setAcctFilter] = useState<"all" | PlatformId>("all");
  // 댓글 대상 글 개수(최신글/인기글)는 댓글 템플릿(writer-modal)에서 정한 값을
  // 그대로 쓴다. 게시 모달에서 다시 고르지 않는다(중복 UI 제거). 1~50로 정규화하고
  // 범위 밖·누락은 1로 떨어진다.
  const commentCount = clampCommentCount(doc?.commentCount);
  const [linkOverride, setLinkOverride] = useState("");
  const [showPreview, setShowPreview] = useState(false);
  const [flow, setFlow] = useState<null | "running" | PublishResult[]>(null);
  // 이번 모달 세션에서 이미 게시(큐 적재)한 계정의 loginId(#4/#5). 게시 직후 그 자리에서
  // 목록·선택에서 빼고, '계속작성'으로 목록을 새로고침해도 다시 나타나지 않게 한다(백엔드가
  // 아직 '대기'로 바꾸기 전이라도). 모달을 다시 열면(remount) 비워져 정상 목록으로 돌아간다.
  const [submittedLoginIds, setSubmittedLoginIds] = useState<Set<string>>(
    () => new Set(),
  );

  useEffect(() => {
    void ipc.accounts.list().then((a) => {
      setAccounts(a);
      // 게시 가능한 계정만 선택 대상이다. 기존 선택에서 로그인 실패 계열을 걸러내고
      // (모달 진입 시 실패 계정이 체크된 채 게시 위치가 파생되는 것을 막는다), 남은 게
      // 없으면 첫 게시 가능 계정 하나를 기본 선택한다.
      const postable = a.filter((x) => isPostable(x.status));
      const postableIds = new Set(postable.map((x) => x.id));
      setSelected((s) => {
        const kept = s.filter((id) => postableIds.has(id));
        if (kept.length) return kept;
        return postable[0] ? [postable[0].id] : [];
      });
    });
    void ipc.stocks.list().then(setStocks);
  }, []);

  // 계정 목록을 다시 불러와(새로고침) 상태를 갱신한다(#4/#5). 게시(숫자)로 큐에 적재된 뒤,
  // 또는 '계속작성'으로 모달을 유지할 때 호출해, 백엔드가 갱신한 계정 상태(예: 게시 성공 →
  // 대기)를 반영한다. 게시 가능하지 않게 된 계정은 selected에서도 걸러낸다(모달 진입 로직과 동일).
  const refreshAccounts = () => {
    void ipc.accounts.list().then((a) => {
      setAccounts(a);
      // 게시 가능하지 않게 됐거나(상태 변화) 이번 세션에서 이미 게시한 계정은 선택에서 뺀다.
      const postableIds = new Set(
        a.filter((x) => isPostable(x.status)).map((x) => x.id),
      );
      setSelected((s) =>
        s.filter((id) => {
          if (!postableIds.has(id)) return false;
          const acc = a.find((x) => x.id === id);
          return !acc || !submittedLoginIds.has(acc.loginId);
        }),
      );
    });
  };

  // 게시(숫자)를 누른 그 자리에서 방금 게시한 계정을 체크박스 목록에서 즉시 사라지게 한다(#4).
  // 그 loginId를 submittedLoginIds에 기록해 visibleAccts에서 숨기고(목록에서 즉시 제거),
  // selected에서도 뺀다. 기록은 새로고침(refreshAccounts)에도 유지돼, 백엔드가 아직 '대기'로
  // 바꾸기 전이라도 방금 쓴 계정이 다시 나타나지 않는다(모달을 다시 열면 remount로 비워진다).
  const removeSubmittedAccounts = (jobs: PublishJob[]) => {
    const submitted = new Set(jobs.map((j) => j.loginId));
    setSubmittedLoginIds((prev) => new Set([...prev, ...submitted]));
    setSelected((s) =>
      s.filter((id) => {
        const a = accounts.find((x) => x.id === id);
        return !a || !submitted.has(a.loginId);
      }),
    );
  };

  // 게시판 링크 저장: 링크에서 cafeId+menuId를 파싱해 resolvedCafes에 추가한다
  // (같은 cafeId+menuId는 중복 제거). 파싱 실패(카페 홈 링크 등)면 추가하지 않고 안내한다.
  const saveCafeLink = () => {
    const link = cafeLink.trim();
    if (!link) return;
    const parsed = parseCafeBoardLink(link);
    if (!parsed) {
      notifications.show({
        message:
          "게시판 URL을 인식하지 못했어요. 올릴 게시판으로 들어가 그 주소를 붙여넣어 주세요.",
        color: "red",
      });
      return;
    }
    setCafeLink("");
    setResolvedCafes((prev) =>
      prev.some((c) => c.cafeId === parsed.cafeId && c.menuId === parsed.menuId)
        ? prev
        : [...prev, { cafeId: parsed.cafeId, menuId: parsed.menuId, link }],
    );
  };
  const selectCafe = (key: string) =>
    setSelectedCafes((s) => (s.includes(key) ? s : [...s, key]));
  const removeCafe = (key: string) =>
    setSelectedCafes((s) => s.filter((x) => x !== key));

  // 블로그 글 링크 추가: 링크에서 blogId+logNo를 파싱해 resolvedBlogs에 누적한다(중복 제거).
  // 파싱 실패(글이 아닌 링크 등)면 추가하지 않고 안내한다(카페 미러).
  const saveBlogLink = () => {
    const link = blogLink.trim();
    if (!link) return;
    const parsed = parseBlogPostLink(link);
    if (!parsed) {
      notifications.show({
        message:
          "블로그 글 URL을 인식하지 못했어요. 댓글을 달 블로그 글로 들어가 그 주소를 붙여넣어 주세요.",
        color: "red",
      });
      return;
    }
    setBlogLink("");
    setResolvedBlogs((prev) =>
      prev.some((b) => b.blogId === parsed.blogId && b.logNo === parsed.logNo)
        ? prev
        : [...prev, { blogId: parsed.blogId, logNo: parsed.logNo, link }],
    );
  };
  const selectBlog = (key: string) =>
    setSelectedBlogs((s) => (s.includes(key) ? s : [...s, key]));
  const removeBlog = (key: string) =>
    setSelectedBlogs((s) => s.filter((x) => x !== key));

  // "최신 N개" 모드(#279): 블로그 홈 링크에서 blogId(+categoryNo)를 파싱해 blogHomes에 누적한다
  // (중복 제거). 파싱 실패면 추가하지 않고 안내한다(특정 글 URL 흐름 미러).
  const saveBlogHomeLink = () => {
    const link = blogHomeLink.trim();
    if (!link) return;
    const parsed = parseBlogLink(link);
    if (!parsed) {
      notifications.show({
        message:
          "블로그 주소를 인식하지 못했어요. 댓글을 달 블로그 홈으로 들어가 그 주소를 붙여넣어 주세요.",
        color: "red",
      });
      return;
    }
    setBlogHomeLink("");
    setBlogHomes((prev) =>
      prev.some(
        (b) => b.blogId === parsed.blogId && b.categoryNo === parsed.categoryNo,
      )
        ? prev
        : [
            ...prev,
            {
              blogId: parsed.blogId,
              ...(parsed.categoryNo !== undefined
                ? { categoryNo: parsed.categoryNo }
                : {}),
              link,
            },
          ],
    );
  };
  const removeBlogHome = (blogId: string, categoryNo?: number) =>
    setBlogHomes((s) =>
      s.filter((b) => !(b.blogId === blogId && b.categoryNo === categoryNo)),
    );

  // 클립(#클립): 창작자 링크(@handle)를 파싱해 clipHomes에 누적한다(중복 제거). 파싱 실패면 안내.
  const saveClipHomeLink = () => {
    const link = clipHomeLink.trim();
    if (!link) return;
    const parsed = parseClipLink(link);
    if (!parsed) {
      notifications.show({
        message:
          "클립 주소를 인식하지 못했어요. 댓글을 달 창작자 페이지(clip.naver.com/@아이디)로 들어가 그 주소를 붙여넣어 주세요.",
        color: "red",
      });
      return;
    }
    setClipHomeLink("");
    setClipHomes((prev) =>
      prev.some(
        (c) => c.handle === parsed.handle && c.mediaType === parsed.mediaType,
      )
        ? prev
        : [
            ...prev,
            {
              handle: parsed.handle,
              ...(parsed.mediaType ? { mediaType: parsed.mediaType } : {}),
              link,
            },
          ],
    );
  };
  const removeClipHome = (handle: string, mediaType?: "all" | "video") =>
    setClipHomes((s) =>
      s.filter((c) => !(c.handle === handle && c.mediaType === mediaType)),
    );

  if (!doc) {
    return <Modal opened={false} onClose={onClose} />;
  }

  const mode = doc.kind;
  const comments = (doc.comments ?? []).filter(Boolean);
  const commentTargetMode = doc.commentTarget ?? "latest";
  // Three comment targets are wired: a pasted article URL (`url`), and the cafe's
  // latest/popular lists (`latest`/`popular`) fetched via list_cafe_articles —
  // the top-N (`commentCount`) of that list become the targets at publish time.
  const isListTarget =
    commentTargetMode === "latest" || commentTargetMode === "popular";
  // 특정 게시글(url) 댓글의 대상 링크들. 여러 링크를 넣으면 각 링크의 글마다 댓글이 달린다.
  // 단일 commentUrl(기존)도 길이 1로 펴 하위호환한다. 각 링크를 플랫폼별 파서에 통과시켜,
  // 매칭되는 플랫폼의 대상 목록에 들어간다(카페/종토방/밴드 글이 섞여 있어도 각자 잡힌다).
  const isUrlComment = mode === "comment" && commentTargetMode === "url";
  const commentUrls = doc.commentUrls?.length
    ? doc.commentUrls
    : doc.commentUrl
      ? [doc.commentUrl]
      : [];
  const urlTargets = isUrlComment
    ? commentUrls.map((u) => parseCafeArticleUrl(u)).filter((t) => t !== null)
    : [];
  const forumUrlTargets = isUrlComment
    ? commentUrls.map((u) => parseForumArticleUrl(u)).filter((t) => t !== null)
    : [];
  const bandUrlTargets = isUrlComment
    ? commentUrls.map((u) => parseBandPostUrl(u)).filter((t) => t !== null)
    : [];
  const toggle = (id: string) => {
    // 게시 불가 계정(로그인 실패 계열)은 선택에 넣지 않는다 — 방어선(클릭은 disabled로
    // 이미 막히지만, 어떤 경로로도 실패 계정이 selected에 들어오지 못하게 한다).
    const acc = accounts.find((x) => x.id === id);
    if (acc && !isPostable(acc.status)) return;
    // 계정 선택을 바꾸면 이전 계정 기준으로 고른 종목을 비운다(#267-2). A에서 고른 종목이
    // B로 전환(A 해제 + B 체크)했을 때 그대로 남지 않고 공백으로 시작하게 한다.
    setStockCodes([]);
    setStockNames({});
    setSelected((s) =>
      s.includes(id) ? s.filter((x) => x !== id) : [...s, id],
    );
  };
  // 게시 위치·잡 산출은 게시 가능한 선택 계정만 본다. selected는 위에서 정화되지만,
  // 파생 지점에서도 한 번 더 걸러 실패 계정이 게시 위치에 절대 새어 나오지 않게 한다.
  const usableSelected = selected.filter((id) => {
    const acc = accounts.find((x) => x.id === id);
    return !!acc && isPostable(acc.status);
  });
  const selPlatforms = acctPlatforms(usableSelected, accounts);
  const selectedNaver = usableSelected
    .map((id) => accounts.find((a) => a.id === id))
    .filter((a): a is Account => !!a && a.platform === "naver");

  const acctFilters = [
    { value: "all", label: "전체" },
    { value: "forum", label: "종목토론방" },
    { value: "naver", label: "네이버 카페" },
    { value: "blog", label: "네이버블로그" },
    { value: "band", label: "밴드" },
  ];
  const visibleAccts = accounts.filter(
    (a) =>
      // 게시 목록에서 아예 숨기는 종료성 상태: 대기(게시 성공 후, #267-3)·차단(도중 차단, #2)·
      // 대기초과(서버/타임아웃 실패, #7). 비활성(회색)이 아니라 목록에서 제거한다(사용자 지시:
      // 차단·대기초과도 대기처럼 안 보이게). 계정 화면에서 상태 배지를 눌러 다시 활성으로 바꾸면
      // 게시 가능 상태가 되어 자동으로 다시 보인다.
      !isHiddenFromPublish(a.status) &&
      // 이번 세션에서 방금 게시(큐 적재)한 계정은 그 자리에서 숨긴다(#4) — 백엔드가 '대기'로
      // 바꾸기 전이라도 목록에서 즉시 빠진다. 모달을 다시 열면(remount) 다시 보인다.
      !submittedLoginIds.has(a.loginId) &&
      (acctFilter === "all" || a.platform === acctFilter),
  );
  const visUsable = visibleAccts
    .filter((a) => isPostable(a.status))
    .map((a) => a.id);
  const allVisibleOn =
    visUsable.length > 0 && visUsable.every((id) => selected.includes(id));
  const selectAllVisible = () => {
    // 계정 선택 변경 → 이전 계정 기준 종목 비움(#267-2, toggle과 동일 규칙).
    setStockCodes([]);
    setStockNames({});
    setSelected((s) =>
      allVisibleOn
        ? s.filter((id) => !visUsable.includes(id))
        : [...new Set([...s, ...visUsable])],
    );
  };

  const jobs: PublishJob[] = [];
  usableSelected.forEach((aid) => {
    const a = accounts.find((x) => x.id === aid);
    if (!a) return;
    if (a.platform === "forum") {
      if (forumUrlTargets.length > 0) {
        // "특정 게시글" 댓글: URL이 곧 대상이라 종목 선택이 필요 없다 — 링크(대상)마다 잡 1개.
        // 여러 링크면 각 링크의 글에 모두 댓글이 달린다. code는 URL에서 뽑은 종목코드(토큰
        // 치환·라벨용), commentUrl은 댓글 달 글 URL.
        forumUrlTargets.forEach((t, i) =>
          jobs.push({
            key: `${aid}-u${i}`,
            platform: "forum",
            loginId: a.loginId,
            targetName: `종목토론방 글 #${t.postId}`,
            code: t.code,
            commentUrl: t.url,
            board: "댓글",
            status: a.status,
          }),
        );
      } else {
        stockCodes.forEach((code) =>
          jobs.push({
            key: aid + "-" + code,
            platform: "forum",
            loginId: a.loginId,
            targetName:
              stockNames[code] ??
              stocks.find((x) => x.code === code)?.name ??
              code,
            code,
            board: "종목토론방",
            status: a.status,
          }),
        );
      }
    } else if (a.platform === "naver") {
      if (isUrlComment) {
        // url 댓글 대상은 카페 게시판이 필요 없다(특정 글이 곧 대상) — 링크(글)마다 잡 1개.
        // 여러 카페 글 링크면 각 글에 모두 댓글이 달린다. cafeId+articleId를 잡에 동결한다.
        urlTargets.forEach((t, i) =>
          jobs.push({
            key: `${aid}-u${i}`,
            platform: "naver",
            loginId: a.loginId,
            targetName: `게시글 #${t.articleId}`,
            cafeId: t.cafeId,
            articleId: t.articleId,
            board: "댓글",
            status: a.status,
          }),
        );
      } else {
        // 글/글+댓글, 그리고 댓글(최신/인기)은 선택한 게시판마다(계정×게시판) 잡을
        // 만든다(밴드 미러). 댓글(최신/인기)은 그 카페의 글 목록을 읽을 대상이 된다.
        const isComment = mode === "comment";
        const targetName = isComment
          ? commentTargetMode === "popular"
            ? `인기글 ${commentCount}건`
            : `최신글 ${commentCount}건`
          : undefined;
        selectedCafes.forEach((k) => {
          const c = resolvedCafes.find(
            (x) => cafeKey(x.cafeId, x.menuId) === k,
          );
          if (!c) return;
          jobs.push({
            key: `${aid}-${k}`,
            platform: "naver",
            loginId: a.loginId,
            targetName: targetName ?? `카페 ${c.cafeId}`,
            cafeId: c.cafeId,
            menuId: c.menuId,
            board: isComment ? "댓글" : `게시판 ${c.menuId}`,
            status: a.status,
          });
        });
      }
    } else if (a.platform === "band") {
      if (bandUrlTargets.length > 0) {
        // "특정 게시글" 댓글: URL이 곧 대상(글)이라 '밴드 선택'이 필요 없다 — 링크(글)마다 잡 1개.
        // 여러 밴드 글 링크면 각 글에 모두 댓글이 달린다. link에 글 URL(band_no+post_no 포함)을
        // 동결해 워커가 그 글에 바로 댓글을 단다.
        bandUrlTargets.forEach((t, i) =>
          jobs.push({
            key: `${aid}-u${i}`,
            platform: "band",
            loginId: a.loginId,
            targetName: `밴드 글 #${t.postNo}`,
            bandLink: t.url,
            board: "댓글",
            status: a.status,
          }),
        );
      } else {
        // 선택한 각 밴드마다 잡 1개(계정 × 밴드). 라벨은 조회된 실제 밴드명. 댓글 전용 모드면
        // 대상(최신글/인기글 + 개수)을, 그 외엔 "전체글"을 board 라벨로 둔다.
        const bandBoard =
          mode === "comment"
            ? commentTargetMode === "popular"
              ? `인기글 ${commentCount}건`
              : `최신글 ${commentCount}건`
            : "전체글";
        selectedBands.forEach((no) => {
          const b = resolvedBands.find((x) => x.bandNo === no);
          if (!b) return;
          jobs.push({
            key: `${aid}-${no}`,
            platform: "band",
            loginId: a.loginId,
            targetName: b.name,
            // 가입 링크를 잡에 동결한다(밴드명이 같은 다른 밴드와의 오조회 방지).
            bandLink: b.link,
            board: bandBoard,
            status: a.status,
          });
        });
      }
    } else if (a.platform === "blog") {
      // 블로그는 댓글 전용(#271). 댓글 작성의 commentTarget을 따른다(카페와 동일, 별도 토글 없음).
      // url이면 선택한 각 글마다 잡 1개(계정×글)로 blogId+logNo를 동결한다. 최신글/인기글이면
      // 추가한 각 블로그(링크)마다 잡 1개로 blogId(+categoryNo)+개수(blogCount=commentCount)를
      // 동결해, 워커가 그 블로그의 최신 글 상위 N개에 댓글을 단다(블로그는 인기글=최신글 동일).
      if (isListTarget) {
        blogHomes.forEach((b, i) => {
          jobs.push({
            key: `${aid}-bh${i}`,
            platform: "blog",
            loginId: a.loginId,
            targetName: `${b.blogId} · 최신 ${commentCount}건`,
            blogId: b.blogId,
            logNo: "",
            blogCount: commentCount,
            ...(b.categoryNo !== undefined ? { categoryNo: b.categoryNo } : {}),
            board: "댓글",
            status: a.status,
          });
        });
      } else {
        selectedBlogs.forEach((k) => {
          const b = resolvedBlogs.find((x) => blogKey(x.blogId, x.logNo) === k);
          if (!b) return;
          jobs.push({
            key: `${aid}-${k}`,
            platform: "blog",
            loginId: a.loginId,
            targetName: `${b.blogId} · 글 ${b.logNo}`,
            blogId: b.blogId,
            logNo: b.logNo,
            board: "댓글",
            status: a.status,
          });
        });
      }
    } else if (a.platform === "clip") {
      // 클립도 댓글 전용(#클립) — 항상 "최신 N개" 모드. 추가한 창작자(@handle)마다 잡 1개로
      // handle + 개수(clipCount=commentCount) + 탭(전체/영상)을 동결해, 워커가 그 창작자의 최신
      // 미디어 상위 N개에 댓글을 단다(댓글 전 클립 프로필 생성은 백엔드가 보장).
      clipHomes.forEach((c, i) => {
        jobs.push({
          key: `${aid}-cl${i}`,
          platform: "clip",
          loginId: a.loginId,
          targetName: `@${c.handle} · 최신 ${commentCount}건`,
          clipHandle: c.handle,
          clipCount: commentCount,
          ...(c.mediaType ? { clipMediaType: c.mediaType } : {}),
          board: "댓글",
          status: a.status,
        });
      });
    }
  });
  // forum은 보통 종목(stockCodes)을 1개 이상 골라야 한다. 단 "특정 게시글" 댓글이면 URL이
  // 곧 대상(종목코드까지 URL에 있음)이라 종목 선택을 면제한다 — 종목 미선택으로 게시가
  // 막히지 않게 한다.
  const targetsOk =
    !selPlatforms.includes("forum") ||
    stockCodes.length > 0 ||
    forumUrlTargets.length > 0;
  // Comment-only mode needs comments and a resolved target. `url`은 특정 글 하나로
  // 풀리고, latest/popular는 그 목록을 읽을 카페(게시판)가 필요하므로 선택한 게시판이
  // 1개 이상이면 준비된 것으로 본다(게시 시점 워커가 상위 N을 추출).
  const listTargetReady = selectedNaver.length > 0 && selectedCafes.length > 0;
  // 밴드만 선택된 댓글 전용(네이버 list 대상이 없음)은 밴드 대상(최신/인기)으로 충분하다.
  // 밴드는 url 미지원이라 latest/popular(isListTarget)일 때만 준비된 것으로 본다.
  const bandOnlyCommentReady =
    selectedNaver.length === 0 &&
    selPlatforms.includes("band") &&
    selectedBands.length > 0 &&
    isListTarget;
  // 블로그(#271/#279) 댓글 대상이 준비됐는지 — 댓글 작성의 commentTarget을 따른다(카페와 동일).
  // url이면 선택한 글이, 최신글/인기글이면 추가한 블로그(링크)가 1개 이상이면 준비된 것으로 본다.
  const blogTargetsReady = isListTarget
    ? blogHomes.length > 0
    : selectedBlogs.length > 0;
  // 블로그는 댓글 전용이라 네이버 list/url 대상이 없어도 블로그만으로 충분하다.
  const blogOnlyCommentReady =
    selectedNaver.length === 0 &&
    selPlatforms.includes("blog") &&
    blogTargetsReady;
  // 클립(#클립)도 댓글 전용·항상 최신 N개 — 추가한 창작자(@handle)가 1개 이상이면 준비된 것으로 본다.
  const clipTargetsReady = clipHomes.length > 0;
  const clipOnlyCommentReady =
    selectedNaver.length === 0 &&
    selPlatforms.includes("clip") &&
    clipTargetsReady;
  // url 모드의 "대상 해석됨"은 카페·종목토론방·밴드 글 중 하나라도 풀린 링크가 있으면 충분하다
  // — 사용자가 넣은 링크가 어느 플랫폼 글인지에 따라 해당 목록에 잡힌다(여러 링크/혼합 허용).
  const urlTargetResolved =
    urlTargets.length > 0 ||
    forumUrlTargets.length > 0 ||
    bandUrlTargets.length > 0;
  const commentReady =
    mode !== "comment" ||
    (comments.length > 0 &&
      ((isListTarget ? listTargetReady : urlTargetResolved) ||
        bandOnlyCommentReady ||
        blogOnlyCommentReady ||
        clipOnlyCommentReady));
  // 네이버 카페가 선택됐으면 게시할 게시판을 1개 이상 골라야 한다(밴드와 동일). url 댓글
  // 대상은 게시판이 필요 없어 면제한다. 게시판 미선택이면 게시를 막아 조용한 누락을 막는다.
  const naverReady =
    !selPlatforms.includes("naver") ||
    (mode === "comment" && commentTargetMode === "url") ||
    selectedCafes.length > 0;
  // 밴드가 선택됐으면 게시할 밴드를 1개 이상 골라야 게시 가능(사수 요구 흐름).
  // 예약(schedule)도 이제 밴드를 plan에 싣으므로 면제하지 않는다 — 즉시·예약 공통으로
  // 밴드 플랫폼이 선택됐다면 최소 1개의 밴드를 골라야 게시할 수 있다.
  // 밴드 "특정 게시글" 댓글은 URL이 곧 대상이라 '밴드 선택'을 면제한다(forum 종목 면제와 동일).
  const bandReady =
    !selPlatforms.includes("band") ||
    bandUrlTargets.length > 0 ||
    selectedBands.length > 0;
  // 블로그(#271)가 선택됐으면 게시할 블로그 글을 1개 이상 골라야 게시 가능(카페 게시판 미러).
  // 글을 추가하고 선택해야 게시할 수 있다 — 미선택이면 게시를 막아 조용한 누락을 방지한다.
  const blogReady = !selPlatforms.includes("blog") || blogTargetsReady;
  // 클립(#클립)이 선택됐으면 창작자(@handle)를 1개 이상 추가해야 게시 가능.
  const clipReady = !selPlatforms.includes("clip") || clipTargetsReady;
  const canPublish =
    usableSelected.length > 0 &&
    targetsOk &&
    commentReady &&
    naverReady &&
    bandReady &&
    blogReady &&
    clipReady &&
    jobs.length > 0;

  // 종목토론방(forum) 계정들의 로그인 ID(중복 제거, 선택 순서 유지). "나눠서 게시"의 분배 단위.
  const forumLoginIds = [
    ...new Set(
      jobs
        .filter((j) => j.platform === "forum" && j.code && !j.commentUrl)
        .map((j) => j.loginId),
    ),
  ];
  // "나눠서 게시" 활성 조건(#267-5): forum 계정 2개 이상 + 선택 종목 2개 이상 + 종목수 ≥ 계정수.
  // (계정 1개에 여러 종목 / 여러 계정에 1종목 / 계정수 > 종목수면 비활성 — 균등 분배가 불가하거나
  //  분배 의미가 없는 경우.)
  const canDistribute =
    canPublish &&
    forumLoginIds.length > 1 &&
    stockCodes.length > 1 &&
    stockCodes.length >= forumLoginIds.length;

  // "나눠서 게시"(댓글 분배, #403): 종토 "특정 게시글"(url 댓글) 대상 계정들 — 이 맥락 전용.
  const forumUrlLoginIds = [
    ...new Set(
      jobs
        .filter((j) => j.platform === "forum" && !!j.commentUrl)
        .map((j) => j.loginId),
    ),
  ];
  // 특정글+댓글+저장 맥락에서만 노출: 댓글(또는 글+댓글) 모드 + url 대상 + 종토 특정글 계정 + 댓글 존재.
  const showCommentDistribute =
    (mode === "comment" || mode === "both") &&
    commentTargetMode === "url" &&
    forumUrlLoginIds.length > 0 &&
    comments.length > 0;
  // 활성: 작성 댓글 수 == 선택 계정 수일 때만(그때만 1:1 겹침 없이 분배). 아니면 회색 안내만.
  const canCommentDistribute =
    canPublish &&
    showCommentDistribute &&
    comments.length === forumUrlLoginIds.length;

  // 선택 종목을 forum 계정별로 균등 분배해(#267-5), 각 계정이 자기 몫의 종목 잡만 갖도록 정상
  // jobs에서 forum 종목 잡을 필터링한다. forum 외(카페/밴드)·forum "특정글 댓글" 잡은 그대로 둔다.
  const distributeForumJobs = (allJobs: PublishJob[]): PublishJob[] => {
    const buckets = distributeStocksEvenly(stockCodes, forumLoginIds.length);
    const allowedByLogin = new Map<string, Set<string>>();
    forumLoginIds.forEach((loginId, i) =>
      allowedByLogin.set(loginId, new Set(buckets[i] ?? [])),
    );
    return allJobs.filter((j) => {
      const isForumStock = j.platform === "forum" && !!j.code && !j.commentUrl;
      if (!isForumStock) return true;
      return allowedByLogin.get(j.loginId)?.has(j.code as string) ?? false;
    });
  };

  const action =
    mode === "comment" ? "댓글" : mode === "both" ? "글+댓글" : "글";

  // Build the backend PostJob for a naver UI job — cafeId/menuId는 잡에 동결된
  // (게시판 링크 파싱) 값이다. boardType은 게시 시점 백엔드가 menuId로 해결하므로
  // 빈 문자열로 둔다(쿠키 없이 게시판 목록을 못 받기 때문).
  const toPostJob = (j: PublishJob): PostJob => {
    return {
      // 백엔드는 loginId(쿠키 파일 키)로 계정을 찾는다.
      accountId: j.loginId,
      cafe: j.cafeId != null ? String(j.cafeId) : j.targetName,
      menuId: j.menuId ?? 0,
      boardType: "",
      subject: doc.title,
      bodyText: htmlToText(doc.body ?? ""),
      tagList: [],
    };
  };

  // 예약 시점에 동결할 댓글 대상 스펙. comment/both 모드일 때만 만든다. url이면
  // 파싱된 cafeId/articleId를, latest/popular면 계정이 고른 cafeId + 상위 N(count)을
  // 박제한다(실제 글 목록 해석은 워커가 게시 시점에 수행). post 전용이면 undefined.
  const commentSpecFor = (j: PublishJob): CommentTargetSpec | undefined => {
    if (mode !== "comment" && mode !== "both") return undefined;
    if (commentTargetMode === "url") {
      // 링크(글)마다 잡 1개라, 그 잡에 동결된 cafeId/articleId를 그대로 쓴다(여러 링크면
      // 잡마다 다른 글). 둘이 없으면(비정상) 대상 미설정으로 둔다.
      if (j.cafeId == null || j.articleId == null) return undefined;
      return { mode: "url", cafeId: j.cafeId, articleId: j.articleId };
    }
    const cafeId = j.cafeId;
    if (cafeId == null) return undefined;
    return { mode: commentTargetMode, count: commentCount, cafeId };
  };

  // 밴드 댓글 전용(comment) 모드의 동결 대상. "특정 게시글"(band 글 URL)이면 mode=url로
  // 동결해 워커가 그 글에 직접 댓글을 달게 한다(post_no는 link에 있음). 그 외엔 기존 글
  // (최신/인기) 대상이다. cafeId/articleId는 밴드에서 쓰지 않으므로 비운다.
  const bandCommentSpecFor = (): CommentTargetSpec | undefined => {
    if (mode !== "comment") return undefined;
    if (bandUrlTargets.length > 0) return { mode: "url" };
    const m = commentTargetMode === "popular" ? "popular" : "latest";
    return { mode: m, count: commentCount };
  };

  // 게시 plan(동결 실행 페이로드): 즉시·예약 게시가 공유한다. 본문은 모달이 이미
  // 평문화한 값을 박제하고, 엔진이 있는 naver/forum/band 대상을 모두 싣는다. naver의
  // cafe/menuId는 잡에 동결된(게시판 링크 파싱) 값이고, band 링크는 잡에서 가져온다.
  // jobs를 인자로 받아 plan을 만든다(#267-5: 분배 게시는 분배된 jobs로 호출). 정상 게시는
  // 화면의 jobs(비분배)를 그대로 넘긴다.
  const buildPlanFromJobs = (
    jobs: PublishJob[],
    opts?: { forumCommentDistribute?: boolean },
  ): PublishPlan => {
    const naver: NaverTarget[] = jobs
      .filter((j) => j.platform === "naver")
      .map((j) => {
        const pj = toPostJob(j);
        const spec = commentSpecFor(j);
        return {
          accountId: pj.accountId,
          cafe: pj.cafe,
          // 완료 로그에 보여줄 표시 이름(동결). 카페명은 쿠키 없이 못 받아 대상명을 쓴다.
          cafeName: j.targetName,
          menuId: pj.menuId,
          boardType: pj.boardType,
          ...(spec ? { commentTarget: spec } : {}),
        };
      });
    const forum: ForumTarget[] = jobs
      .filter((j) => j.platform === "forum")
      .map((j) => ({
        accountId: j.loginId,
        name: j.targetName,
        code: j.code ?? "",
        // "특정 게시글" 댓글 잡이면 그 글 URL을 동결해 워커가 랜덤 글이 아니라 이 글에
        // 댓글을 달게 한다. 일반 종목 게시 잡은 빈 문자열(기존 per-종목 동작).
        commentUrl: j.commentUrl ?? "",
      }));
    const bandSpec = bandCommentSpecFor();
    // url 모드인데 붙여넣은 URL이 밴드 글 URL이 아니면(카페·종토방 글) 밴드 대상을 plan에
    // 싣지 않는다 — 안 그러면 엉뚱한 최신글에 댓글이 달린다. 밴드 글 URL이면(bandUrlTarget)
    // 그 글에 직접 댓글을 달 수 있으므로 정상적으로 싣는다(forum/카페 url 대상도 그대로).
    const bandUrlUnsupported =
      mode === "comment" &&
      commentTargetMode === "url" &&
      bandUrlTargets.length === 0;
    const band: BandTarget[] = bandUrlUnsupported
      ? []
      : jobs
          .filter((j) => j.platform === "band")
          .map((j) => ({
            accountId: j.loginId,
            name: j.targetName,
            // 잡 생성 시 동결한 링크를 그대로 싣는다(밴드명 재조회 없음).
            link: j.bandLink ?? "",
            // 댓글 전용 모드면 기존 글(최신/인기) 대상을 동결한다(없으면 워커가 새 글을 쓰는
            // band_publish로 가 리더 승인제 밴드에서 1003이 난다).
            ...(bandSpec ? { commentTarget: bandSpec } : {}),
          }));
    // 블로그(#271): 댓글 전용 — 잡에 동결된 blogId+logNo로 그 글에 댓글을 단다. 카페와 같은
    // 네이버 쿠키를 재사용하므로 별도 로그인이 필요 없다(백엔드가 저장 쿠키를 그대로 쓴다).
    const blog: BlogTarget[] = jobs
      .filter((j) => j.platform === "blog")
      .map((j) => {
        // "최신 N개" 모드(#279)면 count(+categoryNo)를 동결하고 logNo는 비운다 — 워커가 최신 글
        // 상위 N개를 조회해 댓글을 단다. 링크는 글 URL 대신 블로그 홈으로 둔다(글이 아직 미정).
        const isLatest =
          j.blogCount != null && (j.logNo == null || j.logNo === "");
        return {
          accountId: j.loginId,
          name: j.targetName,
          blogId: j.blogId ?? "",
          logNo: j.logNo ?? "",
          link: isLatest
            ? `https://blog.naver.com/${j.blogId ?? ""}`
            : `https://blog.naver.com/${j.blogId ?? ""}/${j.logNo ?? ""}`,
          ...(isLatest ? { count: j.blogCount } : {}),
          ...(isLatest && j.categoryNo != null
            ? { categoryNo: j.categoryNo }
            : {}),
        };
      });
    // 클립(#클립): 항상 "최신 N개" — handle + count(+탭)을 동결한다. 워커가 그 창작자의 최신
    // 미디어 상위 N개에 댓글을 단다(댓글 전 클립 프로필 생성은 백엔드가 보장).
    const clip: ClipTarget[] = jobs
      .filter((j) => j.platform === "clip")
      .map((j) => ({
        accountId: j.loginId,
        name: j.targetName,
        handle: j.clipHandle ?? "",
        link: `https://clip.naver.com/@${j.clipHandle ?? ""}`,
        ...(j.clipCount != null ? { count: j.clipCount } : {}),
        ...(j.clipMediaType ? { mediaType: j.clipMediaType } : {}),
      }));
    // 게시 대상 계정마다 로그인 스펙을 동봉한다(#225). 워커가 게시 직전에 계정 단위로
    // [IP 회전 → 로그인 → 게시]를 원자 실행해 "로그인 IP == 게시 IP"를 맞춘다 — 그래야
    // 네이버 카페 10004(IP check failure)를 피한다. force/useAdb는 기존 선택 로그인과 동일.
    const loginByAccount = new Map<string, LoginTarget>();
    jobs.forEach((j) => {
      const platform = j.platform === "band" ? "band" : "naver";
      const key = `${j.loginId}::${platform}`;
      if (loginByAccount.has(key)) return;
      loginByAccount.set(key, {
        accountId: j.loginId,
        platform,
        headless: false,
        useAdb: true,
        force: true,
      });
    });
    return {
      postId: doc.id,
      kind: doc.kind,
      title: doc.title,
      bodyText: htmlToText(doc.body ?? ""),
      comments: doc.comments ?? [],
      // 백엔드가 대상별로 #{링크}를 치환할 때 쓴다. 비우면 종목별 시세 링크.
      linkOverride: linkOverride.trim(),
      naver,
      forum,
      band,
      blog,
      clip,
      login: [...loginByAccount.values()],
      // "나눠서 게시"(#403): 종토 특정글 댓글을 계정에 1:1 분배(백엔드가 링크마다 배정). 기본 false.
      forumCommentDistribute: opts?.forumCommentDistribute ?? false,
    };
  };

  // 게시 대상 계정의 자격증명(id/pw)을 accounts.json에 저장한다(#225). 선택 로그인을
  // 없앴으므로, 백엔드가 게시 직전 로그인하려면 자격증명이 미리 저장돼 있어야 한다(기존
  // runLogin이 로그인 전에 하던 일을 게시 흐름으로 옮긴 것). pw 없는 계정은 건너뛴다.
  const persistCredentials = async (jobs: PublishJob[]) => {
    const seen = new Set<string>();
    const creds = jobs
      .map((j) => accounts.find((a) => a.loginId === j.loginId))
      .filter((a): a is Account => !!a && !!a.loginId.trim() && !!a.pw)
      .filter((a) =>
        seen.has(a.loginId) ? false : (seen.add(a.loginId), true),
      )
      .map((a) => ({ id: a.loginId, password: a.pw, label: a.loginId }));
    if (creds.length === 0) return;
    await ipc.auth.bootstrap();
    await ipc.auth.saveAccounts(creds);
  };

  // jobs와 when을 인자로 받아 즉시/예약 큐에 적재한다(#267-5). 정상 게시는 dispatchPublish(jobs,
  // when)으로, "나눠서 게시"는 분배된 jobs로 호출한다 — 디스패치 로직은 완전히 동일하게 재사용한다.
  const dispatchPublish = (
    jobs: PublishJob[],
    when: "now" | "schedule",
    opts?: { forumCommentDistribute?: boolean },
  ) => {
    // 게시 위치(표시용 locs)는 즉시·예약 공통이다. 같은 플랫폼·대상·코드는 한 번만 싣는다.
    const seen = new Set<string>();
    const locs: QueueLocation[] = [];
    jobs.forEach((j) => {
      const key = `${j.platform}|${j.targetName}|${j.code ?? ""}`;
      if (seen.has(key)) return;
      seen.add(key);
      locs.push({
        p: j.platform,
        name: j.targetName,
        ...(j.code ? { code: j.code } : {}),
      });
    });

    if (when !== "schedule") {
      // 즉시 게시("지금 바로")도 게시 큐의 즉시 처리 대기열(now 큐)에 적재한다(#198).
      // 워커가 곧바로 집어 카페/종목토론방/밴드 게시와 완료 로그를 예약 게시와 동일
      // 경로로 처리한다 — 진행률·취소·완료 로그가 예약 게시와 일관된다.
      const item: QueueNowItem = {
        id: newNowId(),
        title: doc.title,
        kind: doc.kind,
        state: "waiting",
        locs,
        // 대상별 라이브 상태는 워커가 채운다(적재 시점엔 빈 배열).
        items: [],
        plan: buildPlanFromJobs(jobs, opts),
      };
      setFlow("running");
      void persistCredentials(jobs)
        .then(() => ipc.queue.addNow(item))
        .then(() => {
          // 큐 적재 성공 → 방금 쓴 계정을 그 자리에서 목록·선택에서 제거한다(#4).
          removeSubmittedAccounts(jobs);
          setFlow(
            jobs.map((j) => ({
              ...j,
              ok: true,
              msg: `${action} 즉시 처리 대기열에 추가됨`,
            })),
          );
        })
        .catch(() =>
          setFlow(
            jobs.map((j) => ({
              ...j,
              ok: false,
              msg: "대기열 추가 실패 — 잠시 후 다시 시도하세요",
            })),
          ),
        );
      return;
    }

    // 예약: scheduled 큐에 추가해 "예약 대기"에 뜨게 한다.
    const moment = scheduleMoment(date, time);
    const item: QueueScheduledItem = {
      id: newScheduledId(),
      title: doc.title,
      kind: doc.kind,
      when: moment.when,
      rel: moment.label,
      at: toEpochMs(date, time),
      missed: false,
      locs,
      plan: buildPlanFromJobs(jobs, opts),
    };
    // Defense-in-depth: the backend rejects a past time even though the picker
    // already prevents it. 예약도 게시 시점에 백엔드가 로그인하므로 자격증명을 먼저 저장한다.
    void persistCredentials(jobs)
      .then(() => ipc.queue.addScheduled(item, toEpochMs(date, time)))
      .then(() => {
        // 예약도 큐에 적재되면 방금 쓴 계정을 그 자리에서 목록·선택에서 제거한다(#4).
        removeSubmittedAccounts(jobs);
        setFlow(
          jobs.map((j) => ({ ...j, ok: true, msg: `${action} 예약 완료` })),
        );
      })
      .catch(() =>
        notifications.show({
          message: "예약 시각이 현재보다 과거예요. 시간을 다시 선택하세요.",
          color: "red",
        }),
      );
  };

  // "나눠서 즉시 게시"(계정 1개당 큐 1개): 분배된 jobs를 loginId로 묶어 계정마다 별도 now
  // 아이템을 적재한다. dispatchPublish(1큐에 전 계정)와 달리 1큐=1계정이라, 워커가 계정별로
  // 독립 실행하고 종목만 균등 분배된다. 디스패치 외 로직은 dispatchPublish "now" 분기와 동일.
  const dispatchSplitNow = (jobs: PublishJob[]) => {
    const order = [...new Set(jobs.map((j) => j.loginId))];
    const groups = order.map((login) =>
      jobs.filter((j) => j.loginId === login),
    );
    const locsFor = (gjobs: PublishJob[]): QueueLocation[] => {
      const seen = new Set<string>();
      const locs: QueueLocation[] = [];
      gjobs.forEach((j) => {
        const key = `${j.platform}|${j.targetName}|${j.code ?? ""}`;
        if (seen.has(key)) return;
        seen.add(key);
        locs.push({
          p: j.platform,
          name: j.targetName,
          ...(j.code ? { code: j.code } : {}),
        });
      });
      return locs;
    };
    // 계정마다 1개씩, 충돌 불가능한 ID를 미리 만든다(같은 tick이라도 freshIdSuffix가 고유 보장).
    const itemIds = groups.map(() => newNowId());
    // [SPLIT] 진단: 선택 계정 수·계정별 종목 분배 결과·생성된 큐 ID를 남긴다 — "7계정 선택했는데
    // 6개만 받고 1개 증발"이 다시 나면 로그에서 즉시 원인(ID 중복/빈 버킷)을 가릴 수 있게 한다(#6).
    console.info(
      "[SPLIT] distinctLogins=%d perAccountStocks=%o itemIds=%o",
      order.length,
      groups.map((g) => g.length),
      itemIds,
    );
    setFlow("running");
    void persistCredentials(jobs)
      .then(() =>
        Promise.all(
          groups.map((gjobs, i) =>
            ipc.queue.addNow({
              id: itemIds[i]!,
              title: doc.title,
              kind: doc.kind,
              state: "waiting",
              locs: locsFor(gjobs),
              items: [],
              plan: buildPlanFromJobs(gjobs),
            }),
          ),
        ),
      )
      .then(() => {
        // 나눠서 게시도 각 계정 큐 적재 성공 후 그 자리에서 계정을 목록·선택에서 제거한다(#4).
        removeSubmittedAccounts(jobs);
        setFlow(
          jobs.map((j) => ({
            ...j,
            ok: true,
            msg: `${action} 즉시 처리 대기열에 추가됨`,
          })),
        );
      })
      .catch(() =>
        setFlow(
          jobs.map((j) => ({
            ...j,
            ok: false,
            msg: "대기열 추가 실패 — 잠시 후 다시 시도하세요",
          })),
        ),
      );
  };

  const kd = KIND[mode] ?? { t: mode, c: "gray" };
  const allText = [doc.title, doc.body ?? "", ...(doc.comments ?? [])].join(
    " ",
  );
  const hasNameTok = hasToken(allText, "stock");
  const hasCodeTok = hasToken(allText, "code");
  const hasLinkTok = hasToken(allText, "link");
  const showTokens = hasNameTok || hasCodeTok || hasLinkTok;
  const exampleText =
    doc.title || (doc.comments ?? []).find(Boolean) || doc.body || "";

  const tokenChip = (label: string) => (
    <Text
      fz={11.5}
      fw={800}
      ff="monospace"
      c="blue"
      px={8}
      py={2}
      style={{
        background: "var(--mantine-color-body)",
        border: "1px solid var(--mantine-color-blue-filled)",
        borderRadius: 5,
      }}
    >
      {label}
    </Text>
  );

  const timing: {
    v: "now" | "schedule";
    t: string;
    s: string;
    ic: React.ReactNode;
  }[] = [
    {
      v: "now",
      t: "지금 바로 게시",
      s: "대기열에 추가돼 즉시 처리",
      ic: <Icon.bolt size={16} />,
    },
    {
      v: "schedule",
      t: "예약 게시",
      s: "원하는 시간에 자동 업로드",
      ic: <Icon.calendar size={16} />,
    },
  ];

  return (
    <Modal
      opened={open}
      onClose={onClose}
      withCloseButton={false}
      padding={0}
      size={560}
      radius="lg"
      styles={{
        body: { display: "flex", flexDirection: "column", maxHeight: "85vh" },
      }}
    >
      {/* header */}
      <Group
        px={18}
        py={16}
        gap={12}
        wrap="nowrap"
        style={{
          flexShrink: 0,
          borderBottom: "1px solid var(--mantine-color-gray-2)",
        }}
      >
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Text fz={15.5} fw={800}>
            게시 설정
          </Text>
          <Group gap={7} mt={4} wrap="nowrap">
            <Badge size="sm" color={kd.c} variant="light">
              {kd.t}
            </Badge>
            <Text fz={12.5} c="dimmed" truncate>
              {doc.title}
            </Text>
          </Group>
        </Box>
        <ActionIcon size={34} variant="subtle" color="gray" onClick={onClose}>
          <Icon.x size={19} />
        </ActionIcon>
      </Group>

      {/* scroll body */}
      <Box style={{ flex: 1, minHeight: 0, overflowY: "auto" }} px={20} py={18}>
        <Group gap={10} mb={10}>
          <Group gap={7}>
            <Icon.users size={17} color="var(--mantine-color-gray-6)" />
            <Text fz={13.5} fw={700}>
              게시 계정
            </Text>
          </Group>
          <Text fz={12} fw={700} c="blue">
            {selected.length}개
          </Text>
          <Button
            size="compact-xs"
            variant="subtle"
            ml="auto"
            onClick={selectAllVisible}
          >
            {allVisibleOn ? "전체 해제" : "보이는 계정 전체"}
          </Button>
        </Group>
        <SegmentedControl
          fullWidth
          size="xs"
          value={acctFilter}
          onChange={(v) => setAcctFilter(v as "all" | PlatformId)}
          data={acctFilters}
        />
        <Box
          mt={10}
          p={5}
          style={{
            border: "1px solid var(--mantine-color-gray-2)",
            borderRadius: "var(--mantine-radius-md)",
            maxHeight: 198,
            overflowY: "auto",
          }}
        >
          {visibleAccts.map((a) => (
            <AccountRow
              key={a.id}
              a={a}
              selected={selected.includes(a.id)}
              onToggle={toggle}
            />
          ))}
          {visibleAccts.length === 0 && (
            <Text ta="center" py={22} fz={12.5} c="gray.5">
              해당 계정이 없어요
            </Text>
          )}
        </Box>

        {selPlatforms.length > 0 && (
          <>
            <Group gap={7} mt={22} mb={12}>
              <Icon.target size={17} color="var(--mantine-color-gray-6)" />
              <Text fz={13.5} fw={700}>
                게시 위치
              </Text>
            </Group>
            <DestinationPicker
              selPlatforms={selPlatforms}
              stockCodes={stockCodes}
              stockNames={stockNames}
              openStockModal={() => setStockModal(true)}
              removeStock={(c) =>
                setStockCodes((s) => s.filter((x) => x !== c))
              }
              bandLink={bandLink}
              setBandLink={setBandLink}
              bandResolving={bandResolving}
              resolvedBands={resolvedBands}
              selectedBands={selectedBands}
              onSaveBandLink={saveBandLink}
              onSelectBand={selectBand}
              onRemoveBand={removeBand}
              stocks={stocks}
              cafeLink={cafeLink}
              setCafeLink={setCafeLink}
              resolvedCafes={resolvedCafes}
              selectedCafes={selectedCafes}
              onSaveCafeLink={saveCafeLink}
              onSelectCafe={selectCafe}
              onRemoveCafe={removeCafe}
              blogLink={blogLink}
              setBlogLink={setBlogLink}
              resolvedBlogs={resolvedBlogs}
              selectedBlogs={selectedBlogs}
              onSaveBlogLink={saveBlogLink}
              onSelectBlog={selectBlog}
              onRemoveBlog={removeBlog}
              blogIsListTarget={isListTarget}
              blogHomes={blogHomes}
              blogHomeLink={blogHomeLink}
              setBlogHomeLink={setBlogHomeLink}
              onSaveBlogHomeLink={saveBlogHomeLink}
              onRemoveBlogHome={removeBlogHome}
              clipHomes={clipHomes}
              clipHomeLink={clipHomeLink}
              setClipHomeLink={setClipHomeLink}
              onSaveClipHomeLink={saveClipHomeLink}
              onRemoveClipHome={removeClipHome}
            />
          </>
        )}

        {showTokens && (
          <Box
            mt={22}
            p="md"
            style={{
              border: "1px solid var(--mantine-color-blue-filled)",
              background: "var(--mantine-color-blue-light)",
              borderRadius: "var(--mantine-radius-md)",
            }}
          >
            <Group gap={7}>
              <Icon.hash size={15} color="var(--mantine-color-blue-filled)" />
              <Text fz={13} fw={800} c="blue.8">
                변수 자동 치환
              </Text>
            </Group>
            <Group gap={6} mt={9}>
              {hasNameTok && tokenChip("#{종목명}")}
              {hasCodeTok && tokenChip("#{종목코드}")}
              {hasLinkTok && tokenChip("#{링크}")}
              <Text fz={12} c="gray.7">
                가 대상마다 자동으로 채워집니다.
              </Text>
            </Group>
            {hasLinkTok && (
              <Box mt={11}>
                <Text fz={11.5} fw={700} c="gray.7" mb={5}>
                  링크 값{" "}
                  <Text component="span" fw={500} c="gray.5">
                    (선택)
                  </Text>
                </Text>
                <TextInput
                  size="sm"
                  value={linkOverride}
                  onChange={(e) => setLinkOverride(e.currentTarget.value)}
                  placeholder="비우면 종목별 시세 링크 자동 삽입"
                  leftSection={<Icon.link size={14} />}
                />
              </Box>
            )}
            {jobs[0] && (
              <Box
                mt={11}
                pt={10}
                style={{ borderTop: "1px solid rgba(34,139,230,.25)" }}
              >
                <Text fz={11.5} c="dimmed">
                  예시 ·{" "}
                  <Text component="span" fw={700} c="gray.7">
                    {jobs[0].targetName}
                  </Text>
                  :{" "}
                  <Text component="span" c="gray.7">
                    {resolveTemplate(exampleText, jobs[0], linkOverride) || "—"}
                  </Text>
                </Text>
              </Box>
            )}
          </Box>
        )}

        <Group gap={7} mt={22} mb={12}>
          <Icon.clock size={17} color="var(--mantine-color-gray-6)" />
          <Text fz={13.5} fw={700}>
            게시 시점
          </Text>
        </Group>
        <Stack gap={8}>
          {timing.map((o) => {
            const on = when === o.v;
            return (
              <Group
                key={o.v}
                gap={11}
                px={12}
                py={11}
                wrap="nowrap"
                onClick={() => setWhen(o.v)}
                style={{
                  borderRadius: "var(--mantine-radius-md)",
                  border: `1.5px solid ${
                    on
                      ? "var(--mantine-color-blue-filled)"
                      : "var(--mantine-color-gray-2)"
                  }`,
                  background: on
                    ? "var(--mantine-color-blue-light)"
                    : "transparent",
                  cursor: "pointer",
                }}
              >
                <Radio checked={on} readOnly />
                <ThemeIcon
                  variant="transparent"
                  color={on ? "blue" : "gray"}
                  size="sm"
                >
                  {o.ic}
                </ThemeIcon>
                <Box style={{ flex: 1 }}>
                  <Text fz={13.5} fw={700}>
                    {o.t}
                  </Text>
                  <Text fz={11.5} c="dimmed">
                    {o.s}
                  </Text>
                </Box>
              </Group>
            );
          })}
        </Stack>
        {when === "schedule" && (
          <Box mt={10}>
            <DateTimePicker
              date={date}
              time={time}
              onChange={(v) => {
                setDate(v.date);
                setTime(v.time);
              }}
            />
          </Box>
        )}

        {/* 나눠서 게시(#267-5): 여러 계정 + 여러 종목을 균등 분배해 계정별로 서로 다른 종목을
            게시한다. 예약은 위 '예약 게시' 시점(날짜/시간)을 그대로 사용한다. */}
        <Stack gap={8} mt={14}>
          <Button
            variant="light"
            fullWidth
            disabled={!canDistribute}
            leftSection={<Icon.send size={16} />}
            onClick={() => dispatchSplitNow(distributeForumJobs(jobs))}
          >
            나눠서 즉시 게시하기
            {canDistribute
              ? ` (${forumLoginIds.length}계정 · ${stockCodes.length}종목)`
              : ""}
          </Button>
          <Button
            variant="light"
            color="grape"
            fullWidth
            disabled={!canDistribute}
            leftSection={<Icon.calendar size={16} />}
            onClick={() =>
              dispatchPublish(distributeForumJobs(jobs), "schedule")
            }
          >
            나눠서 게시 예약하기
          </Button>
          {!canDistribute &&
            forumLoginIds.length > 1 &&
            stockCodes.length > 0 && (
              <Text fz={11} c="dimmed">
                나눠서 게시는 계정 2개 이상 + 종목 2개 이상이고, 종목 수가 계정
                수 이상일 때 켜집니다.
              </Text>
            )}
        </Stack>

        {/* 나눠서 게시(댓글 분배, #403): 종토 "특정 게시글"+댓글 맥락에서만 노출. #댓글==#계정일 때만
            활성 — 각 링크에서 계정에 서로 다른 댓글 1개씩 무작위 배정. 아니면 회색 카운트 안내. */}
        {showCommentDistribute && (
          <Stack gap={6} mt={14}>
            <Button
              variant="light"
              color="teal"
              fullWidth
              disabled={!canCommentDistribute}
              leftSection={<Icon.send size={16} />}
              onClick={() =>
                dispatchPublish(jobs, "now", { forumCommentDistribute: true })
              }
            >
              나눠서 게시
            </Button>
            {!canCommentDistribute && (
              <Text fz={11.5} c="dimmed" ta="center">
                댓글 : {comments.length}개 &nbsp; 계정 :{" "}
                {forumUrlLoginIds.length}개
              </Text>
            )}
          </Stack>
        )}
      </Box>

      {/* footer */}
      <Group
        px={20}
        py={14}
        gap={10}
        wrap="nowrap"
        style={{
          flexShrink: 0,
          borderTop: "1px solid var(--mantine-color-gray-2)",
        }}
      >
        <Text fz={12} c="gray.5">
          {jobs.length}곳
        </Text>
        <PlatformPill ids={selPlatforms} size={16} />
        {!naverReady && (
          <Text fz={12} c="orange.7">
            게시판 링크를 추가하고 게시할 게시판을 선택해야 게시할 수 있어요
          </Text>
        )}
        {naverReady && !blogReady && (
          <Text fz={12} c="orange.7">
            블로그 링크를 추가하고 게시할 블로그 글을 선택해야 게시할 수 있어요
          </Text>
        )}
        <Box style={{ flex: 1 }} />
        <Button
          size="sm"
          variant="default"
          leftSection={<Icon.eye size={16} />}
          onClick={() => setShowPreview(true)}
        >
          미리보기
        </Button>
        <Button
          size="sm"
          disabled={!canPublish}
          leftSection={
            when === "schedule" ? (
              <Icon.calendar size={18} />
            ) : (
              <Icon.send size={17} />
            )
          }
          onClick={() => dispatchPublish(jobs, when)}
        >
          {when === "schedule"
            ? `예약 (${jobs.length})`
            : `게시 (${jobs.length})`}
        </Button>
      </Group>

      <StockCrawlModal
        open={stockModal}
        preselected={stockCodes}
        onClose={() => setStockModal(false)}
        onConfirm={(stocks) => {
          setStockCodes(stocks.map((s) => s.code));
          setStockNames((m) => ({
            ...m,
            ...Object.fromEntries(stocks.map((s) => [s.code, s.name])),
          }));
          setStockModal(false);
        }}
      />
      <PreviewModal
        open={showPreview}
        onClose={() => setShowPreview(false)}
        mode={mode}
        title={doc.title}
        body={doc.body ?? ""}
        comments={(doc.comments ?? []).filter(Boolean)}
        jobs={jobs}
        linkOverride={linkOverride}
        {...(doc.commentTarget ? { commentTarget: doc.commentTarget } : {})}
        {...(doc.commentCount ? { commentCount: doc.commentCount } : {})}
      />
      <PublishFlow
        state={flow}
        mode={mode}
        when={when}
        date={date}
        time={time}
        count={jobs.length}
        onClose={() => {
          setFlow(null);
          onClose();
        }}
        onKeepWriting={() => {
          // '계속작성'(#5): 게시설정창은 그대로 두고 결과 패널만 닫는다. 계정 목록은 새로고침해
          // 방금 게시한(=#4에서 빠진) 계정이 빠진 목록을 다시 로드한다 — 바로 새 선택을 할 수 있다.
          setFlow(null);
          refreshAccounts();
        }}
        go={go}
      />
    </Modal>
  );
}

export function PublishModal(props: PublishModalProps) {
  // Remount per open / per document so useState initializers re-seed.
  return (
    <PublishModalInner
      key={props.open ? (props.doc?.id ?? "new") : "closed"}
      {...props}
    />
  );
}
