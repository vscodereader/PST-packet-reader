import { useEffect, useMemo, useState } from "react";

import type { SavedEntry, SavedEntryKind } from "./types";

type EntryState = {
  entries: SavedEntry[];
  selectedIds: string[];
  checkedId: string | null;
  editorValue: string;
};

const emptyEntryState: EntryState = {
  entries: [],
  selectedIds: [],
  checkedId: null,
  editorValue: "",
};

const storageKeys = {
  content: "pstmacro.contents",
  title: "pstmacro.titles",
} as const;

function readStoredEntries(kind: SavedEntryKind) {
  const rawEntries = localStorage.getItem(storageKeys[kind]);

  if (!rawEntries) {
    return [];
  }

  try {
    return JSON.parse(rawEntries) as SavedEntry[];
  } catch {
    return [];
  }
}

function createInitialEntryState(kind: SavedEntryKind): EntryState {
  return {
    ...emptyEntryState,
    entries: readStoredEntries(kind),
  };
}

function createEntry(kind: SavedEntryKind, label: string): SavedEntry {
  return {
    id: `${kind}-${Date.now()}-${crypto.randomUUID()}`,
    label,
    kind,
  };
}

function toggleSelectedId(ids: string[], id: string) {
  return ids.includes(id)
    ? ids.filter((selectedId) => selectedId !== id)
    : [...ids, id];
}

function applyCheckedId(state: EntryState, checkedId: string | null) {
  const checkedEntry = state.entries.find((entry) => entry.id === checkedId);

  return {
    ...state,
    checkedId,
    editorValue: checkedEntry?.label ?? "",
  };
}

function updateEditedEntry(state: EntryState) {
  const nextLabel = state.editorValue.trim();

  if (!state.checkedId || nextLabel.length === 0) {
    return state;
  }

  return {
    ...state,
    entries: state.entries.map((entry) =>
      entry.id === state.checkedId ? { ...entry, label: nextLabel } : entry,
    ),
  };
}

function deleteCheckedEntry(state: EntryState) {
  if (!state.checkedId) {
    return state;
  }

  return {
    ...state,
    entries: state.entries.filter((entry) => entry.id !== state.checkedId),
    selectedIds: state.selectedIds.filter((id) => id !== state.checkedId),
    checkedId: null,
    editorValue: "",
  };
}

export function useMacroEditor() {
  const [titleDraft, setTitleDraft] = useState("");
  const [contentDraft, setContentDraft] = useState("");
  const [blankMemo, setBlankMemo] = useState("");
  const [titleState, setTitleState] = useState<EntryState>(() =>
    createInitialEntryState("title"),
  );
  const [contentState, setContentState] = useState<EntryState>(() =>
    createInitialEntryState("content"),
  );

  useEffect(() => {
    localStorage.setItem(storageKeys.title, JSON.stringify(titleState.entries));
  }, [titleState.entries]);

  useEffect(() => {
    localStorage.setItem(
      storageKeys.content,
      JSON.stringify(contentState.entries),
    );
  }, [contentState.entries]);

  const selectedTitles = useMemo(
    () =>
      titleState.selectedIds
        .map((id) => titleState.entries.find((entry) => entry.id === id))
        .filter((entry): entry is SavedEntry => Boolean(entry)),
    [titleState.entries, titleState.selectedIds],
  );

  const selectedContents = useMemo(
    () =>
      contentState.selectedIds
        .map((id) => contentState.entries.find((entry) => entry.id === id))
        .filter((entry): entry is SavedEntry => Boolean(entry)),
    [contentState.entries, contentState.selectedIds],
  );

  function saveDraft(kind: SavedEntryKind) {
    if (kind === "title") {
      const nextTitle = titleDraft.trim();

      if (nextTitle.length === 0) {
        return;
      }

      setTitleState((current) => ({
        ...current,
        entries: [...current.entries, createEntry("title", nextTitle)],
      }));
      setTitleDraft("");
      return;
    }

    const nextContent = contentDraft.trim();

    if (nextContent.length === 0) {
      return;
    }

    setContentState((current) => ({
      ...current,
      entries: [...current.entries, createEntry("content", nextContent)],
    }));
    setContentDraft("");
  }

  function toggleEntry(kind: SavedEntryKind, id: string) {
    const update = (current: EntryState) => ({
      ...current,
      selectedIds: toggleSelectedId(current.selectedIds, id),
    });

    if (kind === "title") {
      setTitleState(update);
      return;
    }

    setContentState(update);
  }

  function checkEntry(kind: SavedEntryKind, id: string | null) {
    if (kind === "title") {
      setTitleState((current) => applyCheckedId(current, id));
      return;
    }

    setContentState((current) => applyCheckedId(current, id));
  }

  function editCheckedEntry(kind: SavedEntryKind) {
    const applyEdit = (current: EntryState) => {
      const checkedEntry = current.entries.find(
        (entry) => entry.id === current.checkedId,
      );

      return {
        ...current,
        editorValue: checkedEntry?.label ?? current.editorValue,
      };
    };

    if (kind === "title") {
      setTitleState(applyEdit);
      return;
    }

    setContentState(applyEdit);
  }

  function saveCheckedEntry(kind: SavedEntryKind) {
    if (kind === "title") {
      setTitleState(updateEditedEntry);
      return;
    }

    setContentState(updateEditedEntry);
  }

  function deleteChecked(kind: SavedEntryKind) {
    if (kind === "title") {
      setTitleState(deleteCheckedEntry);
      return;
    }

    setContentState(deleteCheckedEntry);
  }

  function setEditorValue(kind: SavedEntryKind, value: string) {
    if (kind === "title") {
      setTitleState((current) => ({ ...current, editorValue: value }));
      return;
    }

    setContentState((current) => ({ ...current, editorValue: value }));
  }

  return {
    blankMemo,
    contentDraft,
    contentState,
    selectedContents,
    selectedTitles,
    titleDraft,
    titleState,
    checkEntry,
    deleteChecked,
    editCheckedEntry,
    saveCheckedEntry,
    saveDraft,
    setBlankMemo,
    setContentDraft,
    setEditorValue,
    setTitleDraft,
    toggleEntry,
  };
}
