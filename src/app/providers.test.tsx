import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";

import { AppProviders } from "./providers";

describe("AppProviders", () => {
  it("renders children unmodified", () => {
    render(
      <AppProviders>
        <p>child</p>
      </AppProviders>,
    );
    expect(screen.getByText("child")).toBeInTheDocument();
  });
});
