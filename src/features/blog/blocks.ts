// 네이버 블로그 편집기 툴바의 본문 = 블록 배열. 각 블록은 발행 시 백엔드(document_model.rs)에서
// SmartEditor documentModel components[]로 변환된다. 여기서는 블록의 TS 타입과 순수 조작
// 헬퍼(생성/서식토글/정렬/텍스트변경)만 둔다 — 네트워크·UI는 block-editor.tsx가 담당한다.
//
// 각 블록의 `id`는 React 키 전용 클라이언트 값이며, 백엔드 serde는 이 필드를 무시한다(태그는 `type`).

import type {
  OglinkMeta,
  PlaceResult,
  UploadedFile,
  UploadedImage,
} from "@/shared/ipc";

/** 문단/컴포넌트 정렬. */
export type Align = "left" | "center" | "right" | "justify";

/** 텍스트 블록 — 블록 전체에 서식/정렬 적용(하단 서식 툴바 미러). 줄바꿈마다 문단. */
export interface TextBlock {
  id: string;
  type: "text";
  text: string;
  align: Align;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikeThrough: boolean;
}

/** 소스코드 블록(순수 클라이언트). */
export interface CodeBlock {
  id: string;
  type: "code";
  code: string;
  align: Align;
}

/** 일정 블록(순수 클라이언트). */
export interface ScheduleBlock {
  id: string;
  type: "schedule";
  title: string;
  /** ISO8601(예: 2026-07-14T10:40:09+09:00). */
  startAt: string;
  dateOnly: boolean;
  align: Align;
}

/** 파일 블록(업로드 결과). */
export interface FileBlock {
  id: string;
  type: "file";
  fileId: string;
  fileName: string;
  fileSize: number;
}

/** 사진 블록(업로드 결과). */
export interface ImageBlock {
  id: string;
  type: "image";
  src: string;
  path: string;
  domain: string;
  fileSize: number;
  width: number;
  height: number;
  originalWidth: number;
  originalHeight: number;
  fileName: string;
}

/** 링크(oglink) 블록. */
export interface OglinkBlock {
  id: string;
  type: "oglink";
  title: string;
  domain: string;
  link: string;
  thumbnailSrc: string;
  thumbnailWidth: number;
  thumbnailHeight: number;
  description: string;
  oglinkSign: string;
}

/** 스티커 블록. */
export interface StickerBlock {
  id: string;
  type: "sticker";
  packCode: string;
  seq: number;
  align: Align;
}

/** 장소(placesMap)의 place 원소. */
export interface PlaceItem {
  placeId: string;
  name: string;
  address: string;
  latitude: string;
  longitude: string;
  searchType: string;
  tel: string;
}

/** 장소(지도) 블록. */
export interface PlacesMapBlock {
  id: string;
  type: "placesMap";
  thumbnailSrc: string;
  places: PlaceItem[];
  align: Align;
}

/** 본문 블록 유니온. */
export type Block =
  | TextBlock
  | CodeBlock
  | ScheduleBlock
  | FileBlock
  | ImageBlock
  | OglinkBlock
  | StickerBlock
  | PlacesMapBlock;

/** 텍스트 블록의 서식 토글 키. */
export type TextMark = "bold" | "italic" | "underline" | "strikeThrough";

let counter = 0;

/** React 키 전용 클라이언트 블록 id. crypto.randomUUID가 있으면 그것을, 없으면 카운터를 쓴다. */
export function newBlockId(): string {
  const uuid =
    typeof globalThis.crypto?.randomUUID === "function"
      ? globalThis.crypto.randomUUID()
      : `${Date.now()}-${(counter += 1)}`;
  return `blk-${uuid}`;
}

/** 빈 텍스트 블록. */
export function createTextBlock(text = ""): TextBlock {
  return {
    id: newBlockId(),
    type: "text",
    text,
    align: "left",
    bold: false,
    italic: false,
    underline: false,
    strikeThrough: false,
  };
}

/** 빈 소스코드 블록(편집기 기본 정렬 justify). */
export function createCodeBlock(code = ""): CodeBlock {
  return { id: newBlockId(), type: "code", code, align: "justify" };
}

/** 일정 블록. */
export function createScheduleBlock(
  title: string,
  startAt: string,
  dateOnly = false,
): ScheduleBlock {
  return {
    id: newBlockId(),
    type: "schedule",
    title,
    startAt,
    dateOnly,
    align: "left",
  };
}

