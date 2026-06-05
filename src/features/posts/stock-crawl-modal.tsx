import {
  Box,
  Button,
  Checkbox,
  Group,
  Loader,
  Modal,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { useEffect, useState } from "react";

import type { Stock } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface StockCrawlModalProps {
  open: boolean;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: Stock[]) => void;
}

function StockCrawlModalInner({
  open,
  preselected,
  onClose,
  onConfirm,
}: StockCrawlModalProps) {
  const [phase, setPhase] = useState<"crawling" | "done">("crawling");
  const [q, setQ] = useState("");
  const [sel, setSel] = useState<string[]>(preselected);
  const [found, setFound] = useState(0);
  const [nonce, setNonce] = useState(0);
  const [stocks, setStocks] = useState<Stock[]>([]);

  useEffect(() => {
    void ipc.stocks.list().then(setStocks);
  }, []);

  useEffect(() => {
    let n = 0;
    const iv = window.setInterval(() => {
      n += Math.ceil(Math.random() * 3);
      setFound(Math.min(n, stocks.length));
    }, 120);
    const to = window.setTimeout(() => {
      window.clearInterval(iv);
      setFound(stocks.length);
      setPhase("done");
      void ipc.activity.append("info", `종목 ${stocks.length}개 크롤링`);
    }, 1400);
    return () => {
      window.clearInterval(iv);
      window.clearTimeout(to);
    };
  }, [nonce, stocks.length]);

  const recrawl = () => {
    setPhase("crawling");
    setFound(0);
    setNonce((x) => x + 1);
  };
  const toggle = (code: string) =>
    setSel((s) =>
      s.includes(code) ? s.filter((x) => x !== code) : [...s, code],
    );

  const list = stocks.filter(
    (s) => !q || s.name.includes(q) || s.code.includes(q),
  );

  return (
    <Modal
      opened={open}
      onClose={onClose}
      title="종목토론방 선택"
      size={580}
      radius="lg"
    >
      <Group
        gap={9}
        p="sm"
        mb={14}
        wrap="nowrap"
        style={{
          background: "var(--mantine-color-forum-light)",
          borderRadius: "var(--mantine-radius-md)",
        }}
      >
        <Icon.globe size={18} color="var(--mantine-color-forum-filled)" />
        <Text fz={12.5} fw={600} c="gray.7" style={{ flex: 1 }}>
          finance.naver.com 에서 종목토론방을 불러옵니다.
        </Text>
        <Button
          size="compact-xs"
          variant="light"
          color="forum"
          leftSection={<Icon.refresh size={14} />}
          onClick={recrawl}
        >
          다시 크롤링
        </Button>
      </Group>

      {phase === "crawling" ? (
        <Stack align="center" py={44} gap={6}>
          <Loader size="md" color="forum" />
          <Text fz={14} fw={700} mt={8}>
            종목토론방을 수집하는 중…
          </Text>
          <Text fz={12.5} c="dimmed">
            {found}개 발견
          </Text>
        </Stack>
      ) : (
        <>
          <TextInput
            mb={10}
            value={q}
            onChange={(e) => setQ(e.currentTarget.value)}
            placeholder="종목명 또는 코드 검색"
            leftSection={<Icon.search size={16} />}
          />
          <Box
            style={{
              border: "1px solid var(--mantine-color-gray-2)",
              borderRadius: "var(--mantine-radius-md)",
              overflow: "hidden",
              maxHeight: 340,
              overflowY: "auto",
            }}
          >
            {list.map((s) => {
              const checked = sel.includes(s.code);
              const up = s.chg >= 0;
              return (
                <Group
                  key={s.code}
                  gap={11}
                  px={13}
                  py={10}
                  wrap="nowrap"
                  onClick={() => toggle(s.code)}
                  style={{
                    borderBottom: "1px solid var(--mantine-color-gray-2)",
                    cursor: "pointer",
                    background: checked
                      ? "var(--mantine-color-blue-light)"
                      : "transparent",
                  }}
                >
                  <Checkbox checked={checked} readOnly size="sm" />
                  <Box style={{ flex: 1, minWidth: 0 }}>
                    <Group gap={7} wrap="nowrap">
                      <Text fz={14} fw={700}>
                        {s.name}
                      </Text>
                      <Text fz={10.5} fw={700} c="dimmed" ff="monospace">
                        {s.code}
                      </Text>
                      <Text
                        fz={10}
                        fw={700}
                        c="dimmed"
                        px={4}
                        style={{
                          border: "1px solid var(--mantine-color-gray-3)",
                          borderRadius: 4,
                        }}
                      >
                        {s.market}
                      </Text>
                    </Group>
                    <Text fz={11.5} c="dimmed" mt={2}>
                      게시글 {s.posts}개
                    </Text>
                  </Box>
                  <Box ta="right">
                    <Text fz={13} fw={700} c="gray.7" ff="monospace">
                      {s.price}
                    </Text>
                    <Text fz={11.5} fw={700} c={up ? "red" : "blue"}>
                      {up ? "▲" : "▼"} {Math.abs(s.chg)}%
                    </Text>
                  </Box>
                </Group>
              );
            })}
          </Box>
        </>
      )}

      <Group mt={16} gap={10}>
        <Text fz={12.5} c="dimmed">
          {sel.length}개 종목 선택됨
        </Text>
        <Box style={{ flex: 1 }} />
        <Button size="sm" variant="default" onClick={onClose}>
          취소
        </Button>
        <Button
          size="sm"
          disabled={!sel.length}
          leftSection={<Icon.check size={16} />}
          onClick={() =>
            onConfirm(
              sel
                .map((c) => stocks.find((s) => s.code === c))
                .filter((s): s is Stock => !!s),
            )
          }
        >
          적용 ({sel.length})
        </Button>
      </Group>
    </Modal>
  );
}

export function StockCrawlModal(props: StockCrawlModalProps) {
  // Remount per open so the crawl restarts and selection re-seeds from props.
  return (
    <StockCrawlModalInner key={props.open ? "open" : "closed"} {...props} />
  );
}
