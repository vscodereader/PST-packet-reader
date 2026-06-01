import { invoke } from "@tauri-apps/api/core";

import type { Account } from "@/shared/bindings/Account";
import { ACCOUNTS } from "@/shared/data/mock";

import { isTauri } from "./runtime";

export type { Account };

/**
 * Typed wrapper around the Rust `accounts` commands.
 *
 * Each call goes over Tauri IPC when running in the app; outside Tauri (browser
 * dev, Vitest) it falls back to an in-memory copy of the mock data so the UI
 * stays fully interactive. Every mutation returns the full updated list, letting
 * the caller replace its state in one step — mirroring the Rust command contract.
 */

let fallback: Account[] = ACCOUNTS.map((a) => ({ ...a }));

/** Reset the in-memory fallback to the pristine mock data. Test-only seam. */
export function resetFallbackForTests(): void {
  fallback = ACCOUNTS.map((a) => ({ ...a }));
}

function snapshot(): Account[] {
  return fallback.map((a) => ({ ...a }));
}

export async function listAccounts(): Promise<Account[]> {
  if (isTauri()) return invoke<Account[]>("list_accounts");
  return snapshot();
}

export async function addAccount(account: Account): Promise<Account[]> {
  if (isTauri()) return invoke<Account[]>("add_account", { account });
  fallback = [...fallback, account];
  return snapshot();
}

export async function updateAccount(account: Account): Promise<Account[]> {
  if (isTauri()) return invoke<Account[]>("update_account", { account });
  fallback = fallback.map((a) => (a.id === account.id ? account : a));
  return snapshot();
}

export async function deleteAccounts(ids: string[]): Promise<Account[]> {
  if (isTauri()) return invoke<Account[]>("delete_accounts", { ids });
  fallback = fallback.filter((a) => !ids.includes(a.id));
  return snapshot();
}
