import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

import "./band-account-list.css";

type BandAccount = {
  id: string;
  password: string;
};

type QueueJobStatus = "pending" | "expired" | "running" | "success" | "failed";

type QueueJob = {
  accountId: string;
  status: QueueJobStatus;
  message: string;
};

type QueueStatus = {
  isRunning: boolean;
  currentAccountId: string | null;
  jobs: QueueJob[];
};

const STATUS_LABEL: Record<QueueJobStatus, string> = {
  pending: "Pending",
  expired: "Expired",
  running: "Running",
  success: "Done",
  failed: "Failed",
};

export function BandAccountList() {
  const [accounts, setAccounts] = useState<BandAccount[]>([]);
  const [inputId, setInputId] = useState("");
  const [inputPassword, setInputPassword] = useState("");
  const [queueStatus, setQueueStatus] = useState<QueueStatus | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [isStarting, setIsStarting] = useState(false);

  const canAdd =
    inputId.trim() !== "" &&
    inputPassword.trim() !== "" &&
    accounts.length < 10;
  const canRunAll =
    accounts.length > 0 && !queueStatus?.isRunning && !isStarting;

  useEffect(() => {
    if (!queueStatus?.isRunning) return;
    const id = setInterval(() => {
      invoke<QueueStatus>("get_band_queue_status")
        .then(setQueueStatus)
        .catch(() => undefined);
    }, 2000);
    return () => clearInterval(id);
  }, [queueStatus?.isRunning]);

  function addAccount() {
    setAccounts((prev) => [
      ...prev,
      { id: inputId.trim(), password: inputPassword.trim() },
    ]);
    setInputId("");
    setInputPassword("");
  }

  function removeAccount(index: number) {
    setAccounts((prev) => prev.filter((_, i) => i !== index));
  }

  async function runAllLogins() {
    setIsStarting(true);
    setLoginError(null);
    try {
      await invoke("save_accounts", {
        accounts: accounts.map((a) => ({
          id: a.id,
          password: a.password,
          label: "",
        })),
      });
      const status = await invoke<QueueStatus>("enqueue_band_login", {
        accountIds: accounts.map((a) => a.id),
        headless: false,
        useAdb: false,
      });
      setQueueStatus(status);
    } catch (err) {
      setLoginError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsStarting(false);
    }
  }

  function getJobStatus(accountId: string): QueueJob | undefined {
    return [...(queueStatus?.jobs ?? [])]
      .reverse()
      .find((j: QueueJob) => j.accountId === accountId);
  }

  return (
    <main className="container">
      <h1>Band Account Manager</h1>

      <div className="account-form row">
        <input
          placeholder="Username"
          value={inputId}
          onChange={(e) => setInputId(e.currentTarget.value)}
        />
        <input
          type="password"
          placeholder="Password"
          value={inputPassword}
          onChange={(e) => setInputPassword(e.currentTarget.value)}
        />
        <button type="button" disabled={!canAdd} onClick={addAccount}>
          +
        </button>
      </div>

      {accounts.length > 0 && (
        <ul className="account-list">
          {accounts.map((account, index) => {
            const job = getJobStatus(account.id);
            return (
              <>
                <li key={index} className="account-item">
                  <span className="account-id">{account.id}</span>
                  <span className="account-password">{"*".repeat(8)}</span>
                  <span
                    className={
                      job
                        ? `status-badge status-badge-${job.status}`
                        : "status-badge status-badge-empty"
                    }
                    title={job?.status === "failed" ? job.message : undefined}
                  >
                    {job ? STATUS_LABEL[job.status] : "—"}
                  </span>
                  <button
                    type="button"
                    className="delete-btn"
                    onClick={() => removeAccount(index)}
                    aria-label={`Remove ${account.id}`}
                    disabled={queueStatus?.isRunning}
                  >
                    ×
                  </button>
                </li>
                {job?.status === "failed" && (
                  <li className="account-error">{job.message}</li>
                )}
              </>
            );
          })}
        </ul>
      )}

      {accounts.length >= 10 && (
        <p className="account-limit">Maximum 10 accounts reached.</p>
      )}

      {accounts.length > 0 && (
        <button
          type="button"
          className="run-all-btn"
          disabled={!canRunAll}
          onClick={() => void runAllLogins()}
        >
          {isStarting
            ? "Starting..."
            : queueStatus?.isRunning
              ? "Login in progress..."
              : "Run All Auto Login"}
        </button>
      )}

      {loginError && <p className="login-error">{loginError}</p>}
    </main>
  );
}
