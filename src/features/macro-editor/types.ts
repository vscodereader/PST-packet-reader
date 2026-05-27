export type SavedEntryKind = "title" | "content";

export type SavedEntry = {
  id: string;
  label: string;
  kind: SavedEntryKind;
};
