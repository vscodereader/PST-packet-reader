import "@testing-library/jest-dom";

import { configure } from "@testing-library/react";
import { vi } from "vitest";

// Tauri event bus isn't available in jsdom. Components that call `listen(...)`
// (e.g. app-shell's #400 내용변경 토스트 리스너) would otherwise fire a real IPC
// call and reject. Stub it to resolve with a no-op unsubscribe.
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
  emit: () => Promise.resolve(),
}));

// Heavy Mantine views (e.g. the Accounts table) can take well over a second to
// mount + run their async IPC load on a cold/slow CI runner. Testing Library's
// default `findBy*` / `waitFor` budget is only 1000ms, so the load races the
// timeout and fails flakily. Give async queries real headroom.
configure({ asyncUtilTimeout: 5000 });

// jsdom lacks ResizeObserver; Mantine ScrollArea / Popover rely on it.
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

// jsdom lacks visualViewport; Mantine Textarea `autosize` listens on it and
// throws (addEventListener on undefined) at mount otherwise.
if (typeof window !== "undefined" && !window.visualViewport) {
  Object.defineProperty(window, "visualViewport", {
    writable: true,
    value: {
      width: 1024,
      height: 768,
      addEventListener: () => {},
      removeEventListener: () => {},
    },
  });
}

// jsdom lacks scrollIntoView; Mantine Select/Combobox calls it on open.
if (typeof Element !== "undefined" && !Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}

// jsdom doesn't implement matchMedia; Mantine queries it on mount.
if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}
