import { MantineProvider } from "@mantine/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { MonthCalendar } from "./month-calendar";

function renderCal(props: Partial<Parameters<typeof MonthCalendar>[0]> = {}) {
  const onChange = vi.fn();
  render(
    <MantineProvider>
      <MonthCalendar
        value={new Date(2026, 4, 29)}
        minDate={new Date(2026, 4, 1)}
        onChange={onChange}
        {...props}
      />
    </MantineProvider>,
  );
  return onChange;
}

describe("MonthCalendar", () => {
  it("renders the month of the selected value", () => {
    renderCal();
    expect(screen.getByText("2026년 5월")).toBeInTheDocument();
  });

  it("emits the clicked day", async () => {
    const onChange = renderCal();
    await userEvent.click(screen.getByLabelText("2026-5-15"));
    expect(onChange).toHaveBeenCalledTimes(1);
    const d = onChange.mock.calls[0]![0] as Date;
    expect([d.getFullYear(), d.getMonth(), d.getDate()]).toEqual([2026, 4, 15]);
  });

  it("disables days before minDate", async () => {
    const onChange = renderCal({ minDate: new Date(2026, 4, 10) });
    const day5 = screen.getByLabelText("2026-5-5");
    expect(day5).toBeDisabled();
    await userEvent.click(day5);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("navigates to the previous month without changing the value", async () => {
    const onChange = renderCal();
    await userEvent.click(screen.getByLabelText("이전 달"));
    expect(screen.getByText("2026년 4월")).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });
});
