import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { DateTimePicker } from "./date-time-picker";

function renderPicker(
  over: Partial<Parameters<typeof DateTimePicker>[0]> = {},
) {
  const onChange = vi.fn();
  render(
    <MantineProvider>
      <DateTimePicker
        date="2026-05-29"
        time="18:00"
        minDate={new Date(2026, 4, 1)}
        onChange={onChange}
        {...over}
      />
    </MantineProvider>,
  );
  return onChange;
}

describe("DateTimePicker", () => {
  it("renders a friendly trigger label", () => {
    renderPicker();
    expect(
      screen.getByRole("button", { name: /5월 29일.*18:00/ }),
    ).toBeInTheDocument();
  });

  it("emits an ISO date when a day is picked", async () => {
    const onChange = renderPicker();
    await userEvent.click(screen.getByRole("button", { name: /5월 29일/ }));
    await userEvent.click(await screen.findByLabelText("2026-5-20"));
    expect(onChange).toHaveBeenCalledWith({
      date: "2026-05-20",
      time: "18:00",
    });
  });

  it("emits HH:MM when the time changes", async () => {
    const onChange = renderPicker();
    await userEvent.click(screen.getByRole("button", { name: /5월 29일/ }));
    await userEvent.click(await screen.findByLabelText("시 증가"));
    expect(onChange).toHaveBeenCalledWith({
      date: "2026-05-29",
      time: "19:00",
    });
  });
});
