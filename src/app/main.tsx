import { MantineProvider } from "@mantine/core";
import React from "react";
import ReactDOM from "react-dom/client";

import { MacroEditorPage } from "@/features/macro-editor/macro-editor-page";

import "@mantine/core/styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <MantineProvider>
      <MacroEditorPage />
    </MantineProvider>
  </React.StrictMode>,
);
