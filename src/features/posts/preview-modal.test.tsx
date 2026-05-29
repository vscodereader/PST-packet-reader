import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import type { PublishJob } from "@/shared/data/types";

import { PreviewModal } from "./preview-modal";

const forumJob: PublishJob = {
  key: "a1-005930",
  platform: "forum",
  loginId: "invest_king7",
  targetName: "삼성전자",
  code: "005930",
  board: "종목토론방",
  status: "active",
};

function renderPreview(over: Partial<Parameters<typeof PreviewModal>[0]> = {}) {
  render(
    <MantineProvider>
      <PreviewModal
        open
        onClose={vi.fn()}
        mode="post"
        title="#{종목명} 분석"
        body="<p>본문 내용</p>"
        comments={[]}
        jobs={[forumJob]}
        linkOverride=""
        {...over}
      />
    </MantineProvider>,
  );
}

describe("PreviewModal", () => {
  it("resolves template variables in the post title", async () => {
    renderPreview();
    expect(await screen.findByText("삼성전자 분석")).toBeInTheDocument();
    expect(screen.getByText("본문 내용")).toBeInTheDocument();
  });

  it("renders resolved comment samples in comment mode", async () => {
    renderPreview({
      mode: "comment",
      comments: ["좋네요 #{종목명}"],
      commentTarget: "latest",
      commentCount: 3,
    });
    expect(await screen.findByText("좋네요 삼성전자")).toBeInTheDocument();
    expect(screen.getByText("댓글 1")).toBeInTheDocument();
  });

  it("falls back to a placeholder target when no jobs are given", async () => {
    renderPreview({ jobs: [], title: "제목" });
    // appears in both the tab pill and the card header
    expect((await screen.findAllByText("대상 미선택")).length).toBeGreaterThan(
      0,
    );
  });

  it("shows a comment-target line with a url target", async () => {
    renderPreview({
      mode: "comment",
      comments: ["좋아요"],
      commentTarget: "url",
    });
    expect(await screen.findByText(/지정 게시글/)).toBeInTheDocument();
  });

  it("switches between target tabs", async () => {
    const jobB = {
      ...forumJob,
      key: "a2-035720",
      targetName: "카카오",
      code: "035720",
    };
    renderPreview({ jobs: [forumJob, jobB], title: "제목" });
    await userEvent.click(await screen.findByText("카카오"));
    // 카카오 now appears in both the tab pill and the card header
    expect(screen.getAllByText("카카오").length).toBeGreaterThan(1);
  });
});
