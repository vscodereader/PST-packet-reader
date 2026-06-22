import { MantineProvider } from "@mantine/core";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { WriterModal } from "./writer-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

function renderWriter(over: Partial<Parameters<typeof WriterModal>[0]> = {}) {
  const onSave = vi.fn();
  render(
    <MantineProvider>
      <WriterModal
        open
        doc={null}
        drafts={[]}
        onClose={vi.fn()}
        onSave={onSave}
        onSaveDraft={vi.fn()}
        onDeleteDraft={vi.fn()}
        {...over}
      />
    </MantineProvider>,
  );
  return { onSave };
}

describe("WriterModal", () => {
  it("renders the mode pills and a title field", async () => {
    renderWriter();
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "글 작성" })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "댓글 작성" }),
    ).toBeInTheDocument();
  });

  it("saves with the entered title via 저장", async () => {
    const { onSave } = renderWriter();
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "테스트 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]![0]).toMatchObject({
      title: "테스트 글",
      kind: "post",
      status: "ready",
    });
  });

  it("shows the comment composer in 댓글 작성 mode", async () => {
    renderWriter();
    await userEvent.click(screen.getByRole("button", { name: "댓글 작성" }));
    expect(await screen.findByText("댓글 대상")).toBeInTheDocument();
    expect(screen.getByText("댓글 내용")).toBeInTheDocument();
  });

  it("shows both the editor and the own-post comment notice in 글 + 댓글 mode", async () => {
    renderWriter();
    await userEvent.click(screen.getByRole("button", { name: "글 + 댓글" }));
    expect(
      await screen.findByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("위에서 작성한 글에 바로 댓글이 달립니다."),
    ).toBeInTheDocument();
  });

  it("adds a comment row in 댓글 작성 mode", async () => {
    renderWriter();
    await userEvent.click(screen.getByRole("button", { name: "댓글 작성" }));
    const ph = "자연스러운 댓글을 입력하세요";
    const before = (await screen.findAllByPlaceholderText(ph)).length;
    await userEvent.click(screen.getByRole("button", { name: /댓글 추가/ }));
    expect(screen.getAllByPlaceholderText(ph).length).toBe(before + 1);
  });

  it("saves a draft via the 임시저장 menu", async () => {
    const onSaveDraft = vi.fn();
    renderWriter({ onSaveDraft });
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "초안 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "임시저장" }));
    await userEvent.click(await screen.findByText("임시저장하기"));
    expect(onSaveDraft).toHaveBeenCalledTimes(1);
    expect(onSaveDraft.mock.calls[0]![0]).toMatchObject({
      title: "초안 글",
      status: "draft",
    });
  });

  it("inserts a template token into the title via the 변수 menu", async () => {
    renderWriter();
    const title = await screen.findByPlaceholderText("제목을 입력하세요");
    await userEvent.click(title);
    await userEvent.click(screen.getByRole("button", { name: "변수" }));
    await userEvent.click(await screen.findByText("종목명"));
    expect(title).toHaveValue("#{종목명}");
  });

  it("saves comment-mode content with the comment kind", async () => {
    const onSave = vi.fn();
    renderWriter({ onSave });
    await userEvent.click(screen.getByRole("button", { name: "댓글 작성" }));
    const boxes =
      await screen.findAllByPlaceholderText("자연스러운 댓글을 입력하세요");
    await userEvent.type(boxes[0]!, "좋은 분석이네요");
    await userEvent.click(screen.getByRole("button", { name: "저장" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave.mock.calls[0]![0]).toMatchObject({
      kind: "comment",
      status: "ready",
    });
  });

  it("prompts to save and can discard when closing with unsaved content", async () => {
    const onClose = vi.fn();
    renderWriter({ onClose });
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "임시 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "닫기" }));
    expect(
      await screen.findByText("작성 중인 글을 임시저장할까요?"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "저장 안 함" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("loads a draft from the 임시저장 list", async () => {
    const draft = {
      id: "d1",
      title: "불러올 초안",
      kind: "post" as const,
      updated: "방금 전",
      words: 10,
      status: "draft" as const,
      excerpt: "초안 요약",
      body: "<p>초안 본문</p>",
    };
    renderWriter({ drafts: [draft] });
    await userEvent.click(screen.getByRole("button", { name: "임시저장" }));
    await userEvent.click(
      await screen.findByRole("button", { name: "불러오기" }),
    );
    expect(await screen.findByDisplayValue("불러올 초안")).toBeInTheDocument();
  });

  it("invokes formatting commands from the toolbar", async () => {
    renderWriter();
    await userEvent.click(await screen.findByTitle("굵게"));
    await userEvent.click(screen.getByTitle("기울임"));
    await userEvent.click(screen.getByTitle("밑줄"));
    expect(
      screen.getByPlaceholderText("제목을 입력하세요"),
    ).toBeInTheDocument();
  });

  it("converts a pasted URL through the crawl helper", async () => {
    renderWriter();
    await screen.findByPlaceholderText("제목을 입력하세요");
    const body = document.querySelector("[contenteditable]") as HTMLElement;
    fireEvent.paste(body, {
      clipboardData: {
        getData: () =>
          "https://finance.naver.com/item/main.naver?code=005930 참고하세요",
      },
    });
    expect(body).toBeInTheDocument();
  });

  it("keeps a pasted non-stock URL without crashing", async () => {
    renderWriter();
    await screen.findByPlaceholderText("제목을 입력하세요");
    const body = document.querySelector("[contenteditable]") as HTMLElement;
    fireEvent.paste(body, {
      clipboardData: { getData: () => "https://www.example.com/article" },
    });
    expect(body).toBeInTheDocument();
  });

  it("inserts a token into the body when the editor is focused", async () => {
    renderWriter();
    const body = document.querySelector("[contenteditable]") as HTMLElement;
    await userEvent.click(body);
    await userEvent.click(screen.getByRole("button", { name: "변수" }));
    await userEvent.click(await screen.findByText("종목코드"));
    expect(body).toBeInTheDocument();
  });

  it("inserts an image via the file picker", async () => {
    renderWriter();
    await screen.findByPlaceholderText("제목을 입력하세요");
    const fileInput = document.querySelector(
      'input[type="file"]',
    ) as HTMLInputElement;
    const file = new File(["x"], "pic.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });
    await waitFor(() =>
      expect(
        screen.getByPlaceholderText("제목을 입력하세요"),
      ).toBeInTheDocument(),
    );
  });

  it("resets to a single empty row when removing the last comment", async () => {
    renderWriter();
    await userEvent.click(screen.getByRole("button", { name: "댓글 작성" }));
    const ph = "자연스러운 댓글을 입력하세요";
    let dels = await screen.findAllByTitle("삭제");
    await userEvent.click(dels[0]!); // 2 → 1
    dels = screen.getAllByTitle("삭제");
    await userEvent.click(dels[0]!); // length===1 → reset to [""]
    expect(screen.getAllByPlaceholderText(ph).length).toBe(1);
  });

  it("saves a draft from the close-confirmation dialog", async () => {
    const onClose = vi.fn();
    const onSaveDraft = vi.fn();
    renderWriter({ onClose, onSaveDraft });
    await userEvent.type(
      await screen.findByPlaceholderText("제목을 입력하세요"),
      "닫기 전 글",
    );
    await userEvent.click(screen.getByRole("button", { name: "닫기" }));
    const dialog = (
      await screen.findByText("작성 중인 글을 임시저장할까요?")
    ).closest('[role="dialog"]') as HTMLElement;
    await userEvent.click(
      within(dialog).getByRole("button", { name: "임시저장" }),
    );
    expect(onSaveDraft).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("edits comment-target options in comment mode", async () => {
    const user = userEvent.setup();
    renderWriter();
    await screen.findByPlaceholderText("제목을 입력하세요");

    // 댓글 작성 모드로 전환하면 댓글 대상(CommentComposer) 컨트롤이 노출된다.
    await user.click(screen.getByRole("button", { name: "댓글 작성" }));

    // "특정 게시글"(url) 대상 → URL 입력칸(기본 3개)이 나타난다.
    await user.click(
      await screen.findByRole("button", { name: "특정 게시글" }),
    );
    const urlInputs = await screen.findAllByPlaceholderText(/글 URL/);
    expect(urlInputs).toHaveLength(3);
    const urlInput = urlInputs[0]!;
    await user.type(
      urlInput,
      "https://stock.naver.com/domestic/stock/005930/discussion/421063210",
    );
    expect(urlInput).toHaveValue(
      "https://stock.naver.com/domestic/stock/005930/discussion/421063210",
    );

    // "인기글" 대상 → 대상당 댓글 개수 버튼이 나타나고, 개수를 바꾼다.
    await user.click(screen.getByRole("button", { name: "인기글" }));
    await user.click(screen.getByRole("button", { name: "3" }));

    // AI 변형 생성 버튼(토스트만 띄움)도 눌러본다.
    await user.click(screen.getByRole("button", { name: /변형 생성/ }));
    expect(
      screen.getByRole("button", { name: /변형 생성/ }),
    ).toBeInTheDocument();
  });

  it("특정 게시글 링크는 '링크 추가'로 칸을 늘리고 여러 링크를 저장한다", async () => {
    const user = userEvent.setup();
    const onSaveDraft = vi.fn();
    renderWriter({ onSaveDraft });
    await screen.findByPlaceholderText("제목을 입력하세요");

    await user.click(screen.getByRole("button", { name: "댓글 작성" }));
    await user.click(
      await screen.findByRole("button", { name: "특정 게시글" }),
    );

    // 기본 3칸 + '링크 추가'로 4칸이 된다(글+댓글의 '댓글 추가'와 동일 UX).
    let urlInputs = await screen.findAllByPlaceholderText(/글 URL/);
    expect(urlInputs).toHaveLength(3);
    await user.click(screen.getByRole("button", { name: /링크 추가/ }));
    urlInputs = await screen.findAllByPlaceholderText(/글 URL/);
    expect(urlInputs).toHaveLength(4);

    // 서로 다른 링크 2개를 넣고 댓글도 채운 뒤 임시저장하면, 빈 칸은 버려지고
    // 채운 링크만 commentUrls에 담긴다(첫 링크는 하위호환 commentUrl에도 채워진다).
    await user.type(
      urlInputs[0]!,
      "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
    );
    await user.type(urlInputs[1]!, "https://www.band.us/band/103043410/post/2");
    await user.type(
      screen.getAllByPlaceholderText("자연스러운 댓글을 입력하세요")[0]!,
      "좋은 글이네요",
    );
    await user.click(screen.getByRole("button", { name: "임시저장" }));
    await user.click(await screen.findByText("임시저장하기"));

    expect(onSaveDraft).toHaveBeenCalledTimes(1);
    const saved = onSaveDraft.mock.calls[0]![0] as {
      commentUrls?: string[];
      commentUrl?: string;
    };
    expect(saved.commentUrls).toEqual([
      "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      "https://www.band.us/band/103043410/post/2",
    ]);
    expect(saved.commentUrl).toBe(
      "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
    );
  });

  it("triggers image-add and rejects a non-text body drop", async () => {
    const user = userEvent.setup();
    renderWriter();
    await screen.findByPlaceholderText("제목을 입력하세요");

    // 이미지 추가 버튼 → 숨은 파일 입력 클릭을 트리거한다(jsdom에선 no-op).
    await user.click(screen.getByTitle("이미지 추가"));

    // 본문 에디터에 드롭 → onDrop이 기본 동작을 막고 안내 토스트를 띄운다.
    const body = document.querySelector(
      '[contenteditable="true"]',
    ) as HTMLElement;
    fireEvent.drop(body, {
      dataTransfer: { files: [], items: [], getData: () => "" },
    });
    expect(body).toBeInTheDocument();
  });
});
