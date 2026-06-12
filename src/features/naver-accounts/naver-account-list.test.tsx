import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi
    .fn()
    .mockResolvedValue({ isRunning: false, currentAccountId: null, jobs: [] }),
}));

import { NaverAccountList } from "./naver-account-list";

describe("NaverAccountList", () => {
  it("renders the heading", () => {
    render(<NaverAccountList />);
    expect(
      screen.getByRole("heading", { name: /naver account manager/i }),
    ).toBeInTheDocument();
  });

  it("renders username and password input fields", () => {
    render(<NaverAccountList />);
    expect(screen.getByPlaceholderText("Username")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Password")).toBeInTheDocument();
  });

  it("disables add button when inputs are empty", () => {
    render(<NaverAccountList />);
    expect(screen.getByRole("button", { name: /\+/ })).toBeDisabled();
  });

  it("disables add button when only username is filled", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "testuser");
    expect(screen.getByRole("button", { name: /\+/ })).toBeDisabled();
  });

  it("enables add button when both fields are filled", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "testuser");
    await user.type(screen.getByPlaceholderText("Password"), "testpass");
    expect(screen.getByRole("button", { name: /\+/ })).toBeEnabled();
  });

  it("shows added account in list and clears inputs", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "myid");
    await user.type(screen.getByPlaceholderText("Password"), "mypass");
    await user.click(screen.getByRole("button", { name: /\+/ }));

    expect(screen.getByText("myid")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Username")).toHaveValue("");
    expect(screen.getByPlaceholderText("Password")).toHaveValue("");
  });

  it("removes account when delete button is clicked", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "myid");
    await user.type(screen.getByPlaceholderText("Password"), "mypass");
    await user.click(screen.getByRole("button", { name: /\+/ }));

    await user.click(screen.getByRole("button", { name: /Remove myid/ }));
    expect(screen.queryByText("myid")).not.toBeInTheDocument();
  });

  it("disables add button after 10 accounts are added", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);

    for (let i = 1; i <= 10; i++) {
      await user.clear(screen.getByPlaceholderText("Username"));
      await user.clear(screen.getByPlaceholderText("Password"));
      await user.type(screen.getByPlaceholderText("Username"), `user${i}`);
      await user.type(screen.getByPlaceholderText("Password"), `pass${i}`);
      await user.click(screen.getByRole("button", { name: /\+/ }));
    }

    await user.type(screen.getByPlaceholderText("Username"), "extra");
    await user.type(screen.getByPlaceholderText("Password"), "extra");
    expect(screen.getByRole("button", { name: /\+/ })).toBeDisabled();
  });

  it("does not show run-all button when no accounts", () => {
    render(<NaverAccountList />);
    expect(
      screen.queryByRole("button", { name: /run all auto login/i }),
    ).not.toBeInTheDocument();
  });

  it("shows run-all button when at least one account exists", async () => {
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "testuser");
    await user.type(screen.getByPlaceholderText("Password"), "testpass");
    await user.click(screen.getByRole("button", { name: /\+/ }));
    expect(
      screen.getByRole("button", { name: /run all auto login/i }),
    ).toBeInTheDocument();
  });

  it("calls save_accounts and enqueue_cookie_refresh when run-all is clicked", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "myid");
    await user.type(screen.getByPlaceholderText("Password"), "mypass");
    await user.click(screen.getByRole("button", { name: /\+/ }));

    await user.click(
      screen.getByRole("button", { name: /run all auto login/i }),
    );

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_accounts", {
        accounts: [{ id: "myid", password: "mypass", label: "" }],
      });
      expect(invoke).toHaveBeenCalledWith("enqueue_cookie_refresh", {
        accountIds: ["myid"],
        headless: false,
        useAdb: true,
      });
    });
  });

  it("shows a login error when enqueue rejects", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined) // save_accounts
      .mockRejectedValueOnce(new Error("로그인 시작 실패")); // enqueue_cookie_refresh
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "myid");
    await user.type(screen.getByPlaceholderText("Password"), "mypass");
    await user.click(screen.getByRole("button", { name: /\+/ }));
    await user.click(
      screen.getByRole("button", { name: /run all auto login/i }),
    );

    expect(await screen.findByText("로그인 시작 실패")).toBeInTheDocument();
  });

  it("renders a failed job's status badge and error message", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined) // save_accounts
      .mockResolvedValueOnce({
        isRunning: false,
        currentAccountId: null,
        jobs: [{ accountId: "myid", status: "failed", message: "쿠키 만료" }],
      }); // enqueue_cookie_refresh
    const user = userEvent.setup();
    render(<NaverAccountList />);
    await user.type(screen.getByPlaceholderText("Username"), "myid");
    await user.type(screen.getByPlaceholderText("Password"), "mypass");
    await user.click(screen.getByRole("button", { name: /\+/ }));
    await user.click(
      screen.getByRole("button", { name: /run all auto login/i }),
    );

    expect(await screen.findByText("Failed")).toBeInTheDocument();
    expect(screen.getByText("쿠키 만료")).toBeInTheDocument();
  });
});
