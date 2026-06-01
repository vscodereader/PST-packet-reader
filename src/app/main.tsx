import React from "react";
import ReactDOM from "react-dom/client";

import { MacroApp } from "./app-shell";
import { AppProviders } from "./providers";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders>
      <MacroApp />
    </AppProviders>
  </React.StrictMode>,
);
