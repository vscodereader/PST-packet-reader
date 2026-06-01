import { invoke } from "@tauri-apps/api/core";

import type { Account } from "@/shared/bindings/Account";

export type { Account };

/**
 * Typed wrapper around the Rust `accounts` commands over Tauri IPC. Every
 * mutation returns the full updated list, letting the caller replace its state
 * in one step — mirroring the Rust command contract.
 */

export async function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

export async function addAccount(account: Account): Promise<Account[]> {
  return invoke<Account[]>("add_account", { account });
}

export async function updateAccount(account: Account): Promise<Account[]> {
  return invoke<Account[]>("update_account", { account });
}

export async function deleteAccounts(ids: string[]): Promise<Account[]> {
  return invoke<Account[]>("delete_accounts", { ids });
}
