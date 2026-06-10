import { MantineProvider } from "@mantine/core";
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";

import { TimeStepper, type TimeValue } from "./time-stepper";

function renderStepper(value: TimeValue) {
  const onChange = vi.fn();
  render(
    <MantineProvider>
      <TimeStepper value={value} onChange={onChange} />
    </MantineProvider>,
  );
  return onChange;
}

describe("TimeStepper", () => {
  it("shows zero-padded hour and minute", () => {
    renderStepper({ h: 9, m: 5 });
    expect(screen.getByLabelText("시")).toHaveValue("09");
    expect(screen.getByLabelText("분")).toHaveValue("05");
  });

  it("increments the hour by 1", async () => {
    const onChange = renderStepper({ h: 18, m: 0 });
    await userEvent.click(screen.getByLabelText("시 증가"));
    expect(onChange).toHaveBeenCalledWith({ h: 19, m: 0 });
  });

  it("wraps the hour from 23 to 0", async () => {
    const onChange = renderStepper({ h: 23, m: 0 });
    await userEvent.click(screen.getByLabelText("시 증가"));
    expect(onChange).toHaveBeenCalledWith({ h: 0, m: 0 });
  });

  it("increments the minute by 1", async () => {
    const onChange = renderStepper({ h: 0, m: 51 });
    await userEvent.click(screen.getByLabelText("분 증가"));
    expect(onChange).toHaveBeenCalledWith({ h: 0, m: 52 });
  });

  it("wraps the minute from 59 to 0", async () => {
    const onChange = renderStepper({ h: 0, m: 59 });
    await userEvent.click(screen.getByLabelText("분 증가"));
    expect(onChange).toHaveBeenCalledWith({ h: 0, m: 0 });
  });

  it("steps the minute down by 1, wrapping 0 → 59", async () => {
    const onChange = renderStepper({ h: 0, m: 0 });
    await userEvent.click(screen.getByLabelText("분 감소"));
    expect(onChange).toHaveBeenCalledWith({ h: 0, m: 59 });
  });

  it("clamps out-of-range direct input", () => {
    const onChange = renderStepper({ h: 0, m: 0 });
    fireEvent.change(screen.getByLabelText("분"), { target: { value: "99" } });
    expect(onChange).toHaveBeenCalledWith({ h: 0, m: 59 });
  });

  it("treats a cleared field as 0", () => {
    const onChange = renderStepper({ h: 18, m: 30 });
    fireEvent.change(screen.getByLabelText("분"), { target: { value: "" } });
    expect(onChange).toHaveBeenCalledWith({ h: 18, m: 0 });
  });
});
