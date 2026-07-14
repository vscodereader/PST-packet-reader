import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { BlockEditor } from "./block-editor";
import type { Block } from "./blocks";

// 삽입 API/파일 다이얼로그는 상호작용 시에만 호출된다 — 렌더/순수 블록 테스트에는 스텁으로 충분.
vi.mock("@/shared/ipc", () => ({
  ipc: {
    blog: {
      oglink: vi.fn(),
      places: vi.fn(),
      staticmap: vi.fn(),
      stickers: vi.fn(),
      stickerSeqs: vi.fn(),
      uploadFile: vi.fn(),
      uploadPhoto: vi.fn(),
    },
  },
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

/** blocks 상태를 들고 BlockEditor를 감싸는 테스트 하네스. */
function Harness() {
  const [blocks, setBlocks] = useState<Block[]>([]);
  return (
    <MantineProvider>
      <BlockEditor accountId="acc1" blocks={blocks} onChange={setBlocks} />
    </MantineProvider>
  );
}

describe("BlockEditor", () => {
  it("상단 삽입 툴바에 7개 버튼만 렌더한다", () => {
    render(<Harness />);
    for (const label of [
      "사진",
      "스티커",
      "링크",
      "파일",
      "일정",
      "소스코드",
      "장소",
    ]) {
      expect(screen.getByRole("button", { name: label })).toBeInTheDocument();
    }
  });

  it("소스코드 버튼은 소스코드 블록을 추가한다", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "소스코드" }));
    expect(screen.getByLabelText("소스코드")).toBeInTheDocument();
  });

  it("문단 추가 후 서식 툴바(B·I·U·취소선)가 보이고 굵게를 토글한다", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "문단 추가" }));
    const bold = screen.getByLabelText("굵게");
    expect(bold).toBeInTheDocument();
    expect(screen.getByLabelText("기울임")).toBeInTheDocument();
    expect(screen.getByLabelText("밑줄")).toBeInTheDocument();
    expect(screen.getByLabelText("취소선")).toBeInTheDocument();
    expect(bold).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(bold);
    expect(screen.getByLabelText("굵게")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("삭제 버튼은 블록을 제거한다", () => {
    render(<Harness />);
    fireEvent.click(screen.getByRole("button", { name: "소스코드" }));
    expect(screen.getByLabelText("소스코드")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("삭제"));
    expect(screen.queryByLabelText("소스코드")).not.toBeInTheDocument();
  });
});
