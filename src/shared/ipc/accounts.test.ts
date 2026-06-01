import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Account } from "@/shared/bindings/Account";

import {
  addAccount,
  deleteAccounts,
  listAccounts,
  resetFallbackForTests,
  updateAccount,
} from "./accounts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

function setTauri(on: boolean) {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
}

const sample: Account = {
  id: "x1",
  platform: "forum",
  loginId: "tester",
  pw: "pw",
  status: "new",
  last: "—",
  tags: [],
};

describe("accounts ipc wrapper", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetFallbackForTests();
  });
  afterEach(() => setTauri(false));

  describe("inside Tauri", () => {
    beforeEach(() => setTauri(true));

    it("listAccounts invokes the list_accounts command", async () => {
      mockInvoke.mockResolvedValue([sample]);
      const result = await listAccounts();
      expect(mockInvoke).toHaveBeenCalledWith("list_accounts");
      expect(result).toEqual([sample]);
    });

    it("addAccount invokes add_account with the account payload", async () => {
      mockInvoke.mockResolvedValue([sample]);
      await addAccount(sample);
      expect(mockInvoke).toHaveBeenCalledWith("add_account", {
        account: sample,
      });
    });

    it("updateAccount invokes update_account with the account payload", async () => {
      mockInvoke.mockResolvedValue([]);
      await updateAccount(sample);
      expect(mockInvoke).toHaveBeenCalledWith("update_account", {
        account: sample,
      });
    });

    it("deleteAccounts invokes delete_accounts with the ids payload", async () => {
      mockInvoke.mockResolvedValue([]);
      await deleteAccounts(["x1"]);
      expect(mockInvoke).toHaveBeenCalledWith("delete_accounts", {
        ids: ["x1"],
      });
    });
  });

  describe("without Tauri (mock fallback)", () => {
    it("listAccounts returns the mock data without calling invoke", async () => {
      const result = await listAccounts();
      expect(mockInvoke).not.toHaveBeenCalled();
      expect(result.length).toBeGreaterThan(0);
    });

    it("addAccount appends and persists the new account", async () => {
      const before = (await listAccounts()).length;
      const afterAdd = await addAccount(sample);
      expect(afterAdd.length).toBe(before + 1);
      expect((await listAccounts()).length).toBe(before + 1);
    });

    it("updateAccount replaces the matching account", async () => {
      const list = await listAccounts();
      const first = list[0];
      expect(first).toBeDefined();
      if (!first) return;
      const edited: Account = { ...first, loginId: "edited" };
      const afterUpdate = await updateAccount(edited);
      expect(afterUpdate.find((a) => a.id === first.id)?.loginId).toBe(
        "edited",
      );
    });

    it("deleteAccounts removes the listed account", async () => {
      const list = await listAccounts();
      const first = list[0];
      expect(first).toBeDefined();
      if (!first) return;
      const afterDelete = await deleteAccounts([first.id]);
      expect(afterDelete.find((a) => a.id === first.id)).toBeUndefined();
    });
  });
});
