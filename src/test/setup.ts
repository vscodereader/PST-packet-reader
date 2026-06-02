import "@testing-library/jest-dom";

import { configure } from "@testing-library/react";

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
