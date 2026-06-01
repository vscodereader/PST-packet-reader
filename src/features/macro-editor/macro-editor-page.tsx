import { Container, Title } from "@mantine/core";
import { useState } from "react";

import { LoginPanel } from "./login-panel";
import { StockBatchPanel } from "./stock-batch-panel";

import "./macro-editor.css";

export function MacroEditorPage() {
  // 로그인 패널과 글쓰기 패널이 같은 계정 ID를 공유한다.
  // 로그인이 성공하면 그 계정 ID가 글쓰기 쪽에 자동으로 채워진다.
  const [accountId, setAccountId] = useState("");

  return (
    <Container fluid className="macro-editor">
      <Title order={1} className="macro-editor-title">
        pstmacro
      </Title>

      <LoginPanel onLoggedIn={setAccountId} />

      <StockBatchPanel accountId={accountId} onAccountIdChange={setAccountId} />
    </Container>
  );
}
