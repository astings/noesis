// Captures the README screenshots and demo GIF from the Next.js UI running against a fake
// Tauri backend (mock-tauri.js). Usage: npm run capture [-- --only=home,meeting,ask,search,gif]
// PORT (default 3118): reuse a dev server already on it, or start one there.
import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readdirSync, rmSync, renameSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../..');
const frontend = path.join(repo, 'frontend');
const outDir = path.join(repo, 'docs/readme');
const tmpDir = path.join(here, 'out');
const PORT = process.env.PORT ?? '3118';
const BASE = `http://localhost:${PORT}`;
const VIEWPORT = { width: 1280, height: 800 };
const only = (process.argv.find((a) => a.startsWith('--only=')) ?? '').slice(7).split(',').filter(Boolean);
const want = (name) => !only.length || only.includes(name);

async function serverUp() {
  try { return (await fetch(BASE)).ok; } catch { return false; }
}

// Reuse a running dev server, else start one and stop it at the end. Two next dev processes
// share frontend/.next and break each other's routes, so never start a second one.
async function ensureServer() {
  if (await serverUp()) return null;
  let running = '';
  try { running = execFileSync('pgrep', ['-fl', `${frontend}/node_modules/.*next dev`], { encoding: 'utf8' }); } catch { /* none */ }
  if (running) {
    const port = running.match(/-p (\d+)/)?.[1] ?? '?';
    throw new Error(`a next dev server for this frontend already runs on port ${port}; rerun with PORT=${port}`);
  }
  console.log(`starting next dev on :${PORT}`);
  const proc = spawn('pnpm', ['exec', 'next', 'dev', '-p', PORT], { cwd: frontend, stdio: 'ignore', detached: true });
  for (let i = 0; i < 120; i++) {
    if (await serverUp()) return proc;
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Error(`next dev did not answer on :${PORT} within 120 s`);
}

async function newPage(browser, { video } = {}) {
  const context = await browser.newContext({
    viewport: VIEWPORT,
    deviceScaleFactor: video ? 1 : 2,
    colorScheme: 'light',
    timezoneId: 'America/New_York',
    locale: 'en-US',
    ...(video ? { recordVideo: { dir: tmpDir, size: VIEWPORT } } : {}),
  });
  await context.addInitScript({ path: path.join(here, 'mock-tauri.js') });
  const page = await context.newPage();
  await page.clock.setSystemTime(new Date('2026-10-30T15:00:00-04:00'));
  page.on('pageerror', (e) => console.warn('page error:', e.message));
  return { context, page };
}

const settle = (page, ms = 800) => page.waitForTimeout(ms);

// The sidebar starts collapsed on every launch; open it so the projects show.
async function expandSidebar(page) {
  await page.getByLabel('Expand sidebar').click();
  await page.mouse.move(VIEWPORT.width / 2, VIEWPORT.height - 5);
  await settle(page, 500);
}

// firstLine: start of the meeting's first transcript line in mock-tauri.js, to know it rendered
async function openMeeting(page, id = 'meeting-demo-1', firstLine = 'Thanks for making the time') {
  await page.goto(`${BASE}/meeting-details?id=${id}`);
  await page.getByText(firstLine).first().waitFor();
  await expandSidebar(page);
}

async function shot(page, name) {
  await page.screenshot({ path: path.join(outDir, `${name}.png`) });
  console.log(`wrote docs/readme/${name}.png`);
}

async function ask(page, question, { slow = false } = {}) {
  const input = page.getByPlaceholder(/Ask about this meeting/).first();
  await input.click();
  await input.pressSequentially(question, { delay: slow ? 45 : 0 });
  await input.press('Enter');
  await page.getByText('250-seat tier').first().waitFor({ timeout: 10000 });
  await settle(page);
}

const scenes = {
  async home(browser) {
    const { context, page } = await newPage(browser);
    await page.goto(BASE);
    await page.getByText('Pricing review with Acme').first().waitFor();
    await expandSidebar(page);
    await shot(page, 'home');
    console.log('unhandled commands:', await page.evaluate(() => [...window.__demo.unknown].join(', ')));
    await context.close();
  },
  async meeting(browser) {
    const { context, page } = await newPage(browser);
    await openMeeting(page);
    await shot(page, 'meeting');
    await context.close();
  },
  async ask(browser) {
    const { context, page } = await newPage(browser);
    await openMeeting(page);
    await ask(page, 'What did we agree on pricing?');
    await shot(page, 'ask');
    await context.close();
  },
  async search(browser) {
    const { context, page } = await newPage(browser);
    await openMeeting(page, 'meeting-demo-3', 'Quick one today');
    await page.getByPlaceholder('Search transcripts').pressSequentially('procurement');
    await page.getByText('Pricing review with Acme').nth(1).waitFor();
    await settle(page);
    await shot(page, 'search');
    await context.close();
  },
  async gif(browser) {
    const { context, page } = await newPage(browser, { video: true });
    await page.goto(BASE);
    await page.getByText('Pricing review with Acme').first().waitFor();
    await expandSidebar(page);
    await settle(page, 700);
    await page.getByPlaceholder('Search transcripts').pressSequentially('procurement', { delay: 70 });
    await page.getByText('Perfect. I will book thirty minutes').first().waitFor();
    await settle(page, 1500);
    await page.getByText('Perfect. I will book thirty minutes').first().click();
    await page.getByText('Thanks for making the time').first().waitFor();
    await settle(page, 1800);
    await ask(page, 'What did we agree on pricing?', { slow: true });
    await settle(page, 2500);
    const video = page.video();
    await context.close();
    const webm = path.join(tmpDir, 'demo.webm');
    renameSync(await video.path(), webm);
    toGif(webm, path.join(outDir, 'demo.gif'));
  },
};

// Two-pass palette keeps the GIF sharp and small; 12 fps at 800 px wide is enough for UI.
function toGif(input, output) {
  const filters = 'fps=12,scale=800:-1:flags=lanczos';
  const palette = path.join(tmpDir, 'palette.png');
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', input, '-vf', `${filters},palettegen=stats_mode=diff`, palette]);
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', input, '-i', palette, '-lavfi', `${filters}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`, output]);
  console.log('wrote docs/readme/demo.gif');
}

mkdirSync(outDir, { recursive: true });
rmSync(tmpDir, { recursive: true, force: true });
mkdirSync(tmpDir, { recursive: true });
const server = await ensureServer();
const browser = await chromium.launch();
try {
  for (const [name, run] of Object.entries(scenes)) if (want(name)) await run(browser);
} finally {
  await browser.close();
  if (server) process.kill(-server.pid);
}
console.log('done:', readdirSync(outDir).join(', '));
