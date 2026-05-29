import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";

import type { LogFilter } from "@/shared/data/types";
import { pickOption } from "@/test/select";

import { Notifications } from "./notifications";

function renderLog(filter: LogFilter | null = null) {
  render(
    <MantineProvider>
      <Notifications filter={filter} />
    </MantineProvider>,
  );
}

describe("Notifications", () => {
  it("renders the title and a batch entry", () => {
    renderLog();
    expect(screen.getByRole("heading", { name: "알림" })).toBeInTheDocument();
    expect(screen.getByText("반도체 흐름 코멘트 10종")).toBeInTheDocument();
  });

  it("expands a batch to reveal per-location sub-logs", async () => {
    renderLog();
    await userEvent.click(screen.getByText("5월 이벤트 결과 발표"));
    // sub-log loginId text becomes visible once expanded
    expect(screen.getByText(/invest_king7/)).toBeInTheDocument();
  });

  it("shows the account filter chip when a loginId filter is passed", () => {
    renderLog({ loginId: "value_pick", platform: "forum" });
    expect(screen.getByText("계정 필터 적용됨")).toBeInTheDocument();
    expect(screen.getByText("value_pick")).toBeInTheDocument();
  });

  it("toggles the error trace on a failed sub-log", async () => {
    renderLog();
    await userEvent.click(screen.getByText("반도체 흐름 코멘트 10종"));
    await userEvent.click(
      await screen.findByRole("button", { name: /자세히 보기/ }),
    );
    expect(screen.getByText(/NaverAuthError/)).toBeInTheDocument();
  });

  it("fires the export action", async () => {
    renderLog();
    await userEvent.click(screen.getByRole("button", { name: /내보내기/ }));
    expect(screen.getByRole("heading", { name: "알림" })).toBeInTheDocument();
  });

  it("clears the account filter", async () => {
    renderLog({ loginId: "value_pick", platform: "forum" });
    await userEvent.click(screen.getByRole("button", { name: /필터 해제/ }));
    expect(screen.queryByText("계정 필터 적용됨")).not.toBeInTheDocument();
  });

  it("filters batches by the search box", async () => {
    renderLog();
    await userEvent.type(
      screen.getByPlaceholderText("내용·종목·계정 검색"),
      "카카오",
    );
    expect(screen.getByText("차트 관점 분석")).toBeInTheDocument();
    expect(
      screen.queryByText("반도체 흐름 코멘트 10종"),
    ).not.toBeInTheDocument();
  });

  it("switches to the system category", async () => {
    renderLog();
    await userEvent.click(screen.getByText(/시스템 \d/));
    expect(
      screen.getByText(/엑셀에서 계정 4건을 가져왔습니다/),
    ).toBeInTheDocument();
  });

  it("filters by status via the status select", async () => {
    renderLog();
    await pickOption(0, "실패");
    // a fully-failed batch remains; an all-success one is filtered out
    expect(screen.queryByText("5월 이벤트 결과 발표")).not.toBeInTheDocument();
  });

  it("filters by platform via the platform select", async () => {
    renderLog();
    await pickOption(1, "밴드");
    expect(screen.getByRole("heading", { name: "알림" })).toBeInTheDocument();
  });
});
