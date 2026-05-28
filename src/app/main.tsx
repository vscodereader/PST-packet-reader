import React from "react";
import ReactDOM from "react-dom/client";

import { MacroEditorPage } from "@/features/macro-editor/macro-editor-page";

import { AppProviders } from "./providers";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders>
      <MacroEditorPage />
    </AppProviders>
  </React.StrictMode>,
);