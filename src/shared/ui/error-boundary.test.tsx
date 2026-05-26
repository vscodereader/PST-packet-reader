import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

import { AppErrorBoundary } from "./error-boundary";

describe("AppErrorBoundary", () => {
  beforeEach(() => {
    // React still logs the caught error to console.error; silence it.
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders children when they don't throw", () => {
    render(
      <AppErrorBoundary>
        <p>healthy</p>
      </AppErrorBoundary>,
    );
    expect(screen.getByText("healthy")).toBeInTheDocument();
  });

  it("shows the fallback with the error message when a child throws", () => {
    function Bomb(): never {
      throw new Error("kaboom");
    }
    render(
      <AppErrorBoundary>
        <Bomb />
      </AppErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/kaboom/);
    expect(
      screen.getByRole("button", { name: /try again/i }),
    ).toBeInTheDocument();
  });

  it("renders fallback for non-Error throw values", () => {
    function StringBomb(): never {
      throw "raw string";
    }
    render(
      <AppErrorBoundary>
        <StringBomb />
      </AppErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(/raw string/);
  });

  it("resets when the Try again button is clicked", async () => {
    let shouldThrow = true;
    function MaybeBomb() {
      if (shouldThrow) throw new Error("first time");
      return <p>recovered</p>;
    }
    render(
      <AppErrorBoundary>
        <MaybeBomb />
      </AppErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();

    shouldThrow = false;
    await userEvent.click(screen.getByRole("button", { name: /try again/i }));
    expect(screen.getByText("recovered")).toBeInTheDocument();
  });
});
