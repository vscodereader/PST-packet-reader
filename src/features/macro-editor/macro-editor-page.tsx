import { Container, Title } from "@mantine/core";

import { StockBatchPanel } from "./stock-batch-panel";

import "./macro-editor.css";

export function MacroEditorPage() {
  return (
    <Container fluid className="macro-editor">
      <Title order={1} className="macro-editor-title">
        pstmacro
      </Title>

      <StockBatchPanel />
    </Container>
  );
}
