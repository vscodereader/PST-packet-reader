import { createTheme, type MantineColorsTuple } from "@mantine/core";

// Brand accent colors from the design handoff (index.html CSS vars).
// Mantine's default `blue[6]` is already #228be6, so primaryColor stays "blue".
const forum: MantineColorsTuple = [
  "#f3f0ff",
  "#e5dbff",
  "#d0bfff",
  "#b197fc",
  "#9775fa",
  "#845ef7",
  "#7048e8",
  "#6741d9",
  "#5f3dc4",
  "#5235ab",
];

const naver: MantineColorsTuple = [
  "#e8f9ef",
  "#d3f3e0",
  "#a8e6c1",
  "#7ad9a0",
  "#52cd84",
  "#36c672",
  "#03c75a",
  "#00b34e",
  "#009f44",
  "#008a39",
];

const band: MantineColorsTuple = [
  "#e7f7ee",
  "#cfeede",
  "#a3ddbf",
  "#74cb9e",
  "#4dbc83",
  "#34b372",
  "#19b35c",
  "#119e4f",
  "#088a44",
  "#007637",
];

export const theme = createTheme({
  primaryColor: "blue",
  // @fontsource/pretendard registers the family "Pretendard" (imported in
  // providers.tsx). Lead with it so the loaded face is the one that applies.
  fontFamily:
    "Pretendard, -apple-system, BlinkMacSystemFont, system-ui, 'Segoe UI', Roboto, sans-serif",
  defaultRadius: "sm",
  colors: { forum, naver, band },
});

// Maps a platform id to its Mantine theme color name.
export const PLATFORM_COLOR: Record<string, string> = {
  forum: "forum",
  naver: "naver",
  band: "band",
  instagram: "pink",
  threads: "dark",
};
