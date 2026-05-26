import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue("Hello, test!"),
}));

describe("App", () => {
  it("renders the welcome heading", () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: /welcome to tauri \+ react/i }),
    ).toBeInTheDocument();
  });
});
