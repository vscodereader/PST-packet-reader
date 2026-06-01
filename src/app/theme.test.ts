import { describe, it, expect } from "vitest";

import { PLATFORM_COLOR, theme } from "./theme";

describe("theme", () => {
  it("uses blue as the primary color and registers Pretendard", () => {
    expect(theme.primaryColor).toBe("blue");
    expect(theme.fontFamily).toContain("Pretendard");
  });

  it("registers the custom platform color scales", () => {
    expect(theme.colors).toHaveProperty("forum");
    expect(theme.colors).toHaveProperty("naver");
    expect(theme.colors).toHaveProperty("band");
  });
});

describe("PLATFORM_COLOR", () => {
  it("maps every platform id to a Mantine color name", () => {
    expect(PLATFORM_COLOR).toMatchObject({
      forum: "forum",
      naver: "naver",
      band: "band",
      instagram: "pink",
      threads: "dark",
    });
  });
});
