"use strict";

const fsPromises = require("fs").promises;
const path = require("path");
const { addExtra } = require("playwright-extra");
const playwright = require("playwright");
const StealthPlugin = require("puppeteer-extra-plugin-stealth");

// addExtra로 playwright를 직접 주입해야 pkg 스냅샷에서 동적 탐색 없이 동작함
const chromium = addExtra(playwright.chromium);
chromium.use(StealthPlugin());

const LOGIN_URL = "https://nid.naver.com/nidlogin.login";
const SUCCESS_COOKIE_NAMES = new Set(["NID_AUT", "NID_SES"]);

// 로그인 실패 감지용 선택자 (에러 메시지 영역)
const ERROR_SELECTOR = "#err_common";
const LOGIN_FAIL_TIMEOUT_MS = 3000;

function requiredString(input, key) {
  const value = input[key];
  if (typeof value !== "string" || value.trim() === "") {
    throw new Error(`${key} must be a non-empty string`);
  }
  return value;
}

function validateInput(input) {
  if (!input || typeof input !== "object") {
    throw new Error("input must be an object");
  }
  for (const key of [
    "accountId",
    "id",
    "password",
    "cookiesPath",
    "chromePath",
  ]) {
    requiredString(input, key);
  }
  return {
    accountId: requiredString(input, "accountId"),
    id: requiredString(input, "id"),
    password: requiredString(input, "password"),
    cookiesPath: requiredString(input, "cookiesPath"),
    headless: input.headless === true,
    chromePath: requiredString(input, "chromePath"),
  };
}

function hasNaverSessionCookies(cookies) {
  const names = new Set(
    cookies.filter((c) => c.domain.includes("naver.com")).map((c) => c.name),
  );
  return [...SUCCESS_COOKIE_NAMES].every((name) => names.has(name));
}

function isElementVisible(el) {
  const style = window.getComputedStyle(el);
  return (
    style.display !== "none" &&
    style.visibility !== "hidden" &&
    el.offsetHeight > 0
  );
}

async function queryVisible(page, selector) {
  return page.$eval(selector, isElementVisible).catch(() => false);
}

// 로그인 실패 원인을 반환. 실패가 아니면 null.
async function detectFailure(page) {
  // 1. 아이디/비밀번호 오류 메시지
  if (await queryVisible(page, ERROR_SELECTOR)) {
    return "로그인 실패: 아이디 또는 비밀번호를 확인해주세요";
  }

  // 2. 봇 탐지 → 로그인 폼에 캡챠 출현
  if (await queryVisible(page, "#captchaDiv")) {
    return "로그인 실패: 캡챠가 감지되었습니다";
  }

  // 3. 비정상 접근 차단 페이지 (로그인 폼 자체가 사라짐)
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

async function waitForLogin(page, context, timeoutMs = 10 * 60 * 1000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const cookies = await context.cookies();
    if (hasNaverSessionCookies(cookies)) {
      return cookies;
    }

    const reason = await detectFailure(page);
    if (reason) {
      await new Promise((resolve) =>
        setTimeout(resolve, LOGIN_FAIL_TIMEOUT_MS),
      );
      throw new Error(reason);
    }

    await new Promise((resolve) => setTimeout(resolve, 2000));
  }
  throw new Error(
    "login timed out; complete captcha, 2FA, or extra verification in Chrome",
  );
}

function buildLaunchArgs() {
  return ["--incognito", "--no-first-run", "--no-default-browser-check"];
}

function buildLaunchOptions(input) {
  return {
    channel: "chrome",
    executablePath: input.chromePath,
    headless: input.headless,
    args: buildLaunchArgs(),
  };
}

async function run(inputPath) {
  const text = await fsPromises.readFile(inputPath, "utf8");
  const input = validateInput(JSON.parse(text));

  const browser = await chromium.launch(buildLaunchOptions(input));

  const context = await browser.newContext();
  const page = await context.newPage();
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

if (require.main === module) {
  const inputPath = process.argv[2];
  if (!inputPath) {
    console.error("input json path is required");
    process.exit(1);
  }
  run(inputPath).catch((error) => {
    console.error(error.message ?? error);
    process.exit(1);
  });
} else {
  module.exports = {
    validateInput,
    hasNaverSessionCookies,
    isElementVisible,
    queryVisible,
    detectFailure,
    waitForLogin,
    buildLaunchArgs,
    buildLaunchOptions,
    run,
  };
}
