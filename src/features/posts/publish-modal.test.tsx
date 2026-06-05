import { MantineProvider } from "@mantine/core";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import type { LibraryPost } from "@/shared/data/types";
import { invoke as ipcBackend } from "@/test/ipc";
import { pickOption } from "@/test/select";

import { PublishModal } from "./publish-modal";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("@/test/ipc")).invoke,
}));

const postDoc: LibraryPost = {
  id: "l1",
  title: "#{종목명} 4분기 실적 기대",
  kind: "post",
  updated: "방금 전",
  words: 100,
  status: "ready",
  excerpt: "요약",
  body: "<p>#{종목명} 본문 #{링크}</p>",
};

function renderPublish(over: Partial<Parameters<typeof PublishModal>[0]> = {}) {
  const go = vi.fn();
  render(
    <MantineProvider>
      <PublishModal open doc={postDoc} onClose={vi.fn()} go={go} {...over} />
    </MantineProvider>,
  );
  return { go };
}

describe("PublishModal", () => {
  it("renders 게시 설정 with the document title", async () => {
    renderPublish();
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("게시 설정");
    expect(dialog).toHaveTextContent("#{종목명} 4분기 실적 기대");
  });

  it("shows the template-variable substitution card for tokenized docs", async () => {
    renderPublish();
    expect(await screen.findByText("변수 자동 치환")).toBeInTheDocument();
  });

  it("opens the preview from 미리보기", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: "미리보기" }),
    );
    expect(await screen.findByText(/각 게시판 서식으로/)).toBeInTheDocument();
  });

  it("reveals the schedule button when 예약 게시 is chosen", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("예약 게시"));
    expect(
      await screen.findByRole("button", { name: /예약 \(\d+\)/ }),
    ).toBeInTheDocument();
  });

  it("opens the stock crawl modal from 종목 선택", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /종목 선택/ }),
    );
    expect(await screen.findByText(/finance\.naver\.com/)).toBeInTheDocument();
  });

  it("starts the publish flow and shows progress", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    expect(await screen.findByText(/게시하는 중/)).toBeInTheDocument();
  });

  it("completes the publish flow and routes to a follow-up view", async () => {
    vi.spyOn(Math, "random").mockReturnValue(1); // force every job to succeed
    const { go } = renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    // results render after the simulated upload (~2s)
    expect(
      await screen.findByText("계속 작성", undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "알림 보기" }));
    expect(go).toHaveBeenCalledWith("log");
    vi.restoreAllMocks();
  });

  it("offers a retry control when a job fails", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0); // force every job to fail
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /^게시 \(\d+\)/ }),
    );
    expect(
      await screen.findByRole("button", { name: "재시도" }, { timeout: 3000 }),
    ).toBeInTheDocument();
    vi.restoreAllMocks();
  });

  it("publishes naver jobs through the real run_post_jobs command", async () => {
    renderPublish();
    // Swap the preselected forum account for a naver one. Selecting it loads
    // that account's joined cafes; picking a cafe resolves its boards (first
    // board auto-selected), which drives a real backend publish.
    await userEvent.click(await screen.findByText("invest_king7"));
    await userEvent.click(screen.getByText("money_lab"));
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
    // boards resolve and the first board is auto-selected → the naver job
    // becomes valid, so the publish button's count ticks up to 1.
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // The result row renders "{loginId} · {msg}" in one node, so match loosely.
    expect(
      await screen.findByText(/글 게시 완료/, undefined, { timeout: 3000 }),
    ).toBeInTheDocument();
    expect(ipcBackend).toHaveBeenCalledWith(
      "run_post_jobs",
      expect.objectContaining({
        jobs: [
          expect.objectContaining({
            // 백엔드는 쿠키 파일 키(loginId)로 계정을 찾는다 — UI 고유 id("a5")가 아니다.
            accountId: "money_lab",
            cafe: "11111111",
            menuId: 1,
            boardType: "L",
          }),
        ],
      }),
    );
  });

  it("loads an account's joined cafes when a naver account is selected", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("money_lab"));
    // The joined-cafe loader populates the per-account cafe select.
    await screen.findByPlaceholderText("가입 카페 선택");
    // 가입 카페는 쿠키 파일 키(loginId)로 조회해야 한다 — UI 고유 id("a5")로 조회하면
    // 백엔드가 쿠키 파일을 못 찾아 빈 목록을 돌려준다(회귀: 가입 카페가 안 뜨던 버그).
    expect(ipcBackend).toHaveBeenCalledWith("list_joined_cafes", {
      accountId: "money_lab",
    });
    expect(ipcBackend).not.toHaveBeenCalledWith("list_joined_cafes", {
      accountId: "a5",
    });
  });

  it("deselects an account when its row is clicked again", async () => {
    renderPublish();
    expect(await screen.findByText("1개")).toBeInTheDocument();
    // the first usable account (a1) is preselected; click its row to deselect
    await userEvent.click(screen.getByText("invest_king7"));
    expect(await screen.findByText("0개")).toBeInTheDocument();
  });

  it("selects every visible account and expands the destinations", async () => {
    renderPublish();
    await userEvent.click(
      await screen.findByRole("button", { name: /보이는 계정 전체/ }),
    );
    // 15 accounts − 1 errored = 14 selectable
    expect(await screen.findByText("14개")).toBeInTheDocument();
    // forum + naver + band → 3 platform marks in the footer pill row
    expect(
      screen.getByRole("button", { name: /종목 선택/ }),
    ).toBeInTheDocument();
  });

  it("edits the link override when the doc has a link token", async () => {
    renderPublish();
    const input = await screen.findByPlaceholderText(
      "비우면 종목별 시세 링크 자동 삽입",
    );
    await userEvent.type(input, "https://x.test");
    expect(input).toHaveValue("https://x.test");
  });

  it("removes a selected stock chip", async () => {
    renderPublish();
    // "삼성전자" shows in the chip and in the variable-preview example;
    // the chip (first in DOM) carries the remove button.
    const chip = (await screen.findAllByText("삼성전자"))[0]!.closest("div")!;
    await userEvent.click(within(chip).getByRole("button"));
    expect(screen.queryByText("삼성전자")).not.toBeInTheDocument();
  });

  it("shows the custom date/time picker once 예약 게시 is chosen", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("예약 게시"));
    // Native date/time inputs are replaced by the DateTimePicker trigger, which
    // renders the scheduled moment (defaulting to now) as a friendly label.
    expect(
      await screen.findByRole("button", { name: /\d+월 \d+일.*\d\d:\d\d/ }),
    ).toBeInTheDocument();
  });

  it("adds the post to the scheduled queue when 예약 is confirmed", async () => {
    renderPublish();
    await userEvent.click(await screen.findByText("예약 게시"));
    await userEvent.click(
      await screen.findByRole("button", { name: /^예약 \(\d+\)/ }),
    );
    const scheduled = (await ipcBackend("list_queue_scheduled")) as {
      title: string;
    }[];
    expect(scheduled.some((q) => q.title === postDoc.title)).toBe(true);
  });

  it("comments on the just-posted article in 'both' mode", async () => {
    const bothDoc: LibraryPost = {
      id: "lb",
      title: "실적 점검 + 댓글",
      kind: "both",
      updated: "방금 전",
      words: 100,
      status: "ready",
      excerpt: "요약",
      body: "<p>본문</p>",
      comments: ["좋네요"],
    };
    renderPublish({ doc: bothDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    await screen.findByPlaceholderText("가입 카페 선택");
    await pickOption(0, "주식투자연구소 카페");
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // post lands first…
    expect(ipcBackend).toHaveBeenCalledWith("run_post_jobs", expect.anything());
    // …then a comment request for that article (articleId 1000 from the mock,
    // cafeId from the picked joined cafe) is sent. The backend now distributes
    // the comment pool (issue #98), so the front sends the target + pool — the
    // chosen `content` is no longer decided here.
    expect(ipcBackend).toHaveBeenCalledWith(
      "run_comment_jobs",
      expect.objectContaining({
        req: expect.objectContaining({
          targets: [
            expect.objectContaining({
              accountId: "money_lab",
              cafeId: 11111111,
              articleId: 1000,
            }),
          ],
          comments: ["좋네요"],
        }),
      }),
    );
  });

  it("comments on a pasted article URL in 'comment' mode", async () => {
    const commentDoc: LibraryPost = {
      id: "lc",
      title: "URL 댓글 세트",
      kind: "comment",
      updated: "방금 전",
      words: 30,
      status: "ready",
      excerpt: "요약",
      commentTarget: "url",
      commentUrl: "https://cafe.naver.com/ca-fe/cafes/31732304/articles/9",
      comments: ["댓글1", "댓글2"],
    };
    renderPublish({ doc: commentDoc });
    await userEvent.click(await screen.findByText("invest_king7")); // drop forum
    await userEvent.click(screen.getByText("money_lab")); // a5 naver
    // No board pick needed in comment mode — the job is the account itself.
    await userEvent.click(
      await screen.findByRole(
        "button",
        { name: /^게시 \(1\)/ },
        { timeout: 3000 },
      ),
    );
    // Distribution moved to the backend (issue #98): the front just sends one
    // target per account plus the comment pool; which comment each account gets
    // is RNG-chosen in Rust, so it's not asserted here.
    const commentCalls = ipcBackend.mock.calls.filter(
      (c) => c[0] === "run_comment_jobs",
    );
    const call = commentCalls[commentCalls.length - 1];
    expect(call).toBeDefined();
    const req = (
      call?.[1] as { req: { targets: unknown[]; comments: string[] } }
    ).req;
    expect(req.targets).toHaveLength(1);
    expect(req.targets[0]).toEqual(
      expect.objectContaining({
        accountId: "money_lab",
        cafeId: 31732304,
        articleId: 9,
      }),
    );
    expect(req.comments).toEqual(["댓글1", "댓글2"]);
  });

  it("picks a per-account cafe/board and the band destination", async () => {
    renderPublish();
    // Add a naver and a band account alongside the default forum one.
    await userEvent.click(await screen.findByText("money_lab")); // a5 naver
    await userEvent.click(screen.getByText("value_invest")); // a7 band
    await screen.findByPlaceholderText("가입 카페 선택");
    // naver row exposes cafe (0) + board (1); band card adds the band select (2)
    await pickOption(0, "개미투자 카페");
    // forum (a1) + band (a7) = 2 jobs; the naver job lands once its first board
    // is auto-selected, bringing the total to 3.
    await screen.findByRole(
      "button",
      { name: /^게시 \(3\)/ },
      { timeout: 3000 },
    );
    await pickOption(1, "공지사항");
    await pickOption(2, "단타클럽 BAND");
    // Mantine Select keeps a hidden duplicate input, so assert on the visible
    // listbox inputs in order: cafe, board, band.
    const combos = [
      ...document.querySelectorAll<HTMLInputElement>(
        'input[aria-haspopup="listbox"]',
      ),
    ];
    expect(combos[0]).toHaveValue("개미투자 카페");
    expect(combos[1]).toHaveValue("공지사항");
    expect(combos[2]).toHaveValue("단타클럽 BAND");
  });
});
