import childProcess from "node:child_process";
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
  chromePath: string;
  cdpPort: number;
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

// 입력 객체에서 필수 문자열 값을 추출하고 유효성을 검사
function requiredString(input: RawInput, key: string): string {
  const value = input[key];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${key} must be a non-empty string`);
  }
  return value;
}

// 로그인에 필요한 입력값들의 형식과 내용을 검증하고 LoginInput 객체로 변환
export function validateInput(input: unknown): LoginInput {
  if (!input || typeof input !== "object") {
    throw new Error("input must be an object");
  }
  const candidate = input as RawInput;

  for (const key of [
    "accountId",
    "id",
    "password",
    "cookiesPath",
    "chromePath",
  ]) {
    requiredString(candidate, key);
  }

  const cdpPort = candidate.cdpPort;
  if (
    typeof cdpPort !== "number" ||
    !Number.isInteger(cdpPort) ||
    cdpPort < 1
  ) {
    throw new Error("cdpPort must be a positive integer");
  }

  return {
    accountId: requiredString(candidate, "accountId"),
    id: requiredString(candidate, "id"),
    password: requiredString(candidate, "password"),
    cookiesPath: requiredString(candidate, "cookiesPath"),
    headless: candidate.headless === true,
    chromePath: requiredString(candidate, "chromePath"),
    cdpPort,
  };
}

// 쿠키 배열에 네이버 로그인 성공에 필요한 NID_AUT, NID_SES 쿠키가 모두 있는지 확인
export function hasNaverSessionCookies(cookies: NaverCookie[]): boolean {
  const names = new Set(
    cookies
      .filter((cookie) => cookie.domain.includes("naver.com"))
      .map((cookie) => cookie.name),
  );
  return [...SUCCESS_COOKIE_NAMES].every((name) => names.has(name));
}

// 네이버 관련 쿠키들을 필터링하고 저장 시간과 함께 CookieResult 객체로 구성
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

// WSL2 기본 게이트웨이(= Windows 호스트 IP)를 ip route에서 읽어 반환
function getWindowsHostIp(): string {
  try {
    const result = childProcess.execSync("ip route show default", {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    const match = result.match(/default via ([\d.]+)/);
    if (match?.[1]) return match[1];
  } catch {
    // ignore error, fallback to localhost
  }
  return "localhost";
}

// CDP 엔드포인트가 응답할 때까지 여러 호스트를 순서대로 시도하고 연결된 호스트를 반환
async function waitForCdpReady(
  port: number,
  timeoutMs = 15_000,
): Promise<string> {
  const candidates = ["localhost", "127.0.0.1", getWindowsHostIp()];
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    for (const host of candidates) {
      try {
        const res = await fetch(`http://${host}:${port}/json/version`, {
          signal: AbortSignal.timeout(500),
        });
        if (res.ok) return host;
      } catch {
        // ignore error, try next host
      }
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  throw new Error(
    `Chrome CDP not ready on port ${port} after ${timeoutMs}ms (tried: ${candidates.join(", ")})`,
  );
}

type BrowserSession = {
  context: BrowserContext;
  close: () => Promise<void>;
};

// Windows Chrome을 CDP 포트로 실행한 뒤 Playwright로 연결
async function createBrowserSession(
  chromePath: string,
  cdpPort: number,
  headless: boolean,
): Promise<BrowserSession> {
  const proc = childProcess.spawn(
    chromePath,
    [
      `--remote-debugging-port=${cdpPort}`,
      "--remote-debugging-address=0.0.0.0",
      "--remote-allow-origins=*",
      "--no-first-run",
      "--no-default-browser-check",
      "--incognito",
      ...(headless ? ["--headless=new"] : []),
    ],
    { detached: true, stdio: "ignore" },
  );
  const host = await waitForCdpReady(cdpPort);
  const browser = await chromium.connectOverCDP(`http://${host}:${cdpPort}`);
  const context = browser.contexts()[0] ?? (await browser.newContext());
  return {
    context,
    close: async () => {
      try {
        await browser.close();
      } catch {
        /* ignore */
      }
      proc.kill();
    },
  };
}

// 파일에서 로그인 입력값을 읽고 검증
async function readInput(inputPath: string): Promise<LoginInput> {
  const text = await fs.readFile(inputPath, "utf8");
  return validateInput(JSON.parse(text));
}

// 로그인 완료를 기다리며 주기적으로 세션 쿠키를 확인 (타임아웃 시 에러 발생)
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

// 네이버 로그인 자동화를 수행하고 세션 쿠키를 파일에 저장
async function run(inputPath: string): Promise<void> {
  const input = await readInput(inputPath);
  const session = await createBrowserSession(
    input.chromePath,
    input.cdpPort,
    input.headless,
  );

  try {
    const page = await session.context.newPage();
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(LOGIN_URL, { waitUntil: "domcontentloaded" });
    await page.locator("#id").fill(input.id);
    await page.locator("#pw").fill(input.password);
    await page.locator("#log\\.login").click();

    const cookies = await waitForLogin(session.context);
    await fs.mkdir(path.dirname(input.cookiesPath), { recursive: true });
    await fs.writeFile(
      input.cookiesPath,
      JSON.stringify(buildCookieResult(input.accountId, cookies), null, 2),
      "utf8",
    );
  } finally {
    await session.close();
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
