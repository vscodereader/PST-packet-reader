import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
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
});
