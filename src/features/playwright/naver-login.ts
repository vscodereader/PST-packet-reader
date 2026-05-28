import fs from "node:fs/promises";
import path from "node:path";
import { argv, exit } from "node:process";
import { fileURLToPath } from "node:url";

import type { BrowserContext } from "playwright";
import { chromium } from "playwright-extra";
import StealthPlugin from "puppeteer-extra-plugin-stealth";

chromium.use(StealthPlugin());

const LOGIN_URL = "https://nid.naver.com/nidlogin.login";
const SUCCESS_COOKIE_NAMES = new Set(["NID_AUT", "NID_SES"]);

type RawInput = Record<string, unknown>;

type LoginInput = {
  accountId: string;
  id: string;
  password: string;
  cookiesPath: string;
  headless: boolean;
};

type NaverCookie = {
  name: string;
  domain: string;
  [key: string]: unknown;
};

type CookieResult = {
  accountId: string;
  savedAt: string;
  cookies: NaverCookie[];
};

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
  const candidate = input as RawInput;

  for (const key of ["accountId", "id", "password", "cookiesPath"]) {
    requiredString(candidate, key);
  }

  return {
    accountId: requiredString(candidate, "accountId"),
    id: requiredString(candidate, "id"),
    password: requiredString(candidate, "password"),
    cookiesPath: requiredString(candidate, "cookiesPath"),
    headless: candidate.headless === true,
  };
}

export function hasNaverSessionCookies(cookies: NaverCookie[]): boolean {
  const names = new Set(
    cookies
      .filter((cookie) => cookie.domain.includes("naver.com"))
      .map((cookie) => cookie.name),
  );
  return [...SUCCESS_COOKIE_NAMES].every((name) => names.has(name));
}

export function buildCookieResult(
  accountId: string,
  cookies: NaverCookie[],
): CookieResult {
  return {
    accountId,
    savedAt: new Date().toISOString(),
    cookies: cookies.filter((cookie) => cookie.domain.includes("naver.com")),
  };
}

async function readInput(inputPath: string): Promise<LoginInput> {
  const text = await fs.readFile(inputPath, "utf8");
  return validateInput(JSON.parse(text));
}

async function waitForLogin(
  context: BrowserContext,
  timeoutMs = 10 * 60 * 1000,
): Promise<NaverCookie[]> {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const cookies = (await context.cookies()) as unknown as NaverCookie[];
    if (hasNaverSessionCookies(cookies)) {
      return cookies;
    }
    await new Promise((resolve) => setTimeout(resolve, 2000));
  }

  throw new Error(
    "login timed out; complete captcha, 2FA, or extra verification in Chrome",
  );
}

async function run(inputPath: string): Promise<void> {
  const input = await readInput(inputPath);
  const browser = await chromium.launch({
    channel: "chrome",
    headless: input.headless,
    args: ["--incognito"],
  });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 900 },
  });

  try {
    const page = await context.newPage();
    await page.goto(LOGIN_URL, { waitUntil: "domcontentloaded" });
    await page.locator("#id").fill(input.id);
    await page.locator("#pw").fill(input.password);
    await page.locator("#log\\.login").click();

    const cookies = await waitForLogin(context);
    await fs.mkdir(path.dirname(input.cookiesPath), { recursive: true });
    await fs.writeFile(
      input.cookiesPath,
      JSON.stringify(buildCookieResult(input.accountId, cookies), null, 2),
      "utf8",
    );
  } finally {
    await context.close();
    await browser.close();
  }
}

const isMain =
  typeof argv[1] === "string" &&
  fileURLToPath(import.meta.url) === path.resolve(argv[1]);

if (isMain) {
  const inputPath = argv[2];
  if (!inputPath) {
    console.error("input json path is required");
    exit(1);
  }
  run(inputPath).catch((error: unknown) => {
    console.error(error);
    exit(1);
  });
}
