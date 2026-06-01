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

  it("snaps to the minimum when the min day is picked with an earlier time", async () => {
    const onChange = renderPicker({
      date: "2026-05-30",
      time: "08:00",
      minDate: new Date(2026, 4, 29, 15, 30),
    });
    await userEvent.click(screen.getByRole("button", { name: /5월 30일/ }));
    // 5/29 is the min day; 08:00 is before 15:30 → snaps up to the minimum.
    await userEvent.click(await screen.findByLabelText("2026-5-29"));
    expect(onChange).toHaveBeenCalledWith({
      date: "2026-05-29",
      time: "15:30",
    });
  });
});
