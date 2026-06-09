import {
  Box,
  Button,
  Checkbox,
  Group,
  Modal,
  Text,
  TextInput,
} from "@mantine/core";
import { useCallback, useEffect, useState } from "react";

import type { ForumStock } from "@/shared/bindings/ForumStock";
import type { ForumStockCategory } from "@/shared/bindings/ForumStockCategory";
import type { StockExchange } from "@/shared/bindings/StockExchange";
import type { StockCandidate } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface StockCrawlModalProps {
  open: boolean;
  preselected: string[];
  onClose: () => void;
  onConfirm: (stocks: StockCandidate[]) => void;
}

const CATEGORIES: { key: ForumStockCategory; label: string }[] = [
  { key: "discussion", label: "토론" },
  { key: "tradingValue", label: "거래대금" },
  { key: "popular", label: "인기 종목" },
  { key: "rising", label: "상승" },
  { key: "falling", label: "하락" },
  { key: "volume", label: "거래량" },
];

function changeColor(t: string): string {
  if (t === "rising") return "var(--mantine-color-red-6)";
  if (t === "falling") return "var(--mantine-color-blue-6)";
  return "var(--mantine-color-gray-6)";
}

function StockCrawlModalInner({
  preselected,
  onClose,
  onConfirm,
}: StockCrawlModalProps) {
  const [category, setCategory] = useState<ForumStockCategory>("tradingValue");
  const [exchange, setExchange] = useState<StockExchange>("krx");
  const [exchangeOpen, setExchangeOpen] = useState(false);
  const [q, setQ] = useState("");
  const [rows, setRows] = useState<ForumStock[]>([]);
  const [page, setPage] = useState(1);
  const [hasNext, setHasNext] = useState(false);
  const [loading, setLoading] = useState(false);
  const [sel, setSel] = useState<string[]>(preselected);
  // code → name for selected items, accumulated as the user picks rows, so
  // confirm can return the name even after the query/category changes.
  const [names, setNames] = useState<Record<string, string>>({});

  // Load page 1 of the current tab/exchange, or search when a query is typed.
  // Debounced so typing doesn't spam the backend.
  useEffect(() => {
    let alive = true;
    const query = q.trim();
    const id = window.setTimeout(() => {
      const req = query
        ? ipc.forumStocks.search(query, 1)
        : ipc.forumStocks.list(category, exchange, 1);
      void req.then((p) => {
        if (!alive) return;
        setRows(p.stocks);
        setPage(1);
        setHasNext(p.hasNext);
      });
    }, 250);
    return () => {
      alive = false;
      window.clearTimeout(id);
    };
  }, [q, category, exchange]);

  const loadMore = useCallback(() => {
    const query = q.trim();
    const next = page + 1;
    setLoading(true);
    const req = query
      ? ipc.forumStocks.search(query, next)
      : ipc.forumStocks.list(category, exchange, next);
    void req
      .then((p) => {
        setRows((prev) => [...prev, ...p.stocks]);
        setPage(next);
        setHasNext(p.hasNext);
      })
      .finally(() => setLoading(false));
  }, [q, category, exchange, page]);

  const toggle = useCallback((s: ForumStock) => {
    setNames((m) => ({ ...m, [s.code]: s.name }));
    setSel((prev) =>
      prev.includes(s.code)
        ? prev.filter((x) => x !== s.code)
        : [...prev, s.code],
    );
  }, []);

  const confirm = () =>
    onConfirm(
      sel.map((code) => ({
        code,
        name: names[code] ?? rows.find((r) => r.code === code)?.name ?? "",
        link: `https://stock.naver.com/domestic/stock/${code}/discussion?chip=all`,
      })),
    );

  const searching = q.trim().length > 0;

  return (
    <Modal opened onClose={onClose} title="종목 선택" size={580} radius="lg">
      <Modal
        opened={exchangeOpen}
        onClose={() => setExchangeOpen(false)}
        title="거래소 선택"
        size={300}
        centered
        overlayProps={{ backgroundOpacity: 0.55, blur: 2 }}
      >
        <Group grow>
          {(["krx", "nxt"] as StockExchange[]).map((ex) => (
            <Button
              key={ex}
              variant={exchange === ex ? "filled" : "default"}
              color="forum"
              onClick={() => {
                setExchange(ex);
                setExchangeOpen(false);
              }}
            >
              {ex.toUpperCase()}
            </Button>
          ))}
        </Group>
      </Modal>

      <TextInput
        mb={10}
        value={q}
        onChange={(e) => setQ(e.currentTarget.value)}
        placeholder="종목명 또는 코드 검색"
        leftSection={<Icon.search size={16} />}
      />

      <Group justify="space-between" mb={8} wrap="nowrap">
        <Group gap={6} wrap="wrap">
          {CATEGORIES.map((c) => (
            <Button
              key={c.key}
              size="xs"
              variant={category === c.key ? "filled" : "default"}
              color="forum"
              disabled={searching}
              onClick={() => setCategory(c.key)}
            >
              {c.label}
            </Button>
          ))}
        </Group>
        <Button
          size="xs"
          variant="light"
          color="gray"
          rightSection={<Icon.chevronDown size={14} />}
          onClick={() => setExchangeOpen(true)}
        >
          {exchange.toUpperCase()}
        </Button>
      </Group>

      <Box
        style={{
          border: "1px solid var(--mantine-color-gray-2)",
          borderRadius: "var(--mantine-radius-md)",
          overflow: "hidden",
          maxHeight: 360,
          overflowY: "auto",
        }}
      >
        {rows.map((s) => {
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
              <Checkbox
                checked={checked}
                onChange={() => toggle(s)}
                onClick={(e) => e.stopPropagation()}
                size="sm"
              />
              {s.isHotDiscussion && (
                <Icon.flame size={15} color="var(--mantine-color-orange-6)" />
              )}
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
              <Text fz={13} fw={600}>
                {s.price}
              </Text>
              <Text
                fz={12}
                fw={700}
                c={changeColor(s.changeType)}
                style={{ minWidth: 56, textAlign: "right" }}
              >
                {s.changeRate}
              </Text>
            </Group>
          );
        })}
      </Box>

      {hasNext && (
        <Button
          mt={10}
          fullWidth
          variant="subtle"
          loading={loading}
          onClick={loadMore}
        >
          더보기
        </Button>
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
          onClick={confirm}
        >
          적용 ({sel.length})
        </Button>
      </Group>
    </Modal>
  );
}

export function StockCrawlModal(props: StockCrawlModalProps) {
  // Remount per open so the load re-runs and selection re-seeds from props.
  if (!props.open) return null;
  return <StockCrawlModalInner key="open" {...props} />;
}
