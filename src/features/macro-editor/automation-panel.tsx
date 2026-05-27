import {
  Alert,
  Button,
  NumberInput,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";

import type { SavedEntry } from "./types";

type AutomationReport = {
  current_url: string;
  login_profile: {
    logged_in: boolean;
    nickname: string | null;
    image_url: string | null;
    message: string;
  };
  register_button_highlighted: boolean;
  selected: {
    category: string;
    rank: string;
    item_text: string;
    method: string;
  };
};

type AutomationPanelProps = {
  contentDraft: string;
  selectedContents: SavedEntry[];
  selectedTitles: SavedEntry[];
  titleDraft: string;
};

function composeTitle(titleDraft: string, selectedTitles: SavedEntry[]) {
  const draft = titleDraft.trim();

  if (draft.length > 0) {
    return draft;
  }

  return selectedTitles
    .map((entry) => entry.label.trim())
    .filter(Boolean)
    .join(" ");
}

function composeBody(contentDraft: string, selectedContents: SavedEntry[]) {
  const draft = contentDraft.trim();

  if (draft.length > 0) {
    return draft;
  }

  return selectedContents
    .map((entry) => entry.label.trim())
    .filter(Boolean)
    .join("\n\n");
}

export function AutomationPanel({
  contentDraft,
  selectedContents,
  selectedTitles,
  titleDraft,
}: AutomationPanelProps) {
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState<number | string>(9222);
  const [running, setRunning] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  async function runAutomation() {
    const title = composeTitle(titleDraft, selectedTitles);
    const body = composeBody(contentDraft, selectedContents);
    const chromePort = typeof port === "number" ? port : Number(port);

    setMessage("");
    setError("");

    if (!title) {
      setError("제목을 작성하거나 저장된 제목을 선택하세요.");
      return;
    }

    if (!body) {
      setError("내용을 작성하거나 저장된 내용을 선택하세요.");
      return;
    }

    if (!Number.isInteger(chromePort) || chromePort <= 0) {
      setError("Chrome DevTools 포트 번호가 올바르지 않습니다.");
      return;
    }

    setRunning(true);

    try {
      const report = await invoke<AutomationReport>("run_naver_discussion", {
        body,
        host,
        port: chromePort,
        title,
      });

      setMessage(
        [
          `선택 카테고리: ${report.selected.category}`,
          `선택 순위: ${report.selected.rank}`,
          `선택 종목: ${report.selected.item_text}`,
          `로그인 확인: ${report.login_profile.nickname ?? report.login_profile.message}`,
          `현재 URL: ${report.current_url}`,
          report.register_button_highlighted
            ? "등록하기 버튼을 빨간 테두리로 표시했습니다."
            : "등록하기 버튼을 찾지 못했습니다.",
        ].join("\n"),
      );
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setRunning(false);
    }
  }

  return (
    <section className="macro-editor-automation">
      <Stack gap="sm">
        <Text fw={700}>토론방 자동 입력</Text>

        <TextInput
          label="Chrome DevTools host"
          value={host}
          onChange={(event) => setHost(event.currentTarget.value)}
        />

        <NumberInput
          allowDecimal={false}
          allowNegative={false}
          label="Chrome DevTools 포트"
          max={65535}
          min={1}
          value={port}
          onChange={setPort}
        />

        <Button loading={running} onClick={runAutomation}>
          자동 입력 실행
        </Button>

        {error ? (
          <Alert color="red" title="실행 실패">
            <Text component="pre" className="macro-editor-status-text">
              {error}
            </Text>
          </Alert>
        ) : null}

        {message ? (
          <Alert color="green" title="실행 완료">
            <Text component="pre" className="macro-editor-status-text">
              {message}
            </Text>
          </Alert>
        ) : null}
      </Stack>
    </section>
  );
}
