import { ActionIcon, Box, Group, Stack, Text, TextInput } from "@mantine/core";

import { Icon } from "./icons";

export interface TimeValue {
  h: number;
  m: number;
}

export interface TimeStepperProps {
  value: TimeValue;
  onChange: (value: TimeValue) => void;
}

const pad = (n: number) => String(n).padStart(2, "0");
const wrap = (n: number, mod: number) => ((n % mod) + mod) % mod;
const clamp = (n: number, max: number) => Math.max(0, Math.min(max, n));

function Field({
  label,
  value,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
}) {
  const mod = max + 1;
  return (
    <Stack gap={2} align="center">
      <ActionIcon
        variant="subtle"
        color="gray"
        size="sm"
        aria-label={`${label} 증가`}
        onClick={() => onChange(wrap(value + step, mod))}
      >
        <Icon.chevronUp size={15} />
      </ActionIcon>
      <TextInput
        size="sm"
        w={48}
        aria-label={label}
        value={pad(value)}
        onChange={(e) => {
          const digits = e.currentTarget.value.replace(/\D/g, "");
          if (digits === "") {
            onChange(0);
            return;
          }
          onChange(clamp(parseInt(digits, 10), max));
        }}
        styles={{
          input: {
            textAlign: "center",
            fontWeight: 700,
            fontVariantNumeric: "tabular-nums",
          },
        }}
      />
      <ActionIcon
        variant="subtle"
        color="gray"
        size="sm"
        aria-label={`${label} 감소`}
        onClick={() => onChange(wrap(value - step, mod))}
      >
        <Icon.chevronDown size={15} />
      </ActionIcon>
    </Stack>
  );
}

/**
 * Hour/minute picker with up/down steppers (hour ±1, minute ±1, both wrap at
 * the boundary) and direct numeric entry (clamped to range).
 */
export function TimeStepper({ value, onChange }: TimeStepperProps) {
  return (
    <Group gap={6} justify="center" align="center" wrap="nowrap">
      <Field
        label="시"
        value={value.h}
        max={23}
        step={1}
        onChange={(h) => onChange({ ...value, h })}
      />
      <Box pt={20}>
        <Text fz={18} fw={800}>
          :
        </Text>
      </Box>
      <Field
        label="분"
        value={value.m}
        max={59}
        step={1}
        onChange={(m) => onChange({ ...value, m })}
      />
    </Group>
  );
}
