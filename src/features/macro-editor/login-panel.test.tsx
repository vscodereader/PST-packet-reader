import { MantineProvider } from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { LoginPanel } from "./login-panel";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderLoginPanel(onLoggedIn = vi.fn()) {
  render(
    <MantineProvider>
      <LoginPanel onLoggedIn={onLoggedIn} />
    </MantineProvider>,
  );
  return onLoggedIn;
}

describe("LoginPanel", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("renders the login fields and button", () => {
    renderLoginPanel();

    expect(
      screen.getByText("1단계 · 네이버 로그인 자동화"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "로그인" })).toBeInTheDocument();
  });

  it("shows a validation error when id or password is empty", async () => {
    const user = userEvent.setup();
    renderLoginPanel();

    await user.click(screen.getByRole("button", { name: "로그인" }));

    expect(
      await screen.findByText("계정 ID와 비밀번호를 입력하세요."),
    ).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith(
      "enqueue_cookie_refresh",
      expect.anything(),
    );
  });

  it("starts login by saving the account and enqueuing a cookie refresh", async () => {
    const user = userEvent.setup();
    renderLoginPanel();

    await user.type(screen.getByLabelText("네이버 ID"), "tester");
    await user.type(screen.getByLabelText("비밀번호"), "secret");
    await user.click(screen.getByRole("button", { name: "로그인" }));

    expect(invoke).toHaveBeenCalledWith("save_accounts", {
      accounts: [{ id: "tester", password: "secret", label: "tester" }],
    });
    expect(invoke).toHaveBeenCalledWith("enqueue_cookie_refresh", {
      accountIds: ["tester"],
      headless: false,
      useAdb: false,
    });
  });
});
