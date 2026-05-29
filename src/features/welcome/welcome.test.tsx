import { MantineProvider } from "@mantine/core";
import { invoke } from "@tauri-apps/api/core";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { describe, it, expect, vi } from "vitest";

import { Welcome } from "./welcome";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function renderWithMantine(ui: ReactNode) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

describe("Welcome", () => {
  it("renders the welcome heading", () => {
    renderWithMantine(<Welcome />);
    expect(
      screen.getByRole("heading", { name: /welcome to tauri \+ react/i }),
    ).toBeInTheDocument();
  });

  it("submits the form and shows the greet result", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("Hello, Pallas!");
    const user = userEvent.setup();
    renderWithMantine(<Welcome />);

    await user.type(screen.getByPlaceholderText(/enter a name/i), "Pallas");
    await user.click(screen.getByRole("button", { name: /greet/i }));

    expect(invoke).toHaveBeenCalledWith("greet", { name: "Pallas" });
    await waitFor(() => {
      expect(screen.getByText("Hello, Pallas!")).toBeInTheDocument();
    });
  });
});
