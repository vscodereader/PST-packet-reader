import { promises as fsPromises } from "node:fs";
import path from "node:path";

import { addExtra } from "playwright-extra";
import playwright, {
  type Browser,
  type BrowserContext,
  type BrowserContextOptions,
  type Cookie,
  type Page,
} from "playwright";
import StealthPlugin from "puppeteer-extra-plugin-stealth";

type RawInput = Record<string, unknown>;

export type LoginInput = {
  accountId: string;
  id: string;
  password: string;
  cookiesPath: string;
  headless: boolean;
  chromePath: string;
};

type VisibleQueryPage = {
  $eval: (selector: string, pageFunction: (el: Element) => boolean) => Promise<boolean>;
};

type FailureDetectionPage = VisibleQueryPage & {
  $: (selector: string) => Promise<unknown>;
  url: () => string;
};

type CookieContext = {
  cookies: BrowserContext["cookies"];
};

const chromium = addExtra(playwright.chromium);
const stealthPlugin = StealthPlugin();
if (stealthPlugin) {
  chromium.use(stealthPlugin);
}

const LOGIN_URL = "https://nid.naver.com/nidlogin.login";
const SUCCESS_COOKIE_NAMES = new Set(["NID_AUT", "NID_SES"]);

const ERROR_SELECTOR = "#err_common";
const LOGIN_FAIL_TIMEOUT_MS = 3000;

function requiredString(input: RawInput, key: string): string {
  const value = input[key];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${key} must be a non-empty string`);
  }
  return value;
}

export function validateInput(input: unknown): LoginInput {
  if (!input || typeof input !== "object") {
    throw new Error("input must be an object");
  }
  const rawInput = input as RawInput;
  for (const key of [
    "accountId",
    "id",
    "password",
    "cookiesPath",
    "chromePath",
  ]) {
    requiredString(rawInput, key);
  }
  return {
    accountId: requiredString(rawInput, "accountId"),
    id: requiredString(rawInput, "id"),
    password: requiredString(rawInput, "password"),
    cookiesPath: requiredString(rawInput, "cookiesPath"),
    headless: rawInput.headless === true,
    chromePath: requiredString(rawInput, "chromePath"),
  };
}

export function hasNaverSessionCookies(cookies: Array<Pick<Cookie, "name" | "domain">>): boolean {
  const names = new Set(
    cookies.filter((c) => c.domain.includes("naver.com")).map((c) => c.name),
  );
  return [...SUCCESS_COOKIE_NAMES].every((name) => names.has(name));
}

export function isElementVisible(el: Element): boolean {
  const style = window.getComputedStyle(el);
  return (
    style.display !== "none" &&
    style.visibility !== "hidden" &&
    el instanceof HTMLElement &&
    el.offsetHeight > 0
  );
}

export async function queryVisible(page: VisibleQueryPage, selector: string): Promise<boolean> {
  return page.$eval(selector, isElementVisible).catch(() => false);
}

export async function detectFailure(page: FailureDetectionPage): Promise<string | null> {
  if (await queryVisible(page, ERROR_SELECTOR)) {
    return "로그인 실패: 아이디 또는 비밀번호를 확인해주세요";
  }

  if (await queryVisible(page, "#captchaDiv")) {
    return "로그인 실패: 캡챠가 감지되었습니다";
  }

  const url = page.url();
  const isLoginPage = url.includes("nid.naver.com");
  const hasLoginForm = await page
    .$("form#frmNIDLogin, #id")
    .then(Boolean)
    .catch(() => false);
  if (isLoginPage && !hasLoginForm) {
    return "로그인 실패: 접근이 차단되었습니다";
  }

  return null;
}

export async function waitForLogin(
  page: FailureDetectionPage,
  context: CookieContext,
  timeoutMs = 10 * 60 * 1000,
): Promise<Cookie[]> {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const cookies = await context.cookies();
    if (hasNaverSessionCookies(cookies)) {
      return cookies;
    }

    const reason = await detectFailure(page);
    if (reason) {
      await new Promise((resolve) => setTimeout(resolve, LOGIN_FAIL_TIMEOUT_MS));
      throw new Error(reason);
    }

    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
  throw new Error(
    "login timed out; complete captcha, 2FA, or extra verification in Chrome",
  );
}

export function buildLaunchArgs(): string[] {
  return ["--incognito", "--no-first-run", "--no-default-browser-check"];
}

export function buildLaunchOptions(input: Pick<LoginInput, "chromePath" | "headless">): {
  channel: "chrome";
  executablePath: string;
  headless: boolean;
  args: string[];
} {
  return {
    channel: "chrome",
    executablePath: input.chromePath,
    headless: input.headless,
    args: buildLaunchArgs(),
  };
}

export async function run(inputPath: string): Promise<void> {
  const text = await fsPromises.readFile(inputPath, "utf8");
  const input = validateInput(JSON.parse(text));

  const browser = (await chromium.launch(buildLaunchOptions(input))) as Browser;

  const context = (await browser.newContext({} as BrowserContextOptions)) as BrowserContext;
  const page = (await context.newPage()) as Page & FailureDetectionPage;
  try {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(LOGIN_URL, { waitUntil: "domcontentloaded" });
    await page.locator("#id").fill(input.id);
    await page.locator("#pw").fill(input.password);
    await page.locator("#log\\.login").click();

    const cookies = await waitForLogin(page, context);
    await fsPromises.mkdir(path.dirname(input.cookiesPath), {
      recursive: true,
    });
    await fsPromises.writeFile(
      input.cookiesPath,
      JSON.stringify(
        {
          accountId: input.accountId,
          savedAt: new Date().toISOString(),
          cookies: cookies.filter((c) => c.domain.includes("naver.com")),
        },
        null,
        2,
      ),
      "utf8",
    );
  } finally {
    await browser.close();
  }
}

function isCliEntrypoint(): boolean {
  const entrypoint = process.argv[1];
  return entrypoint ? path.basename(entrypoint).startsWith("naver-login") : false;
}

if (isCliEntrypoint()) {
  const inputPath = process.argv[2];
  if (!inputPath) {
    console.error("input json path is required");
    process.exit(1);
  }
  run(inputPath).catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : error);
    process.exit(1);
  });
}
