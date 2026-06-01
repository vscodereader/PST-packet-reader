import {
  Alert,
  Button,
  Group,
  PasswordInput,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

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

// 로그인 자동화를 실행하고, 성공하면 그 계정 ID를 글쓰기 쪽으로 넘기는 화면 컴포넌트입니다.
export function LoginPanel({
  onLoggedIn,
}: {
  onLoggedIn: (accountId: string) => void;
}) {
  const [id, setId] = useState("");
  const [password, setPassword] = useState("");
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const pollRef = useRef<number | null>(null);

  // 컴포넌트가 사라질 때 폴링 타이머를 정리합니다.
  useEffect(() => {
    return () => {
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
    };
  }, []);

  // get_queue_status를 주기적으로 확인해 해당 계정의 로그인 결과를 기다리는 함수입니다.
  function pollStatus(account: string) {
    if (pollRef.current !== null) window.clearInterval(pollRef.current);

    pollRef.current = window.setInterval(() => {
      void invoke<QueueStatus>("get_queue_status")
        .then((status) => {
          const job = [...status.jobs]
            .reverse()
            .find((item) => item.accountId === account);

          if (!job || job.status === "pending" || job.status === "running") {
            return;
          }

          if (pollRef.current !== null) {
            window.clearInterval(pollRef.current);
            pollRef.current = null;
          }
          setRunning(false);

          if (job.status === "success") {
            setMessage(
              `로그인 성공: ${account}. 아래 글쓰기에서 이 계정으로 작성합니다.`,
            );
            onLoggedIn(account);
          } else {
            setError(`로그인 실패(${job.status}): ${job.message}`);
          }
        })
        .catch((caught) => {
          if (pollRef.current !== null) {
            window.clearInterval(pollRef.current);
            pollRef.current = null;
          }
          setRunning(false);
          setError(caught instanceof Error ? caught.message : String(caught));
        });
    }, 2000);
  }

  // 로그인 버튼을 눌렀을 때 계정 저장 → 로그인 큐 실행 → 결과 폴링을 시작하는 함수입니다.
  async function startLogin() {
    const account = id.trim();

    if (!account || !password) {
      setError("계정 ID와 비밀번호를 입력하세요.");
      return;
    }

    setError("");
    setMessage("");
    setRunning(true);

    try {
      await invoke("bootstrap_runtime");
      await invoke("save_accounts", {
        accounts: [{ id: account, password, label: account }],
      });
      await invoke("enqueue_cookie_refresh", {
        accountIds: [account],
        headless: false,
        useAdb: false,
      });
      pollStatus(account);
    } catch (caught) {
      setRunning(false);
      setError(caught instanceof Error ? caught.message : String(caught));
    }
  }

  return (
    <section className="macro-editor-login">
      <Stack gap="sm">
        <Text fw={800}>1단계 · 네이버 로그인 자동화</Text>
        <Text size="xs" c="dimmed" className="macro-editor-field-guide">
          계정 ID와 비밀번호를 입력하고 로그인하면, 그 세션 쿠키로 아래 글쓰기가
          작성됩니다. 캡챠나 2차 인증이 뜨면 열린 Chrome 창에서 직접 완료하세요.
        </Text>
        <Group align="end" className="macro-editor-login-row">
          <TextInput
            label="네이버 ID"
            placeholder="naver_id"
            value={id}
            onChange={(event) => setId(event.currentTarget.value)}
          />
          <PasswordInput
            label="비밀번호"
            placeholder="비밀번호"
            value={password}
            onChange={(event) => setPassword(event.currentTarget.value)}
          />
          <Button loading={running} onClick={() => void startLogin()}>
            로그인
          </Button>
        </Group>

        {error ? (
          <Alert color="red" title="확인 필요">
            <Text component="pre" className="macro-editor-status-text">
              {error}
            </Text>
          </Alert>
        ) : null}

        {message ? (
          <Alert color="green" title="상태">
            <Text component="pre" className="macro-editor-status-text">
              {message}
            </Text>
          </Alert>
        ) : null}
      </Stack>
    </section>
  );
}
