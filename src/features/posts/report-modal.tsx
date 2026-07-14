import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Checkbox,
  Divider,
  Group,
  Loader,
  Modal,
  Radio,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useEffect, useState } from "react";

import { isPostable } from "@/shared/data/config";
import type { Account } from "@/shared/data/types";
import { ipc, REPORT_REASONS } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";
import { PlatformLogo } from "@/shared/ui/platform-logo";

import { AccountRow } from "./publish-modal";

export interface ReportModalProps {
  open: boolean;
  onClose: () => void;
}

/** 게시글 링크에서 사람이 읽을 postId(끝의 숫자)를 뽑는다. 칩 라벨용(없으면 링크 자체). */
function postLabel(url: string): string {
  const m = url.match(/\/discussion\/(\d+)/);
  return m ? `글 #${m[1]}` : url;
}

/** 사유 라디오의 기본 선택 — 실측 7개 중 첫 번째(없으면 빈 문자열, noUncheckedIndexedAccess 가드). */
const DEFAULT_REASON = REPORT_REASONS[0]?.code ?? "";

/** 글 관리 화면의 "신고하기" 버튼이 여는 모달(설계서 naver-report-design.md).
 *
 * 좋아요 모달과 같은 방식으로 게시글 링크를 여러 개(엔터/추가 → 칩) 넣고, 신고 사유(라디오 7개)를
 * 고른 뒤 로그인된 종목토론방 계정을 체크박스([`AccountRow`] 재사용)로 고른다. "IP 회전"을 켜면
 * 계정 사이에 ADB로 IP를 돌리고 새 IP에서 재로그인한다. "신고하기"를 누르면 **모달이 닫히고**
 * 백그라운드로 n×m건이 신고된다(비차단 — 화면 안 막힘). 결과는 알림으로 전달된다.
 */
