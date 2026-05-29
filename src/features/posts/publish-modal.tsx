import { Modal, Stack, Text } from "@mantine/core";

import type { GoFn, LibraryPost } from "@/shared/data/types";

export interface PublishModalProps {
  open: boolean;
  doc: LibraryPost | null;
  onClose: () => void;
  go: GoFn;
}

// Skeleton — account picker (segmented filter), destination selection,
// timing, and template-variable preview land in a follow-up commit.
export function PublishModal({ open, doc, onClose }: PublishModalProps) {
  return (
    <Modal
      opened={open}
      onClose={onClose}
      title="게시 설정"
      size="lg"
      radius="lg"
    >
      <Stack gap="xs" py="md">
        <Text fw={600}>{doc?.title ?? ""}</Text>
        <Text c="dimmed" size="sm">
          게시 설정 구현 예정
        </Text>
      </Stack>
    </Modal>
  );
}
