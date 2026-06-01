import { Box, Button, Divider, Group, Popover, Text } from "@mantine/core";
import { useState } from "react";

import { Icon } from "./icons";
import { MonthCalendar } from "./month-calendar";
import { TimeStepper } from "./time-stepper";

export interface DateTimePickerProps {
  /** Date as "YYYY-MM-DD". */
  date: string;
  /** Time as "HH:MM". */
  time: string;
  onChange: (next: { date: string; time: string }) => void;
  /** Earliest selectable day (defaults to today). */
  minDate?: Date;
}

const pad = (n: number) => String(n).padStart(2, "0");
const WEEKDAYS = ["일", "월", "화", "수", "목", "금", "토"];

function parseDate(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1);
}
const toISO = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

function parseTime(s: string): { h: number; m: number } {
  const [h, m] = s.split(":").map(Number);
  return { h: h ?? 0, m: m ?? 0 };
}
const toHHMM = (h: number, m: number) => `${pad(h)}:${pad(m)}`;

/**
 * Trigger + popover that picks a date (month grid) and time (stepper + direct
 * entry), keeping the `date`/`time` string contract the caller already uses.
 */
export function DateTimePicker({
  date,
  time,
  onChange,
  minDate,
}: DateTimePickerProps) {
  const [opened, setOpened] = useState(false);
  const d = parseDate(date);
  const t = parseTime(time);
  const min = minDate ?? new Date();
  const label = `${d.getMonth() + 1}월 ${d.getDate()}일 (${WEEKDAYS[d.getDay()]}) ${toHHMM(t.h, t.m)}`;

  return (
    <Popover
      opened={opened}
      onChange={setOpened}
      position="bottom-start"
      withArrow
      shadow="md"
    >
      <Popover.Target>
        <Button
          variant="default"
          justify="space-between"
          leftSection={<Icon.calendar size={16} />}
          rightSection={<Icon.chevronDown size={15} />}
          onClick={() => setOpened((o) => !o)}
          style={{ fontVariantNumeric: "tabular-nums" }}
        >
          {label}
        </Button>
      </Popover.Target>
      <Popover.Dropdown p="sm">
        <Group align="flex-start" gap="md" wrap="nowrap">
          <Box w={220}>
            <MonthCalendar
              value={d}
              minDate={min}
              onChange={(next) => onChange({ date: toISO(next), time })}
            />
          </Box>
          <Divider orientation="vertical" />
          <Box>
            <Text fz={11} c="dimmed" fw={600} ta="center" mb={6}>
              시간
            </Text>
            <TimeStepper
              value={t}
              onChange={(nt) => onChange({ date, time: toHHMM(nt.h, nt.m) })}
            />
          </Box>
        </Group>
        <Group justify="flex-end" mt="sm">
          <Button size="xs" onClick={() => setOpened(false)}>
            확인
          </Button>
        </Group>
      </Popover.Dropdown>
    </Popover>
  );
}