/** 파일 업로드 결과 → 파일 블록. */
export function createFileBlock(f: UploadedFile): FileBlock {
  return {
    id: newBlockId(),
    type: "file",
    fileId: f.fileId,
    fileName: f.fileName,
    fileSize: f.fileSize,
  };
}

/** 사진 업로드 결과 → 사진 블록. */
export function createImageBlock(img: UploadedImage): ImageBlock {
  return {
    id: newBlockId(),
    type: "image",
    src: img.src,
    path: img.path,
    domain: img.domain,
    fileSize: img.fileSize,
    width: img.width,
    height: img.height,
    originalWidth: img.originalWidth,
    originalHeight: img.originalHeight,
    fileName: img.fileName,
  };
}

/** oglink 메타 + 원본 URL → 링크 블록. */
export function createOglinkBlock(link: string, meta: OglinkMeta): OglinkBlock {
  return {
    id: newBlockId(),
    type: "oglink",
    title: meta.title,
    domain: meta.domain,
    // oglinkSign이 서명한 정규화 URL(meta.url)을 우선 사용한다(없으면 사용자 입력 link).
    // 사용자 입력을 그대로 쓰면 서명 URL과 달라 발행이 "not acceptable"로 거부된다.
    link: meta.url || link,
    thumbnailSrc: meta.thumbnailSrc,
    thumbnailWidth: meta.thumbnailWidth,
    thumbnailHeight: meta.thumbnailHeight,
    description: meta.description,
    oglinkSign: meta.oglinkSign,
  };
}

/** 스티커 블록. */
export function createStickerBlock(
  packCode: string,
  seq: number,
): StickerBlock {
  return { id: newBlockId(), type: "sticker", packCode, seq, align: "left" };
}

/** staticmap 썸네일 + 선택 장소 → 장소 블록. */
export function createPlacesMapBlock(
  thumbnailSrc: string,
  place: PlaceResult,
): PlacesMapBlock {
  return {
    id: newBlockId(),
    type: "placesMap",
    thumbnailSrc,
    align: "left",
    places: [
      {
        placeId: place.id,
        name: place.name,
        address: place.roadAddress || place.address,
        latitude: place.y,
        longitude: place.x,
        searchType: place.placeType,
        tel: place.tel,
      },
    ],
  };
}

/** 블록 배열에서 한 블록을 patch로 갱신한 새 배열을 만든다(불변). */
export function updateBlock<T extends Block>(
  blocks: Block[],
  id: string,
  patch: (block: T) => T,
): Block[] {
  return blocks.map((b) => (b.id === id ? patch(b as T) : b));
}

/** 텍스트 블록의 서식 마크를 토글한 새 배열을 만든다. */
export function toggleMark(
  blocks: Block[],
  id: string,
  mark: TextMark,
): Block[] {
  return updateBlock<TextBlock>(blocks, id, (b) => ({
    ...b,
    [mark]: !b[mark],
  }));
}

/** 정렬을 지원하는 블록(text/code/schedule/sticker/placesMap)의 align을 바꾼 새 배열을 만든다. */
export function setAlign(blocks: Block[], id: string, align: Align): Block[] {
  return blocks.map((b) => {
    if (b.id !== id) return b;
    if ("align" in b) return { ...b, align };
    return b;
  });
}

/** 텍스트 블록의 본문을 바꾼 새 배열을 만든다. */
export function setText(blocks: Block[], id: string, text: string): Block[] {
  return updateBlock<TextBlock>(blocks, id, (b) => ({ ...b, text }));
}

/** 소스코드 블록의 코드를 바꾼 새 배열을 만든다. */
export function setCode(blocks: Block[], id: string, code: string): Block[] {
  return updateBlock<CodeBlock>(blocks, id, (b) => ({ ...b, code }));
}

/** 블록을 제거한 새 배열을 만든다. */
export function removeBlock(blocks: Block[], id: string): Block[] {
  return blocks.filter((b) => b.id !== id);
}

/** 블록을 위(-1)/아래(+1)로 한 칸 이동한 새 배열을 만든다(범위 밖이면 원본 그대로). */
export function moveBlock(
  blocks: Block[],
  id: string,
  direction: -1 | 1,
): Block[] {
  const index = blocks.findIndex((b) => b.id === id);
  if (index === -1) return blocks;
  const target = index + direction;
  if (target < 0 || target >= blocks.length) return blocks;
  const next = [...blocks];
  const a = next[index];
  const b = next[target];
  if (!a || !b) return blocks;
  next[index] = b;
  next[target] = a;
  return next;
}
