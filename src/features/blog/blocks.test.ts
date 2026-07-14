import { describe, expect, it } from "vitest";

import type { OglinkMeta, PlaceResult, UploadedImage } from "@/shared/ipc";

import {
  createCodeBlock,
  createImageBlock,
  createOglinkBlock,
  createPlacesMapBlock,
  createScheduleBlock,
  createStickerBlock,
  createTextBlock,
  moveBlock,
  newBlockId,
  removeBlock,
  setAlign,
  setCode,
  setText,
  toggleMark,
  type Block,
  type CodeBlock,
  type TextBlock,
} from "./blocks";

describe("blocks model", () => {
  it("newBlockId produces unique prefixed ids", () => {
    const a = newBlockId();
    const b = newBlockId();
    expect(a).not.toBe(b);
    expect(a.startsWith("blk-")).toBe(true);
  });

  it("createTextBlock has no formatting and left align by default", () => {
    const t = createTextBlock("hi");
    expect(t.type).toBe("text");
    expect(t.text).toBe("hi");
    expect(t.align).toBe("left");
    expect(t.bold).toBe(false);
    expect(t.strikeThrough).toBe(false);
  });

  it("createCodeBlock defaults to justify align (editor default)", () => {
    expect(createCodeBlock("x").align).toBe("justify");
  });

  it("createScheduleBlock carries title/startAt/dateOnly", () => {
    const s = createScheduleBlock("회의", "2026-07-14T10:40:09+09:00", true);
    expect(s.type).toBe("schedule");
    expect(s.title).toBe("회의");
    expect(s.startAt).toBe("2026-07-14T10:40:09+09:00");
    expect(s.dateOnly).toBe(true);
  });

  it("createImageBlock maps upload result fields", () => {
    const img: UploadedImage = {
      src: "https://blogfiles.pstatic.net/a/x.png?type=w1",
      path: "/a/x.png",
      domain: "https://blogfiles.pstatic.net",
      fileSize: 100,
      width: 600,
      height: 400,
      originalWidth: 1200,
      originalHeight: 800,
      fileName: "x.png",
    };
    const b = createImageBlock(img);
    expect(b.type).toBe("image");
    expect(b.src).toBe(img.src);
    expect(b.originalWidth).toBe(1200);
  });

  it("createOglinkBlock keeps the original link and meta", () => {
    const meta: OglinkMeta = {
      url: "https://naver.com",
      title: "네이버",
      domain: "naver.com",
      description: "검색",
      thumbnailSrc: "https://img/x.png",
      thumbnailWidth: 300,
      thumbnailHeight: 200,
      oglinkSign: "SIGN",
    };
    const b = createOglinkBlock("https://naver.com", meta);
    expect(b.type).toBe("oglink");
    expect(b.link).toBe("https://naver.com");
    expect(b.oglinkSign).toBe("SIGN");
    expect(b.thumbnailSrc).toBe("https://img/x.png");
  });

  it("createStickerBlock carries pack/seq", () => {
    const b = createStickerBlock("motion2d_01", 10);
    expect(b.packCode).toBe("motion2d_01");
    expect(b.seq).toBe(10);
  });

  it("createPlacesMapBlock builds a single place from search result", () => {
    const place: PlaceResult = {
      id: "1621706163",
      name: "카페",
      tel: "02-1",
      roadAddress: "도로1",
      address: "지번1",
      x: "127.0",
      y: "37.5",
      placeType: "s",
      thumUrl: "https://t/1.png",
    };
    const b = createPlacesMapBlock("https://map/static.png", place);
    expect(b.type).toBe("placesMap");
    expect(b.thumbnailSrc).toBe("https://map/static.png");
    expect(b.places).toHaveLength(1);
    const p = b.places[0]!;
    expect(p.placeId).toBe("1621706163");
    expect(p.address).toBe("도로1");
    expect(p.latitude).toBe("37.5");
    expect(p.longitude).toBe("127.0");
  });

  it("toggleMark flips a formatting flag immutably", () => {
    const blocks: Block[] = [createTextBlock("a")];
    const id = blocks[0]!.id;
    const next = toggleMark(blocks, id, "bold");
    expect((next[0] as TextBlock).bold).toBe(true);
    expect((blocks[0] as TextBlock).bold).toBe(false);
    const back = toggleMark(next, id, "bold");
    expect((back[0] as TextBlock).bold).toBe(false);
  });

  it("setAlign updates align for align-capable blocks", () => {
    const blocks: Block[] = [createTextBlock("a")];
    const id = blocks[0]!.id;
    expect((setAlign(blocks, id, "center")[0] as TextBlock).align).toBe(
      "center",
    );
  });

  it("setText and setCode update the right block", () => {
    const blocks: Block[] = [createTextBlock("a"), createCodeBlock("b")];
    const textId = blocks[0]!.id;
    const codeId = blocks[1]!.id;
    expect((setText(blocks, textId, "z")[0] as TextBlock).text).toBe("z");
    expect((setCode(blocks, codeId, "y")[1] as CodeBlock).code).toBe("y");
  });

  it("removeBlock drops the block", () => {
    const blocks: Block[] = [createTextBlock("a"), createTextBlock("b")];
    const next = removeBlock(blocks, blocks[0]!.id);
    expect(next).toHaveLength(1);
    expect(next[0]!.id).toBe(blocks[1]!.id);
  });

  it("moveBlock swaps neighbours and clamps at edges", () => {
    const blocks: Block[] = [createTextBlock("a"), createTextBlock("b")];
    const first = blocks[0]!.id;
    const down = moveBlock(blocks, first, 1);
    expect(down[0]!.id).toBe(blocks[1]!.id);
    // 맨 위에서 위로 이동 = 변화 없음(원본 반환).
    expect(moveBlock(blocks, first, -1)).toBe(blocks);
  });
});
