import {
  Alert,
  Badge,
  Box,
  Button,
  Group,
  Modal,
  Select,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { useMemo, useState } from "react";

import type { Account, Cafe } from "@/shared/data/types";
import { ipc } from "@/shared/ipc";
import { Icon } from "@/shared/ui/icons";

export interface AddCafeModalProps {
  open: boolean;
  accounts: Account[];
  onClose: () => void;
  /** Called with the saved cafe after a successful resolve + upsert. */
  onAdded: (cafe: Cafe) => void;
}

/** Pull a human message out of the backend error envelope (or any thrown value). */
function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return typeof e === "string" ? e : "카페를 조회하지 못했습니다.";
}

/**
 * "+ 카페 추가" — resolve a cafe URL/slug into a registrable cafe using a chosen
 * naver account's cookie, preview its boards, then persist it. Keeps the heavy
 * discovery at registration time so the publish flow can read from cache.
 */
export function AddCafeModal({
  open,
  accounts,
  onClose,
  onAdded,
}: AddCafeModalProps) {
  const naverAccounts = useMemo(
    () => accounts.filter((a) => a.platform === "naver"),
    [accounts],
  );
  // Default to the first active naver account (else the first naver account);
  // an explicit pick overrides it. Derived, so no effect/cascading render.
  const defaultAccountId = useMemo(() => {
    const def =
      naverAccounts.find((a) => a.status === "active") ?? naverAccounts[0];
    return def?.id ?? "";
  }, [naverAccounts]);
  const [accountId, setAccountId] = useState("");
  const effectiveAccountId = accountId || defaultAccountId;
  const [input, setInput] = useState("");
  const [phase, setPhase] = useState<"idle" | "resolving" | "saving">("idle");
  const [resolved, setResolved] = useState<Cafe | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setInput("");
    setResolved(null);
    setError(null);
    setPhase("idle");
    onClose();
  };

  const resolve = () => {
    const ref = input.trim();
    if (!effectiveAccountId || !ref || phase !== "idle") return;
    setPhase("resolving");
    setError(null);
    setResolved(null);
    ipc.cafes
      .resolve(ref, effectiveAccountId)
      .then((cafe) => {
        setResolved(cafe);
        setPhase("idle");
      })
      .catch((e: unknown) => {
        setError(errorMessage(e));
        setPhase("idle");
      });
  };

  const save = () => {
    if (!resolved || phase !== "idle") return;
    setPhase("saving");
    ipc.cafes
      .upsert(resolved)
      .then(() => {
        onAdded(resolved);
        close();
      })
      .catch((e: unknown) => {
        setError(errorMessage(e));
        setPhase("idle");
      });
  };

  return (
    <Modal
      opened={open}
      onClose={close}
      title="네이버 카페 추가"
      size={460}
      radius="lg"
      centered
    >
      {naverAccounts.length === 0 ? (
        <Alert color="yellow" icon={<Icon.alert size={18} />}>
          네이버 계정을 먼저 추가한 뒤 카페를 등록할 수 있어요.
        </Alert>
      ) : (
        <Stack gap={12}>
          <Select
            label="카페 조회에 사용할 계정"
            data={naverAccounts.map((a) => ({ value: a.id, label: a.loginId }))}
            value={effectiveAccountId}
            onChange={(v) => setAccountId(v ?? "")}
            allowDeselect={false}
          />
          <Group gap={8} align="flex-end">
            <TextInput
              label="카페 주소 또는 ID"
              placeholder="cafe.naver.com/주소 또는 숫자 ID"
              value={input}
              onChange={(e) => setInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") resolve();
              }}
              style={{ flex: 1 }}
            />
            <Button
              onClick={resolve}
              loading={phase === "resolving"}
              disabled={!effectiveAccountId || !input.trim()}
              leftSection={<Icon.search size={15} />}
            >
              조회
            </Button>
          </Group>

          {error && (
            <Alert color="red" icon={<Icon.alert size={18} />}>
              {error}
            </Alert>
          )}

          {resolved && (
            <Box
              p={12}
              style={{
                border: "1px solid var(--mantine-color-gray-2)",
                borderRadius: "var(--mantine-radius-md)",
                background: "var(--mantine-color-gray-0)",
              }}
            >
              <Group justify="space-between" wrap="nowrap">
                <Box style={{ minWidth: 0 }}>
                  <Text fz={14} fw={700} truncate>
                    {resolved.name}
                  </Text>
                  <Badge mt={4} size="sm" variant="light" color="naver">
                    글쓰기 가능 게시판 {resolved.boards.length}개
                  </Badge>
                </Box>
                <Button
                  onClick={save}
                  loading={phase === "saving"}
                  leftSection={<Icon.check size={15} />}
                >
                  저장
                </Button>
              </Group>
            </Box>
          )}
        </Stack>
      )}
    </Modal>
  );
}
