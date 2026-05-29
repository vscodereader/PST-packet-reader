import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect } from "vitest";

import type { LogFilter } from "@/shared/data/types";

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
});
