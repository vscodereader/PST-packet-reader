import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ---------------------------------------------------------------------------
// Hoisted mock objects — must be created before vi.mock() factories run
// ---------------------------------------------------------------------------
const mocks = vi.hoisted(() => {
  const mockLocator = {
    fill: vi.fn().mockResolvedValue(undefined),
    click: vi.fn().mockResolvedValue(undefined),
  };
  const mockPage = {
    setViewportSize: vi.fn().mockResolvedValue(undefined),
    goto: vi.fn().mockResolvedValue(undefined),
    locator: vi.fn().mockReturnValue(mockLocator),
    $eval: vi.fn().mockResolvedValue(false),
    $: vi.fn().mockResolvedValue({}),
    url: vi.fn().mockReturnValue("https://naver.com/main"),
    on: vi.fn(),
    off: vi.fn(),
  };
  const mockContext = {
    newPage: vi.fn().mockResolvedValue(mockPage),
    cookies: vi.fn().mockResolvedValue([
      { name: "NID_AUT", domain: ".naver.com" },
      { name: "NID_SES", domain: ".naver.com" },
    ]),
    on: vi.fn(),
    off: vi.fn(),
  };
  const mockBrowser = {
    newContext: vi.fn().mockResolvedValue(mockContext),
    // playwright-extra inspects contexts() and binds events on the wrapped browser
    contexts: vi.fn().mockReturnValue([]),
    close: vi.fn().mockResolvedValue(undefined),
    on: vi.fn(),
    off: vi.fn(),
    isConnected: vi.fn().mockReturnValue(true),
    version: vi.fn().mockReturnValue("120.0"),
  };
  return { mockLocator, mockPage, mockContext, mockBrowser };
});

// playwright-extra is not intercepted by vi.mock for CJS require — skip mocking it.
// Instead, we spy on require('playwright').chromium.launch in the run tests.
vi.mock("puppeteer-extra-plugin-stealth", () => ({
  default: vi.fn(() => ({ name: "stealth" })),
}));

// ---------------------------------------------------------------------------
// Module references used for spying
// ---------------------------------------------------------------------------
// eslint-disable-next-line @typescript-eslint/no-require-imports
const nodeFs = require("fs") as typeof import("fs");
// eslint-disable-next-line @typescript-eslint/no-require-imports
const realPlaywright = require("playwright") as typeof import("playwright");

// ---------------------------------------------------------------------------
// Import module under test (after mocks are registered)
// ---------------------------------------------------------------------------
const loginModule = await import("./naver-login");

const {
  validateInput,
  hasNaverSessionCookies,
  isElementVisible,
  queryVisible,
  detectFailure,
  waitForLogin,
  buildLaunchArgs,
  buildLaunchOptions,
  run,
} = loginModule;

// ---------------------------------------------------------------------------
// validateInput
// ---------------------------------------------------------------------------
describe("validateInput", () => {
  const valid = {
    accountId: "user1",
    id: "user1",
    password: "pass1",
    cookiesPath: "/tmp/cookies.json",
    chromePath: "/usr/bin/chrome",
  };

  it("returns parsed object for valid input", () => {
    expect(validateInput(valid)).toMatchObject({ ...valid, headless: false });
  });

  it("defaults headless to false when omitted", () => {
    expect(validateInput(valid).headless).toBe(false);
  });

  it("sets headless true only when exactly true", () => {
    expect(validateInput({ ...valid, headless: true }).headless).toBe(true);
    expect(validateInput({ ...valid, headless: 1 }).headless).toBe(false);
    expect(validateInput({ ...valid, headless: "true" }).headless).toBe(false);
  });

  it.each(["accountId", "id", "password", "cookiesPath", "chromePath"])(
    "throws when %s is missing",
    (key) => {
      expect(() => validateInput({ ...valid, [key]: undefined })).toThrow();
    },
  );

  it("throws when a required field is an empty string", () => {
    expect(() => validateInput({ ...valid, id: "  " })).toThrow();
  });

  it("throws when input is not an object", () => {
    expect(() => validateInput(null)).toThrow();
    expect(() => validateInput("string")).toThrow();
  });
});

