import { ActionIcon, Box, Group, Text, UnstyledButton } from "@mantine/core";
import { useState } from "react";

import { Icon } from "./icons";

export interface MonthCalendarProps {
  /** Currently selected day. */
  value: Date;
  onChange: (date: Date) => void;
  /** Earliest selectable day (inclusive); earlier days are disabled. */
  minDate?: Date;
}

const WEEKDAYS = ["일", "월", "화", "수", "목", "금", "토"];

const startOfDay = (d: Date) =>
  new Date(d.getFullYear(), d.getMonth(), d.getDate());
const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() &&
  a.getMonth() === b.getMonth() &&
  a.getDate() === b.getDate();

/**
 * A standalone month-grid date picker built on pure date math (no date lib).
 * The displayed month is internal state; selecting a day calls `onChange`.
 */
export function MonthCalendar({
  value,
  onChange,
  minDate,
}: MonthCalendarProps) {
  const [shown, setShown] = useState(
    () => new Date(value.getFullYear(), value.getMonth(), 1),
  );
  const today = startOfDay(new Date());
  const min = minDate ? startOfDay(minDate) : undefined;

  const year = shown.getFullYear();
  const month = shown.getMonth();
  const leading = new Date(year, month, 1).getDay(); // 0 = Sun
  const daysInMonth = new Date(year, month + 1, 0).getDate();

  const cells: (number | null)[] = [
    ...Array<null>(leading).fill(null),
    ...Array.from({ length: daysInMonth }, (_, i) => i + 1),
  ];

  const shiftMonth = (delta: number) =>
    setShown(new Date(year, month + delta, 1));

  return (
    <Box>
      <Group justify="space-between" mb={8} px={4}>
        <ActionIcon
          variant="subtle"
          color="gray"
          size="sm"
          aria-label="이전 달"
          onClick={() => shiftMonth(-1)}
        >
          <Icon.chevronLeft size={16} />
        </ActionIcon>
        <Text fz={13} fw={700}>
          {year}년 {month + 1}월
        </Text>
        <ActionIcon
          variant="subtle"
          color="gray"
          size="sm"
          aria-label="다음 달"
          onClick={() => shiftMonth(1)}
        >
          <Icon.chevronRight size={16} />
        </ActionIcon>
      </Group>

      <Box
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(7, 1fr)",
          gap: 2,
        }}
      >
        {WEEKDAYS.map((w) => (
          <Text key={w} ta="center" fz={11} c="dimmed" fw={600} py={2}>
            {w}
          </Text>
        ))}
        {cells.map((day, i) => {
          if (day === null) return <Box key={`b${i}`} />;
          const date = new Date(year, month, day);
          const disabled = !!min && date < min;
          const selected = sameDay(date, value);
          const isToday = sameDay(date, today);
          return (
            <UnstyledButton
              key={day}
              disabled={disabled}
              aria-label={`${year}-${month + 1}-${day}`}
              aria-pressed={selected}
              onClick={() => onChange(date)}
              style={{
                height: 30,
                borderRadius: "var(--mantine-radius-sm)",
                fontSize: 12.5,
                fontWeight: selected ? 800 : 500,
                textAlign: "center",
                cursor: disabled ? "not-allowed" : "pointer",
                color: disabled
                  ? "var(--mantine-color-gray-4)"
                  : selected
                    ? "#fff"
                    : "var(--mantine-color-gray-8)",
                background: selected
                  ? "var(--mantine-color-blue-filled)"
                  : "transparent",
                border: isToday
                  ? "1px solid var(--mantine-color-blue-filled)"
                  : "1px solid transparent",
              }}
            >
              {day}
            </UnstyledButton>
          );
        })}
      </Box>
    </Box>
  );
}
