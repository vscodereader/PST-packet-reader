import { describe, expect, it } from "vitest";

import { deviceKillReq, kindColor, queueKillReq } from "./stop-command";

describe("stop-command kill request builders", () => {
  it("queueKillReq targets a single queue by id (no all flag)", () => {
    const req = queueKillReq("dev-1", "q-9");
    expect(req).toEqual({ deviceId: "dev-1", queueId: "q-9" });
    expect("all" in req).toBe(false);
  });

  it("deviceKillReq targets the whole device (all=true, no queueId)", () => {
    const req = deviceKillReq("dev-2");
    expect(req).toEqual({ deviceId: "dev-2", all: true });
    expect("queueId" in req).toBe(false);
  });
});

describe("kindColor", () => {
  it("maps known kinds to distinct colors", () => {
    expect(kindColor("종토")).toBe("blue");
    expect(kindColor("카페")).toBe("green");
    expect(kindColor("밴드")).toBe("grape");
    expect(kindColor("로그인")).toBe("gray");
  });

  it("falls back for unknown kinds", () => {
    expect(kindColor("무엇")).toBe("indigo");
  });
});
