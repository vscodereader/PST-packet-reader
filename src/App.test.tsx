import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import App from "@/App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("App", () => {
  it("renders the welcome heading", () => {
    render(<App />);
    expect(
      screen.getByRole("heading", { name: /welcome to tauri \+ react/i }),
    ).toBeInTheDocument();
  });

  it("submits the form and shows the greet result", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("Hello, Pallas!");
    const user = userEvent.setup();
    render(<App />);

    await user.type(screen.getByPlaceholderText(/enter a name/i), "Pallas");
    await user.click(screen.getByRole("button", { name: /greet/i }));

    expect(invoke).toHaveBeenCalledWith("greet", { name: "Pallas" });
    await waitFor(() => {
      expect(screen.getByText("Hello, Pallas!")).toBeInTheDocument();
    });
  });
});
