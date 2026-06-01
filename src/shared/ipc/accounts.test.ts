import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Account } from "@/shared/bindings/Account";

import {
  addAccount,
  deleteAccounts,
  listAccounts,
  updateAccount,
} from "./accounts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const mockInvoke = vi.mocked(invoke);

const acc: Account = {
  id: "x1",
  platform: "forum",
  loginId: "tester",
  pw: "pw",
  status: "new",
  last: "—",
  tags: [],
};

describe("accounts ipc wrapper", () => {
  beforeEach(() => mockInvoke.mockReset());

  it("listAccounts invokes list_accounts and returns the result", async () => {
    mockInvoke.mockResolvedValue([acc]);
    await expect(listAccounts()).resolves.toEqual([acc]);
    expect(mockInvoke).toHaveBeenCalledWith("list_accounts");
  });

  it("addAccount invokes add_account with the account", async () => {
    mockInvoke.mockResolvedValue([acc]);
    await addAccount(acc);
    expect(mockInvoke).toHaveBeenCalledWith("add_account", { account: acc });
  });

  it("updateAccount invokes update_account with the account", async () => {
    mockInvoke.mockResolvedValue([acc]);
    await updateAccount(acc);
    expect(mockInvoke).toHaveBeenCalledWith("update_account", { account: acc });
  });

  it("deleteAccounts invokes delete_accounts with the ids", async () => {
    mockInvoke.mockResolvedValue([]);
    await deleteAccounts(["x1", "x2"]);
    expect(mockInvoke).toHaveBeenCalledWith("delete_accounts", {
      ids: ["x1", "x2"],
    });
  });
});
