import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

import { AppProviders } from "./providers";

describe("AppProviders", () => {
  beforeEach(() => {
    // ErrorBoundary's React-internal log when we test the throw path.
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders children unmodified on the happy path", () => {
    render(
      <AppProviders>
        <p>child</p>
      </AppProviders>,
    );
    expect(screen.getByText("child")).toBeInTheDocument();
  });

  it("catches a thrown child via the shared AppErrorBoundary", () => {
    function Bomb(): never {
      throw new Error("from-child");
    }
    render(
      <AppProviders>
        <Bomb />
      </AppProviders>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/from-child/);
  });
});
