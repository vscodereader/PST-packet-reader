'use strict';

const fsPromises = require('fs').promises;
const path = require('path');
const { chromium } = require('playwright-extra');
const StealthPlugin = require('puppeteer-extra-plugin-stealth');

chromium.use(StealthPlugin());

const LOGIN_URL = 'https://nid.naver.com/nidlogin.login';
const SUCCESS_COOKIE_NAMES = new Set(['NID_AUT', 'NID_SES']);

function requiredString(input, key) {
  const value = input[key];
  if (typeof value !== 'string' || value.trim() === '') {
    throw new Error(`${key} must be a non-empty string`);
  }
  return value;
}

function validateInput(input) {
  if (!input || typeof input !== 'object') {
    throw new Error('input must be an object');
  }
  for (const key of ['accountId', 'id', 'password', 'cookiesPath', 'chromePath']) {
    requiredString(input, key);
  }
  return {
    accountId: requiredString(input, 'accountId'),
    id: requiredString(input, 'id'),
    password: requiredString(input, 'password'),
    cookiesPath: requiredString(input, 'cookiesPath'),
    headless: input.headless === true,
    chromePath: requiredString(input, 'chromePath'),
  };
}

function hasNaverSessionCookies(cookies) {
  const names = new Set(
    cookies.filter(c => c.domain.includes('naver.com')).map(c => c.name)
  );
  return [...SUCCESS_COOKIE_NAMES].every(name => names.has(name));
}

async function waitForLogin(context, timeoutMs = 10 * 60 * 1000) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    const cookies = await context.cookies();
    if (hasNaverSessionCookies(cookies)) {
      return cookies;
    }
    await new Promise(resolve => setTimeout(resolve, 2000));
  }
  throw new Error('login timed out; complete captcha, 2FA, or extra verification in Chrome');
}

async function run(inputPath) {
  const text = await fsPromises.readFile(inputPath, 'utf8');
  const input = validateInput(JSON.parse(text));

  const browser = await chromium.launch({
    executablePath: input.chromePath,
    headless: input.headless,
    args: [
      '--no-first-run',
      '--no-default-browser-check',
      '--incognito',
    ],
  });

  const context = await browser.newContext();
  try {
    const page = await context.newPage();
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(LOGIN_URL, { waitUntil: 'domcontentloaded' });
    await page.locator('#id').fill(input.id);
    await page.locator('#pw').fill(input.password);
    await page.locator('#log\\.login').click();

    const cookies = await waitForLogin(context);
    await fsPromises.mkdir(path.dirname(input.cookiesPath), { recursive: true });
    await fsPromises.writeFile(
      input.cookiesPath,
      JSON.stringify(
        {
          accountId: input.accountId,
          savedAt: new Date().toISOString(),
          cookies: cookies.filter(c => c.domain.includes('naver.com')),
        },
        null,
        2
      ),
      'utf8'
    );
  } finally {
    await browser.close();
  }
}

const inputPath = process.argv[2];
if (!inputPath) {
  console.error('input json path is required');
  process.exit(1);
}
run(inputPath).catch(error => {
  console.error(error);
  process.exit(1);
});