export function ReportModal({ open, onClose }: ReportModalProps) {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [links, setLinks] = useState<string[]>([]);
  const [linkInput, setLinkInput] = useState("");
  const [reasonCode, setReasonCode] = useState(DEFAULT_REASON);
  const [rotateIp, setRotateIp] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!open) return;
    void ipc.accounts.list().then(setAccounts);
  }, [open]);

  // 입력칸의 링크를 목록에 추가한다(중복 제거, trim). 추가 후 입력칸을 비운다.
  const addLink = () => {
    const link = linkInput.trim();
    if (!link) return;
    setLinks((prev) => (prev.includes(link) ? prev : [...prev, link]));
    setLinkInput("");
  };
  const removeLink = (link: string) =>
    setLinks((prev) => prev.filter((l) => l !== link));

  // 신고는 종목토론방(네이버 증권) 로그인 계정으로만 한다 — 게시 가능한 상태(active/new)만.
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

  const canSubmit =
    links.length > 0 &&
    selectedLoginIds.length > 0 &&
    reasonCode.length > 0 &&
    !submitting;

  const submit = async () => {
    if (!canSubmit) return;
    setSubmitting(true);
    try {
      // 비차단: 커맨드는 즉시 반환한다(백엔드가 백그라운드로 n×m건 신고). 모달을 닫고 안내한다.
      await ipc.report.submit(links, selectedLoginIds, reasonCode, rotateIp);
      notifications.show({
        message: `신고를 시작했습니다 — 링크 ${links.length}개 × 계정 ${selectedLoginIds.length}개(백그라운드 진행)`,
        color: "blue",
      });
      close();
    } catch (err) {
      notifications.show({
        message:
          "신고 시작 실패: " +
          (err instanceof Error ? err.message : String(err)),
        color: "red",
      });
    } finally {
      setSubmitting(false);
    }
  };

  const close = () => {
    setLinks([]);
    setLinkInput("");
    setSelected([]);
    setReasonCode(DEFAULT_REASON);
    setRotateIp(false);
    onClose();
  };

  return (
    <Modal
      opened={open}
      onClose={close}
      title={
        <Group gap={8}>
          <ThemeIcon size={26} radius="xl" variant="light" color="red">
            <Icon.alert size={16} />
          </ThemeIcon>
          <Text fw={800} fz={17}>
            신고하기
          </Text>
        </Group>
      }
      size={520}
      radius="lg"
      centered
    >
      <Stack gap={16}>
        <Text fz={13} c="dimmed">
          신고할 게시글 링크를 넣고(여러 개 가능) 사유와 계정을 고르면, 선택한
          계정들이 그 글들을 신고합니다(백그라운드 진행 — 화면은 막히지 않습니다).
        </Text>

        {/* 링크 입력 — 엔터/추가 → 칩으로 쌓이고 입력칸 비움. */}
        <Stack gap={8}>
          <Group gap={8} align="flex-end" wrap="nowrap">
            <TextInput
              style={{ flex: 1 }}
              label="게시글 링크"
              placeholder="https://stock.naver.com/domestic/stock/000660/discussion/425406371"
              value={linkInput}
              onChange={(e) => setLinkInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addLink();
                }
              }}
              leftSection={<Icon.link size={14} />}
              aria-label="신고할 게시글 링크"
            />
            <Button
              variant="light"
              color="red"
              onClick={addLink}
              disabled={!linkInput.trim()}
            >
              추가
            </Button>
          </Group>
          {links.length > 0 ? (
            <Group gap={6}>
              {links.map((link) => (
                <Badge
                  key={link}
                  color="red"
                  variant="light"
                  radius="xl"
                  size="lg"
                  rightSection={
                    <ActionIcon
                      size={15}
                      variant="transparent"
                      color="red"
                      aria-label={`${link} 제거`}
                      onClick={() => removeLink(link)}
                    >
                      <Icon.x size={11} />
                    </ActionIcon>
                  }
                >
                  {postLabel(link)}
                </Badge>
              ))}
            </Group>
          ) : (
            <Text fz={12} c="orange.7">
              신고할 게시글 링크를 추가하세요.
            </Text>
          )}
        </Stack>

        <Divider label="신고 사유" labelPosition="left" />

        {/* 신고 사유 라디오(설계서 §2.4 실측 7개). 하나만 선택. */}
        <Radio.Group
          value={reasonCode}
          onChange={setReasonCode}
          aria-label="신고 사유"
        >
          <Stack gap={8}>
            {REPORT_REASONS.map((reason) => (
              <Radio
                key={reason.code}
                value={reason.code}
                label={reason.label}
                color="red"
                size="sm"
              />
            ))}
          </Stack>
        </Radio.Group>

        <Divider label="신고할 계정" labelPosition="left" />

        {/* 종목선택 화면과 같은 계정 체크박스(AccountRow) 재사용. */}
        <Stack gap={4}>
          {forumAccounts.length === 0 ? (
            <Group gap={8} px={4} py={10}>
              <PlatformLogo id="forum" size={22} />
              <Text fz={13} c="orange.7">
                신고할 수 있는 종목토론방 로그인 계정이 없습니다. 먼저
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

        {/* IP 회전 — 켜면 계정 사이에 ADB로 IP를 돌리고 새 IP에서 재로그인한다(설계서 §6). */}
        <Checkbox
          checked={rotateIp}
          onChange={(e) => setRotateIp(e.currentTarget.checked)}
          color="red"
          label="IP 회전 (계정마다 ADB로 IP를 바꾸고 새 IP에서 재로그인)"
          description="연결된 폰이 없으면 회전을 건너뛰고 현재 IP로 진행합니다."
        />

        <Group justify="flex-end" gap={9}>
          <Button variant="default" onClick={close} disabled={submitting}>
            닫기
          </Button>
          <Button
            color="red"
            leftSection={
              submitting ? (
                <Loader size={14} color="white" />
              ) : (
                <Icon.alert size={16} />
              )
            }
            onClick={submit}
            disabled={!canSubmit}
          >
            {submitting ? "신고 시작 중…" : "신고하기"}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
