import { Button, Stack, Textarea, TextInput } from "@mantine/core";

type DraftComposerProps = {
  blankMemo: string;
  contentDraft: string;
  titleDraft: string;
  onBlankMemoChange: (value: string) => void;
  onContentDraftChange: (value: string) => void;
  onSaveContent: () => void;
  onSaveTitle: () => void;
  onTitleDraftChange: (value: string) => void;
};

export function DraftComposer({
  blankMemo,
  contentDraft,
  titleDraft,
  onBlankMemoChange,
  onContentDraftChange,
  onSaveContent,
  onSaveTitle,
  onTitleDraftChange,
}: DraftComposerProps) {
  return (
    <Stack gap="md" className="macro-editor-draft">
      <Textarea
        aria-label="빈 텍스트 필드"
        minRows={5}
        value={blankMemo}
        onChange={(event) => onBlankMemoChange(event.currentTarget.value)}
      />

      <div className="macro-editor-save-row">
        <TextInput
          aria-label="제목 작성"
          placeholder="제목을 작성하세요"
          value={titleDraft}
          onChange={(event) => onTitleDraftChange(event.currentTarget.value)}
        />
        <Button onClick={onSaveTitle}>저장</Button>
      </div>

      <div className="macro-editor-save-row macro-editor-save-row-content">
        <Textarea
          aria-label="내용 작성"
          minRows={8}
          placeholder="내용을 작성하세요"
          value={contentDraft}
          onChange={(event) => onContentDraftChange(event.currentTarget.value)}
        />
        <Button onClick={onSaveContent}>저장</Button>
      </div>
    </Stack>
  );
}
