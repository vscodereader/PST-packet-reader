import { Container, Stack, Title } from "@mantine/core";

import { AutomationPanel } from "./automation-panel";
import { DraftComposer } from "./draft-composer";
import { SavedItemPicker } from "./saved-item-picker";
import { useMacroEditor } from "./use-macro-editor";

import "./macro-editor.css";

export function MacroEditorPage() {
  const editor = useMacroEditor();

  return (
    <Container fluid className="macro-editor">
      <Title order={1} className="macro-editor-title">
        pstmacro
      </Title>

      <div className="macro-editor-layout">
        <DraftComposer
          blankMemo={editor.blankMemo}
          contentDraft={editor.contentDraft}
          titleDraft={editor.titleDraft}
          onBlankMemoChange={editor.setBlankMemo}
          onContentDraftChange={editor.setContentDraft}
          onSaveContent={() => editor.saveDraft("content")}
          onSaveTitle={() => editor.saveDraft("title")}
          onTitleDraftChange={editor.setTitleDraft}
        />

        <Stack gap="lg">
          <AutomationPanel
            contentDraft={editor.contentDraft}
            selectedContents={editor.selectedContents}
            selectedTitles={editor.selectedTitles}
            titleDraft={editor.titleDraft}
          />

          <SavedItemPicker
            checkedId={editor.titleState.checkedId}
            editorLabel="선택된 제목 편집"
            editorValue={editor.titleState.editorValue}
            entries={editor.titleState.entries}
            kind="title"
            pickerLabel="제목을 선택하세요"
            selectedEntries={editor.selectedTitles}
            onCheckEntry={editor.checkEntry}
            onDelete={editor.deleteChecked}
            onEdit={editor.editCheckedEntry}
            onEditorValueChange={editor.setEditorValue}
            onSave={editor.saveCheckedEntry}
            onToggleEntry={editor.toggleEntry}
          />

          <SavedItemPicker
            checkedId={editor.contentState.checkedId}
            editorLabel="선택된 내용 편집"
            editorValue={editor.contentState.editorValue}
            entries={editor.contentState.entries}
            kind="content"
            pickerLabel="내용을 선택하세요"
            selectedEntries={editor.selectedContents}
            onCheckEntry={editor.checkEntry}
            onDelete={editor.deleteChecked}
            onEdit={editor.editCheckedEntry}
            onEditorValueChange={editor.setEditorValue}
            onSave={editor.saveCheckedEntry}
            onToggleEntry={editor.toggleEntry}
          />
        </Stack>
      </div>
    </Container>
  );
}
