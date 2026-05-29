import { Modal, Stack, Text } from "@mantine/core";

import type { LibraryPost } from "@/shared/data/types";

export interface WriterModalProps {
  open: boolean;
  doc: LibraryPost | null;
  drafts: LibraryPost[];
  onClose: () => void;
  onSave: (doc: LibraryPost) => void;
  onSaveDraft: (doc: LibraryPost) => void;
  onDeleteDraft: (doc: LibraryPost) => void;
}

// Skeleton — full editor (modes, toolbar, variables, comment composer,
// draft list) lands in a follow-up commit.
export function WriterModal({ open, doc, onClose }: WriterModalProps) {
  return (
    <Modal
      opened={open}
      onClose={onClose}
      title={doc ? "글 편집" : "글쓰기"}
      size="lg"
      radius="lg"
    >
      <Stack gap="xs" py="md">
        <Text fw={600}>{doc?.title ?? "새 글"}</Text>
        <Text c="dimmed" size="sm">
          에디터 구현 예정
        </Text>
      </Stack>
    </Modal>
  );
}
