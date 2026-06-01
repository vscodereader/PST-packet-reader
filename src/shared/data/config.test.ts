import { describe, it, expect } from "vitest";

import {
  ACTIVE_PLATFORMS,
  KIND,
  PLATFORM,
  PLATFORMS,
  STATUS_ACCOUNT,
  STATUS_ACCOUNT_ORDER,
} from "./config";

describe("platform config", () => {
  it("defines the five platforms", () => {
    expect(PLATFORMS.map((p) => p.id)).toEqual([
      "forum",
      "naver",
      "band",
      "instagram",
      "threads",
    ]);
  });

  it("PLATFORM indexes platforms by id", () => {
    expect(PLATFORM.forum?.name).toBe("종합토론방");
    expect(PLATFORM.naver?.color).toBe("naver");
  });

  it("ACTIVE_PLATFORMS is exactly the non-'soon' platforms", () => {
    expect(ACTIVE_PLATFORMS.every((p) => !p.soon)).toBe(true);
    expect(ACTIVE_PLATFORMS).toEqual(PLATFORMS.filter((p) => !p.soon));
  });
});

describe("label tables", () => {
  it("KIND covers post/comment/both", () => {
    expect(Object.keys(KIND).sort()).toEqual(["both", "comment", "post"]);
  });

  it("STATUS_ACCOUNT order matches its label map keys", () => {
    expect([...STATUS_ACCOUNT_ORDER].sort()).toEqual(
      Object.keys(STATUS_ACCOUNT).sort(),
    );
  });
});