// ---------------------------------------------------------------------------
// hasNaverSessionCookies
// ---------------------------------------------------------------------------
describe("hasNaverSessionCookies", () => {
  const make = (name: string) => ({ name, domain: ".naver.com" });

  it("returns true when both NID_AUT and NID_SES are present", () => {
    expect(hasNaverSessionCookies([make("NID_AUT"), make("NID_SES")])).toBe(
      true,
    );
  });

  it("returns false when only NID_AUT is present", () => {
    expect(hasNaverSessionCookies([make("NID_AUT")])).toBe(false);
  });

  it("returns false when only NID_SES is present", () => {
    expect(hasNaverSessionCookies([make("NID_SES")])).toBe(false);
  });

  it("returns false for empty array", () => {
    expect(hasNaverSessionCookies([])).toBe(false);
  });

  it("ignores cookies from non-naver domains", () => {
    expect(
      hasNaverSessionCookies([
        { name: "NID_AUT", domain: "example.com" },
        { name: "NID_SES", domain: "example.com" },
      ]),
    ).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// isElementVisible
// ---------------------------------------------------------------------------
describe("isElementVisible", () => {
  function makeEl(styles: Partial<CSSStyleDeclaration> = {}, height = 100) {
    const el = document.createElement("div");
    Object.assign(el.style, styles);
    Object.defineProperty(el, "offsetHeight", {
      value: height,
      configurable: true,
    });
    document.body.appendChild(el);
    return el;
  }

  it("returns true for a visible element", () => {
    const el = makeEl();
    expect(isElementVisible(el)).toBe(true);
    document.body.removeChild(el);
  });

  it("returns false when display is none", () => {
    const el = makeEl({ display: "none" });
    expect(isElementVisible(el)).toBe(false);
    document.body.removeChild(el);
  });

  it("returns false when visibility is hidden", () => {
    const el = makeEl({ visibility: "hidden" });
    expect(isElementVisible(el)).toBe(false);
    document.body.removeChild(el);
  });

  it("returns false when offsetHeight is 0", () => {
    const el = makeEl({}, 0);
    expect(isElementVisible(el)).toBe(false);
    document.body.removeChild(el);
  });
});

// ---------------------------------------------------------------------------
// queryVisible
// ---------------------------------------------------------------------------
describe("queryVisible", () => {
  it("returns true when $eval resolves truthy", async () => {
    const page = { $eval: vi.fn().mockResolvedValue(true) };
    expect(await queryVisible(page, "#foo")).toBe(true);
  });

  it("returns false when $eval rejects (element not found)", async () => {
    const page = { $eval: vi.fn().mockRejectedValue(new Error("not found")) };
    expect(await queryVisible(page, "#foo")).toBe(false);
  });

  it("returns false when $eval resolves falsy", async () => {
    const page = { $eval: vi.fn().mockResolvedValue(false) };
    expect(await queryVisible(page, "#foo")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// detectFailure
// ---------------------------------------------------------------------------
describe("detectFailure", () => {
  function makePage(
    overrides: {
      evalResults?: Record<string, boolean>;
      hasLoginForm?: boolean;
      url?: string;
    } = {},
  ) {
    const {
      evalResults = {},
      hasLoginForm = true,
      url = "https://naver.com/main",
    } = overrides;
    return {
      $eval: vi.fn((selector: string) =>
        Promise.resolve(evalResults[selector] ?? false),
      ),
      $: vi.fn().mockResolvedValue(hasLoginForm ? {} : null),
      url: vi.fn().mockReturnValue(url),
    };
  }

  it("returns null when no failure condition is met", async () => {
    expect(await detectFailure(makePage())).toBeNull();
  });

  it("returns id/password error message when #err_common is visible", async () => {
    const page = makePage({ evalResults: { "#err_common": true } });
    expect(await detectFailure(page)).toMatch(/아이디 또는 비밀번호/);
  });

  it("returns captcha error message when #captchaDiv is visible", async () => {
    const page = makePage({ evalResults: { "#captchaDiv": true } });
    expect(await detectFailure(page)).toMatch(/캡챠/);
  });

  it("returns blocked error when on login page but form is missing", async () => {
    const page = makePage({
      hasLoginForm: false,
      url: "https://nid.naver.com/blocked",
    });
    expect(await detectFailure(page)).toMatch(/차단/);
  });

  it("returns null when not on login page even if form is missing", async () => {
    const page = makePage({
      hasLoginForm: false,
      url: "https://www.naver.com/main",
    });
    expect(await detectFailure(page)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// waitForLogin
// ---------------------------------------------------------------------------
describe("waitForLogin", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function makeContext(
    cookieSequence: Array<Array<{ name: string; domain: string }>>,
  ) {
    let call = 0;
    return {
      cookies: vi.fn(() =>
        Promise.resolve(
          cookieSequence[Math.min(call++, cookieSequence.length - 1)],
        ),
      ),
    };
  }

  function makeSuccessCookies() {
    return [
      { name: "NID_AUT", domain: ".naver.com" },
      { name: "NID_SES", domain: ".naver.com" },
    ];
  }

  function makePage(
    overrides: {
      evalResults?: Record<string, boolean>;
      hasLoginForm?: boolean;
      url?: string;
    } = {},
  ) {
    const {
      evalResults = {},
      hasLoginForm = true,
      url = "https://naver.com/main",
    } = overrides;
    return {
      $eval: vi.fn((selector: string) =>
        Promise.resolve(evalResults[selector] ?? false),
      ),
      $: vi.fn().mockResolvedValue(hasLoginForm ? {} : null),
      url: vi.fn().mockReturnValue(url),
    };
  }

  it("resolves immediately when cookies are already present", async () => {
    const context = makeContext([makeSuccessCookies()]);
    const page = makePage();
    const result = await waitForLogin(page, context, 5000);
    expect(result).toEqual(makeSuccessCookies());
  });

  it("polls until cookies appear", async () => {
    const empty: Array<{ name: string; domain: string }> = [];
    const context = makeContext([empty, empty, makeSuccessCookies()]);
    const page = makePage();

    const promise = waitForLogin(page, context, 10000);
    await vi.runAllTimersAsync();
    expect(await promise).toEqual(makeSuccessCookies());
  });

  it("throws when error element becomes visible", async () => {
    const empty: Array<{ name: string; domain: string }> = [];
    const context = makeContext([empty]);
    const page = makePage({ evalResults: { "#err_common": true } });

    await expect(async () => {
      const promise = waitForLogin(page, context, 10000);
      promise.catch(() => {});
      await vi.runAllTimersAsync();
      return promise;
    }).rejects.toThrow(/아이디 또는 비밀번호/);
  });

  it("throws on timeout", async () => {
    const empty: Array<{ name: string; domain: string }> = [];
    const context = { cookies: vi.fn().mockResolvedValue(empty) };
    const page = makePage();

    await expect(async () => {
      const promise = waitForLogin(page, context, 100);
      promise.catch(() => {});
      await vi.runAllTimersAsync();
      return promise;
    }).rejects.toThrow(/timed out/);
  });
});

// ---------------------------------------------------------------------------
// buildLaunchArgs — incognito mode detection (pure function, no browser needed)
// ---------------------------------------------------------------------------
describe("buildLaunchArgs - incognito mode", () => {
  it("contains --incognito so the visible Chrome window opens in incognito mode", () => {
    expect(buildLaunchArgs()).toContain("--incognito");
  });

  it("contains expected baseline flags", () => {
    const args = buildLaunchArgs();
    expect(args).toContain("--no-first-run");
    expect(args).toContain("--no-default-browser-check");
  });
});

describe("buildLaunchOptions - Chrome incognito launch", () => {
  it("uses the branded Chrome channel and the configured Chrome executable", () => {
    expect(
      buildLaunchOptions({
        chromePath: "/path/to/chrome.exe",
        headless: false,
      }),
    ).toMatchObject({
      channel: "chrome",
      executablePath: "/path/to/chrome.exe",
      headless: false,
    });
  });

  it("passes the incognito launch arg using normal ASCII hyphens", () => {
    expect(
      buildLaunchOptions({
        chromePath: "/path/to/chrome.exe",
        headless: false,
      }).args,
    ).toContain("--incognito");
  });
});

// ---------------------------------------------------------------------------
// run — incognito mode: verifies browser interaction uses OTR context
// playwright-extra mutates the browser object in place (saves original refs
// then overwrites with plugin wrappers). We save our vi.fn() refs before
// playwright-extra can replace them so we can track calls via those refs.
// ---------------------------------------------------------------------------
describe("run - incognito mode", () => {
  const validInput = {
    accountId: "user1",
    id: "user1",
    password: "pass1",
    cookiesPath: "/tmp/cookies/user1.json",
    chromePath: "/path/to/chrome.exe",
    headless: false,
  };

  // Saved before playwright-extra overwrites them on the mock objects
  let savedNewContext: ReturnType<typeof vi.fn>;
  let savedNewPage: ReturnType<typeof vi.fn>;
  let savedClose: ReturnType<typeof vi.fn>;

  let launchSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    // Fresh vi.fn() refs — playwright-extra will save these as "originals"
    // and call them through its plugin wrappers.
    savedNewContext = vi.fn().mockResolvedValue(mocks.mockContext);
    savedNewPage = vi.fn().mockResolvedValue(mocks.mockPage);
    savedClose = vi.fn().mockResolvedValue(undefined);

    mocks.mockBrowser.newContext = savedNewContext;
    mocks.mockBrowser.close = savedClose;
    mocks.mockBrowser.contexts = vi.fn().mockReturnValue([]);
    mocks.mockBrowser.on = vi.fn();
    mocks.mockBrowser.off = vi.fn();
    mocks.mockBrowser.isConnected = vi.fn().mockReturnValue(true);
    mocks.mockBrowser.version = vi.fn().mockReturnValue("120.0");

    mocks.mockContext.newPage = savedNewPage;
    mocks.mockContext.cookies = vi.fn().mockResolvedValue([
      { name: "NID_AUT", domain: ".naver.com" },
      { name: "NID_SES", domain: ".naver.com" },
    ]);
    mocks.mockContext.on = vi.fn();

    mocks.mockPage.setViewportSize = vi.fn().mockResolvedValue(undefined);
    mocks.mockPage.goto = vi.fn().mockResolvedValue(undefined);
    mocks.mockPage.locator = vi.fn().mockReturnValue(mocks.mockLocator);
    mocks.mockPage.$eval = vi.fn().mockResolvedValue(false);
    mocks.mockPage.$ = vi.fn().mockResolvedValue({});
    mocks.mockPage.url = vi.fn().mockReturnValue("https://naver.com/main");
    mocks.mockPage.on = vi.fn();
    mocks.mockLocator.fill = vi.fn().mockResolvedValue(undefined);
    mocks.mockLocator.click = vi.fn().mockResolvedValue(undefined);

    vi.spyOn(nodeFs.promises, "readFile").mockImplementation(async () =>
      JSON.stringify(validInput),
    );
    vi.spyOn(nodeFs.promises, "writeFile").mockResolvedValue(
      undefined as never,
    );
    vi.spyOn(nodeFs.promises, "mkdir").mockResolvedValue(undefined as never);

    // playwright-extra's proxy ultimately delegates to playwright.chromium.launch()
    launchSpy = vi
      .spyOn(realPlaywright.chromium, "launch")
      .mockResolvedValue(mocks.mockBrowser as never);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("passes --incognito to chromium.launch", async () => {
    await run("/tmp/input.json");
    const launchArgs: string[] =
      (launchSpy.mock.calls[0] as [{ args: string[] }])[0]?.args ?? [];
    expect(launchArgs).toContain("--incognito");
  });

  it("launches through the branded Chrome channel", async () => {
    await run("/tmp/input.json");
    expect(launchSpy.mock.calls[0]?.[0]).toMatchObject({
      channel: "chrome",
      executablePath: validInput.chromePath,
    });
  });

  it("calls browser.newContext() for OTR isolation", async () => {
    await run("/tmp/input.json");
    expect(savedNewContext).toHaveBeenCalledTimes(1);
  });

  it("creates page within the OTR context, not directly on browser", async () => {
    await run("/tmp/input.json");
    expect(savedNewPage).toHaveBeenCalledTimes(1);
  });

  it("closes the browser after login completes", async () => {
    await run("/tmp/input.json");
    expect(savedClose).toHaveBeenCalledTimes(1);
  });
});
