import {
  ActionIcon,
  Badge,
  Button,
  Divider,
  Group,
  Loader,
  Modal,
  NumberInput,
  Stack,
  Text,
  TextInput,
  ThemeIcon,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { useState } from "react";

import { ipc } from "@/shared/ipc";
import type { ViewBoostOutcome } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface ViewCountModalProps {
  open: boolean;
  onClose: () => void;
}

/** 반복 횟수(N)의 허용 범위 — 시크릿창을 실제로 여닫는 부담이 있어 상한을 둔다. */
const MIN_REPEATS = 1;
const MAX_REPEATS = 1000;

/** 게시글 링크에서 사람이 읽을 postId(끝의 숫자)를 뽑는다. 칩 라벨용(없으면 링크 자체). */
function postLabel(url: string): string {
  const m = url.match(/\/discussion\/(\d+)/);
  return m ? `글 #${m[1]}` : url;
}

/** 글 관리 화면의 "조회수" 버튼이 여는 모달.
 *
 * 좋아요 모달과 같은 방식으로 게시글 링크를 여러 개(엔터/추가 → 칩) 넣고, 아래에 공용 "반복
 * 횟수(N)"를 숫자로 정한다. 링크 1개 이상 + N≥1 이면 "조회수" 버튼이 활성화된다. 누르면 백엔드가
 * 각 링크를 **시크릿창으로 N번 순차로 여닫아**(열기→완전로딩→그 창만 종료)
 * 조회수를 올린다. 완료 시 링크별 성공/진행을 보여준다.
 */
export function ViewCountModal({ open, onClose }: ViewCountModalProps) {
  const [links, setLinks] = useState<string[]>([]);
  const [linkInput, setLinkInput] = useState("");
  // Mantine NumberInput은 값을 지우면 ""(빈 문자열)을 준다 — 그 상태를 그대로 담아 "숫자 미입력"을
  // 버튼 비활성으로 반영한다(숫자로 채워야 활성화라는 명세).
  const [repeats, setRepeats] = useState<number | "">(30);
  const [flow, setFlow] = useState<null | "running" | ViewBoostOutcome[]>(null);

  // 입력칸의 링크를 목록에 추가한다(중복 제거, trim). 추가 후 입력칸을 비운다.
  const addLink = () => {
    const link = linkInput.trim();
    if (!link) return;
    setLinks((prev) => (prev.includes(link) ? prev : [...prev, link]));
    setLinkInput("");
  };
  const removeLink = (link: string) =>
    setLinks((prev) => prev.filter((l) => l !== link));

  const running = flow === "running";
  const results = Array.isArray(flow) ? flow : [];
  // 숫자칸이 비었거나(=""), 최소값 미만이면 활성화하지 않는다(링크 1개↑ + 유효 숫자 둘 다 필요).
  const repeatsNum = typeof repeats === "number" ? repeats : NaN;
  const canSubmit = links.length > 0 && repeatsNum >= MIN_REPEATS && !running;

  const submit = async () => {
    if (!canSubmit) return;
    setFlow("running");
    try {
      const outcomes = await ipc.viewCount.boost(links, repeatsNum);
      setFlow(outcomes);
      const ok = outcomes.filter((o) => o.success).length;
      notifications.show({
        message: `조회수 ${outcomes.length}개 링크 중 ${ok}개 완료`,
        color: ok === outcomes.length ? "green" : ok === 0 ? "red" : "yellow",
      });
    } catch (err) {
      notifications.show({
        message:
          "조회수 실패: " + (err instanceof Error ? err.message : String(err)),
        color: "red",
      });
      setFlow(null);
    }
  };

  const close = () => {
    if (running) return;
    setLinks([]);
    setLinkInput("");
    setFlow(null);
    onClose();
  };

  const okCount = results.filter((r) => r.success).length;

  return (
    <Modal
      opened={open}
      onClose={close}
      title={
        <Group gap={8}>
          <ThemeIcon size={26} radius="xl" variant="light" color="teal">
            <Icon.eye size={16} />
          </ThemeIcon>
          <Text fw={800} fz={17}>
            조회수
          </Text>
        </Group>
      }
      size={520}
      radius="lg"
      centered
    >
      <Stack gap={16}>
        <Text fz={13} c="dimmed">
          게시글 링크를 넣고(여러 개 가능) 반복 횟수를 정하면, 각 링크를
          시크릿창으로 그 횟수만큼 여닫아(열기→완전로딩→종료) 조회수를 올립니다.
        </Text>

        {/* 좋아요 모달과 같은 링크 입력 — 엔터/추가 → 칩으로 쌓이고 입력칸 비움. */}
        <Stack gap={8}>
          <Group gap={8} align="flex-end" wrap="nowrap">
            <TextInput
              style={{ flex: 1 }}
              label="게시글 링크"
              placeholder="https://stock.naver.com/domestic/stock/005930/discussion/424274129"
              value={linkInput}
              onChange={(e) => setLinkInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addLink();
                }
              }}
              leftSection={<Icon.link size={14} />}
              aria-label="조회수를 올릴 게시글 링크"
            />
            <Button
              variant="light"
              color="teal"
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
                  color="teal"
                  variant="light"
                  radius="xl"
                  size="lg"
                  rightSection={
                    <ActionIcon
                      size={15}
                      variant="transparent"
                      color="teal"
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
              조회수를 올릴 게시글 링크를 추가하세요.
            </Text>
          )}
        </Stack>

        <Divider label="반복 횟수(링크마다)" labelPosition="left" />

        {/* 각 링크를 시크릿창으로 몇 번 여닫을지. 예: 30이면 링크 하나당 30번 껐다켰다. */}
        <Group gap={10} align="center">
          <NumberInput
            aria-label="반복 횟수"
            value={repeats}
            onChange={(v) => {
              // 비우면 ""(빈 문자열) 그대로 담아 버튼을 비활성으로 만든다. 숫자면 그대로 저장.
              if (v === "" || typeof v === "number") {
                setRepeats(v);
              } else {
                const n = parseInt(v, 10);
                setRepeats(Number.isNaN(n) ? "" : n);
              }
            }}
            min={MIN_REPEATS}
            max={MAX_REPEATS}
            clampBehavior="strict"
            w={110}
            styles={{
              input: { fontFamily: "monospace", textAlign: "center" },
            }}
          />
          <Text fz={12.5} c="dimmed">
            링크 {links.length}개 × {repeatsNum || 0}회 = 총{" "}
            <b>{links.length * (repeatsNum || 0)}</b>번 여닫습니다.
          </Text>
        </Group>

        {/* 결과 패널: 링크별 성공/진행(몇 회 완료)을 보여준다. */}
        {results.length > 0 && (
          <Stack gap={6}>
            <Text fz={13} fw={700}>
              {results.length}개 링크 중 {okCount}개 완료
            </Text>
            <Stack gap={4} style={{ maxHeight: 160, overflowY: "auto" }}>
              {results.map((r, i) => (
                <Group
                  key={`${r.link}-${i}`}
                  gap={8}
                  px={10}
                  py={7}
                  wrap="nowrap"
                  style={{
                    borderRadius: "var(--mantine-radius-sm)",
                    border: "1px solid var(--mantine-color-gray-2)",
                    background: "var(--mantine-color-gray-0)",
                  }}
                >
                  <ThemeIcon
                    size={20}
                    radius="xl"
                    variant="light"
                    color={r.success ? "green" : "red"}
                  >
                    {r.success ? (
                      <Icon.checkCircle size={13} />
                    ) : (
                      <Icon.alert size={13} />
                    )}
                  </ThemeIcon>
                  <Badge size="xs" variant="default" radius="sm">
                    {postLabel(r.link)}
                  </Badge>
                  <Text fz={11.5} c={r.success ? "dimmed" : "red"} truncate>
                    {r.completed}/{r.requested}회 · {r.message}
                  </Text>
                </Group>
              ))}
            </Stack>
          </Stack>
        )}

        <Group justify="flex-end" gap={9}>
          <Button variant="default" onClick={close} disabled={running}>
            닫기
          </Button>
          <Button
            color="teal"
            leftSection={
              running ? (
                <Loader size={14} color="white" />
              ) : (
                <Icon.eye size={16} />
              )
            }
            onClick={submit}
            disabled={!canSubmit}
          >
            {running ? "조회수 올리는 중…" : "조회수"}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
