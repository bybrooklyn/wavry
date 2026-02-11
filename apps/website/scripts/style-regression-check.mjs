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
const OVERVIEW_DOC_URL = new URL(`${BASE_PATH}docs/overview`, BASE_URL).toString();

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

async function getLayoutMetrics(page) {
  return page.evaluate(() => {
    const hero = document.querySelector('[class*="hero"]');
    const cardGrid = document.querySelector('[class*="cardGrid"]');
    const trackGrid = document.querySelector('[class*="trackGrid"]');

    const heroStyle = hero ? window.getComputedStyle(hero) : null;

    const cardColumns = cardGrid
      ? new Set(
          Array.from(cardGrid.children, (node) =>
            Math.round(node.getBoundingClientRect().left),
          ),
        ).size
      : 0;

    const trackColumns = trackGrid
      ? new Set(
          Array.from(trackGrid.children, (node) =>
            Math.round(node.getBoundingClientRect().left),
          ),
        ).size
      : 0;

    const doc = document.documentElement;

    return {
      heroPaddingTop: heroStyle ? parseFloat(heroStyle.paddingTop) : 0,
      heroPaddingBottom: heroStyle ? parseFloat(heroStyle.paddingBottom) : 0,
      cardColumns,
      trackColumns,
      hasHorizontalOverflow: doc.scrollWidth > doc.clientWidth + 1,
    };
  });
}

async function runChecks() {
  const browser = await chromium.launch({headless: true});

  try {
    const desktop = await browser.newPage({viewport: {width: 1366, height: 900}});
    await desktop.goto(ROOT_URL, {waitUntil: 'networkidle'});

    const desktopMetrics = await getLayoutMetrics(desktop);
    assert(
      desktopMetrics.heroPaddingTop >= 100,
      `Desktop hero top padding too small: ${desktopMetrics.heroPaddingTop}px`,
    );
    assert(
      desktopMetrics.heroPaddingBottom >= 90,
      `Desktop hero bottom padding too small: ${desktopMetrics.heroPaddingBottom}px`,
    );
    assert(
      desktopMetrics.cardColumns >= 3,
      `Desktop card grid expected >=3 columns, got ${desktopMetrics.cardColumns}`,
    );
    assert(
      desktopMetrics.trackColumns >= 2,
      `Desktop track grid expected >=2 columns, got ${desktopMetrics.trackColumns}`,
    );
    assert(!desktopMetrics.hasHorizontalOverflow, 'Desktop page has horizontal overflow');

    const mobile = await browser.newPage({viewport: {width: 390, height: 844}});
    await mobile.goto(ROOT_URL, {waitUntil: 'networkidle'});
    const mobileMetrics = await getLayoutMetrics(mobile);
    assert(
      mobileMetrics.cardColumns === 1,
      `Mobile card grid expected 1 column, got ${mobileMetrics.cardColumns}`,
    );
    assert(
      mobileMetrics.trackColumns === 1,
      `Mobile track grid expected 1 column, got ${mobileMetrics.trackColumns}`,
    );
    assert(!mobileMetrics.hasHorizontalOverflow, 'Mobile page has horizontal overflow');

    const docsPage = await browser.newPage({viewport: {width: 1366, height: 900}});
    await docsPage.goto(OVERVIEW_DOC_URL, {waitUntil: 'networkidle'});
    const headingText = await docsPage.textContent('h1');
    assert(
      typeof headingText === 'string' && headingText.includes('Wavry Overview'),
      'Docs overview page did not render expected heading',
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
