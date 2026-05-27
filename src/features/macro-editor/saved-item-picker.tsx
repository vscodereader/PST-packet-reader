import {
  Button,
  Checkbox,
  Group,
  Radio,
  Stack,
  Text,
  Textarea,
  UnstyledButton,
} from "@mantine/core";
import { useState } from "react";

import type { SavedEntry, SavedEntryKind } from "./types";

type SavedItemPickerProps = {
  checkedId: string | null;
  editorLabel: string;
  editorValue: string;
  entries: SavedEntry[];
  kind: SavedEntryKind;
  pickerLabel: string;
  selectedEntries: SavedEntry[];
  onCheckEntry: (kind: SavedEntryKind, id: string | null) => void;
  onDelete: (kind: SavedEntryKind) => void;
  onEdit: (kind: SavedEntryKind) => void;
  onEditorValueChange: (kind: SavedEntryKind, value: string) => void;
  onSave: (kind: SavedEntryKind) => void;
  onToggleEntry: (kind: SavedEntryKind, id: string) => void;
};

export function SavedItemPicker({
  checkedId,
  editorLabel,
  editorValue,
  entries,
  kind,
  pickerLabel,
  selectedEntries,
  onCheckEntry,
  onDelete,
  onEdit,
  onEditorValueChange,
  onSave,
  onToggleEntry,
}: SavedItemPickerProps) {
  const [opened, setOpened] = useState(false);

  return (
    <section className="macro-editor-picker">
      <Group justify="space-between" align="center" gap="sm">
        <Text fw={700}>{pickerLabel}</Text>
        <Radio
          aria-label={`${pickerLabel} 열기`}
          checked={opened}
          onChange={() => setOpened((current) => !current)}
        />
      </Group>

      {opened ? (
        <Stack gap={4} className="macro-editor-option-list">
          {entries.length === 0 ? (
            <Text c="dimmed" size="sm">
              저장된 항목이 없습니다.
            </Text>
          ) : (
            entries.map((entry) => (
              <UnstyledButton
                key={entry.id}
                className="macro-editor-option"
                onClick={() => onToggleEntry(kind, entry.id)}
              >
                <Text size="sm">{entry.label}</Text>
              </UnstyledButton>
            ))
          )}
        </Stack>
      ) : null}

      <Stack gap="xs" className="macro-editor-selected-list">
        {selectedEntries.length === 0 ? (
          <Text c="dimmed" size="sm">
            선택된 항목이 없습니다.
          </Text>
        ) : (
          selectedEntries.map((entry) => (
            <Checkbox
              key={entry.id}
              checked={checkedId === entry.id}
              label={entry.label}
              onChange={(event) =>
                onCheckEntry(kind, event.currentTarget.checked ? entry.id : null)
              }
            />
          ))
        )}
      </Stack>

      <div className="macro-editor-edit-row">
        <Textarea
          aria-label={editorLabel}
          minRows={4}
          value={editorValue}
          onChange={(event) =>
            onEditorValueChange(kind, event.currentTarget.value)
          }
        />
        <Stack gap="xs" className="macro-editor-actions">
          <Button variant="light" onClick={() => onEdit(kind)}>
            편집
          </Button>
          <Button color="red" variant="light" onClick={() => onDelete(kind)}>
            삭제
          </Button>
          <Button onClick={() => onSave(kind)}>저장</Button>
        </Stack>
      </div>
    </section>
  );
}
