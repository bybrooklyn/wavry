import {spawn} from 'node:child_process';
import path from 'node:path';
import process from 'node:process';
import {setTimeout as delay} from 'node:timers/promises';
import {fileURLToPath} from 'node:url';

import {chromium} from 'playwright';

const HOST = '127.0.0.1';
const PORT = 4173;
const BASE_URL = `http://${HOST}:${PORT}`;
const TIMEOUT_MS = 30_000;

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(scriptDir, '..');

function normalizeBasePath(input) {
  const withLeadingSlash = input.startsWith('/') ? input : `/${input}`;
  return withLeadingSlash.endsWith('/') ? withLeadingSlash : `${withLeadingSlash}/`;
}

const BASE_PATH = normalizeBasePath(process.env.STYLE_CHECK_BASE_PATH ?? '/');
const ROOT_URL = new URL(BASE_PATH, BASE_URL).toString();
const PRICING_URL = new URL(`${BASE_PATH}pricing`, BASE_URL).toString();

function startServer() {
  const child = spawn(
    'bun',
    ['run', 'serve', '--', '--host', HOST, '--port', String(PORT)],
    {
      cwd: projectRoot,
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {...process.env},
    },
  );

  child.stdout.on('data', () => {});
  child.stderr.on('data', () => {});

  return child;
}

async function waitForServer() {
  const started = Date.now();
  while (Date.now() - started < TIMEOUT_MS) {
    try {
      const response = await fetch(ROOT_URL);
      if (response.ok) {
        return;
      }
    } catch {
      // Retry until timeout.
    }
    await delay(500);
  }

  throw new Error(`Timed out waiting for ${ROOT_URL}`);
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

async function runChecks() {
  const browser = await chromium.launch({headless: true});

  try {
    const desktop = await browser.newPage({viewport: {width: 1366, height: 900}});
    await desktop.goto(ROOT_URL, {waitUntil: 'networkidle'});

    const desktopMetrics = await desktop.evaluate(() => {
      const doc = document.documentElement;
      const heading = document.querySelector('h1')?.textContent ?? '';
      const bodyText = document.body.textContent ?? '';
      const bodyBg = window.getComputedStyle(document.body).backgroundColor;
      const footerContainer = document.querySelector('.footer .container');
      const footerRect = footerContainer?.getBoundingClientRect();

      const footerCenterDelta = footerRect
        ? Math.abs((footerRect.left + footerRect.width / 2) - window.innerWidth / 2)
        : Number.POSITIVE_INFINITY;

      return {
        heading,
        bodyText,
        bodyBg,
        hasHorizontalOverflow: doc.scrollWidth > doc.clientWidth + 1,
        footerCenterDelta,
      };
    });

    assert(
      desktopMetrics.heading.includes('Wavry') || desktopMetrics.bodyText.includes('Wavry Overview'),
      `Root docs page did not render expected content. Found heading: ${desktopMetrics.heading}`,
    );
    assert(!desktopMetrics.hasHorizontalOverflow, 'Desktop docs page has horizontal overflow');
    assert(desktopMetrics.bodyBg !== 'rgb(255, 255, 255)', 'Background rendered as white');
    assert(
      desktopMetrics.footerCenterDelta <= 8,
      `Footer container is not centered. Delta: ${desktopMetrics.footerCenterDelta.toFixed(2)}px`,
    );

    const deploymentLink = desktop.locator('a[href="/deployment-modes"]').first();
    if ((await deploymentLink.count()) > 0) {
      await deploymentLink.hover();
      const hoverColor = await deploymentLink.evaluate(
        (node) => window.getComputedStyle(node).color,
      );
      assert(hoverColor !== 'rgb(0, 0, 0)', 'Deployment Modes link turns black on hover');
    }

    const licenseLink = desktop.locator('a[href="https://github.com/bybrooklyn/wavry/blob/main/LICENSE"]').first();
    if ((await licenseLink.count()) > 0) {
      await licenseLink.hover();
      const hoverColor = await licenseLink.evaluate(
        (node) => window.getComputedStyle(node).color,
      );
      assert(hoverColor !== 'rgb(0, 0, 0)', 'AGPL/RIFT license link turns black on hover');
    }

    const mobile = await browser.newPage({viewport: {width: 390, height: 844}});
    await mobile.goto(ROOT_URL, {waitUntil: 'networkidle'});

    const mobileMetrics = await mobile.evaluate(() => {
      const doc = document.documentElement;
      const selectors = [
        '.navbar__toggle',
        'button[aria-label="Open navigation bar"]',
        'button[aria-label="Open sidebar"]',
        '.theme-doc-sidebar-menu',
      ];

      const hasVisibleToggle = selectors.some((selector) => {
        const node = document.querySelector(selector);
        if (!node) {
          return false;
        }
        const style = window.getComputedStyle(node);
        const rect = node.getBoundingClientRect();
        return (
          style.display !== 'none' &&
          style.visibility !== 'hidden' &&
          parseFloat(style.opacity || '1') > 0 &&
          rect.width > 0 &&
          rect.height > 0
        );
      });

      return {
        hasHorizontalOverflow: doc.scrollWidth > doc.clientWidth + 1,
        hasVisibleToggle,
      };
    });

    assert(!mobileMetrics.hasHorizontalOverflow, 'Mobile docs page has horizontal overflow');
    assert(!mobileMetrics.hasVisibleToggle, 'Mobile sidebar toggle is visible');

    const pricingPage = await browser.newPage({viewport: {width: 1366, height: 900}});
    await pricingPage.goto(PRICING_URL, {waitUntil: 'networkidle'});
    const pricingHeading = await pricingPage.textContent('h1');
    assert(
      typeof pricingHeading === 'string' && pricingHeading.includes('Pricing'),
      'Pricing docs page did not render expected heading',
    );
  } finally {
    await browser.close();
  }
}

let server;

try {
  server = startServer();
  await waitForServer();
  await runChecks();
  console.log('Style regression checks passed.');
} catch (error) {
  console.error('Style regression checks failed.');
  console.error(error instanceof Error ? error.message : error);
  process.exitCode = 1;
} finally {
  if (server && !server.killed) {
    server.kill('SIGTERM');
    await delay(250);
    if (!server.killed) {
      server.kill('SIGKILL');
    }
  }
}
