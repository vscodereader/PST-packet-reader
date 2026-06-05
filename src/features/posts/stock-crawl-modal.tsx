import {
  Box,
  Button,
  Checkbox,
  Group,
  Modal,
  Text,
  TextInput,
} from "@mantine/core";
import { useEffect, useState } from "react";

import type { StockCandidate } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface StockCrawlModalProps {
  open: boolean;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: StockCandidate[]) => void;
}

function StockCrawlModalInner({
  preselected,
  onClose,
  onConfirm,
}: StockCrawlModalProps) {
  const [q, setQ] = useState("");
  const [results, setResults] = useState<StockCandidate[]>([]);
  const [sel, setSel] = useState<string[]>(preselected);
  // code → name for selected items, accumulated as the user picks from results,
  // so confirm can return the name even after the query changes.
  const [names, setNames] = useState<Record<string, string>>({});

  // Live search: debounce query changes; an empty query returns the top stocks.
  useEffect(() => {
    const id = window.setTimeout(() => {
      void ipc.stocks.search(q).then(setResults);
    }, 250);
    return () => window.clearTimeout(id);
  }, [q]);

  const toggle = (s: StockCandidate) => {
    setNames((m) => ({ ...m, [s.code]: s.name }));
    setSel((prev) =>
      prev.includes(s.code)
        ? prev.filter((x) => x !== s.code)
        : [...prev, s.code],
    );
  };

  const confirm = () =>
    onConfirm(
      sel.map((code) => {
        const hit = results.find((r) => r.code === code);
        return {
          code,
          name: hit?.name ?? names[code] ?? "",
          link: hit?.link ?? "",
        };
      }),
    );

  return (
    <Modal opened onClose={onClose} title="종목 검색" size={580} radius="lg">
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
          finance.naver.com 에서 종목토론방을 실시간 검색합니다.
        </Text>
      </Group>

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
        {results.map((s) => {
          const checked = sel.includes(s.code);
          return (
            <Group
              key={s.code}
              gap={11}
              px={13}
              py={10}
              wrap="nowrap"
              onClick={() => toggle(s)}
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
                </Group>
              </Box>
            </Group>
          );
        })}
      </Box>

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
          onClick={confirm}
        >
          적용 ({sel.length})
        </Button>
      </Group>
    </Modal>
  );
}

export function StockCrawlModal(props: StockCrawlModalProps) {
  // Remount per open so the search re-runs and selection re-seeds from props.
  if (!props.open) return null;
  return <StockCrawlModalInner key="open" {...props} />;
}
