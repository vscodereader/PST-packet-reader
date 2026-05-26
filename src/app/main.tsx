import React from "react";
import ReactDOM from "react-dom/client";

import { AppProviders } from "./providers";

import { Welcome } from "@/features/welcome/welcome";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders>
      <Welcome />
    </AppProviders>
  </React.StrictMode>,
);
