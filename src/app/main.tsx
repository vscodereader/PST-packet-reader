import React from "react";
import ReactDOM from "react-dom/client";

import { Welcome } from "@/features/welcome/welcome";

import { AppProviders } from "./providers";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders>
      <Welcome />
    </AppProviders>
  </React.StrictMode>,
);
