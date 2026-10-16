import assert from 'node:assert/strict';
import { mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from 'playwright';

const toolRoot = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(toolRoot, '../..');
const workRoot = path.join(toolRoot, '.work');
const outputRoot = path.join(projectRoot, 'docs/assets');
const width = 1024;
const height = 704;
const options = { preview: false, executablePath: undefined };
for (let i = 2; i < process.argv.length; i++) {
  const option = process.argv[i];
  if (option === '--preview') options.preview = true;
  else if (option === '--browser-executable' && process.argv[i + 1]) options.executablePath = path.resolve(process.argv[++i]);
  else throw new Error(`Unknown or incomplete option: ${option}`);
}

async function verifyInteractions(page) {
  await page.evaluate(() => window.resetDemo());
  assert.equal(await page.locator('#record-button').isDisabled(), true);
  assert.equal(await page.locator('#minimize-recording').isChecked(), true);
  await page.locator('#select-button').click();
  assert.equal(await page.locator('#controller').isVisible(), false);
  const desktop = await page.locator('#desktop').boundingBox();
  await page.mouse.move(desktop.x + 37, desktop.y + 97);
  await page.mouse.down();
  await page.mouse.move(desktop.x + 677, desktop.y + 457, { steps: 12 });
  await page.mouse.up();
  const selected = await page.evaluate(() => window.demoState.region);
  assert.deepEqual(selected, { x: 36, y: 96, width: 640, height: 360 });
  await page.locator('#fps').selectOption('60');
  await page.locator('#include-cursor').uncheck();
  assert.equal(await page.evaluate(() => window.demoState.fps), 60);
  assert.equal(await page.evaluate(() => window.demoState.cursor), false);
  await page.locator('#record-button').click();
  assert.equal(await page.locator('#controller').isVisible(), false);
  assert.equal(await page.locator('#minimize-recording').isDisabled(), true);
  await page.keyboard.press('Control+Shift+F10');
  await page.waitForFunction(() => window.demoState.phase === 'save-dialog');
  await page.locator('#cancel-save').click();
  assert.equal(await page.evaluate(() => window.demoState.phase), 'pending');
  assert.equal(await page.locator('#select-button').isDisabled(), true);
  assert.equal(await page.locator('#record-button').textContent(), '録画を保存');
  await page.locator('#record-button').click();
  await page.locator('#filename').fill('capture.mp4');
  await page.locator('#save-button').click();
  await page.waitForFunction(() => window.demoState.phase === 'saved');
  assert.equal(await page.locator('#success-dialog').isVisible(), true);
  assert.equal(await page.locator('#saved-path').textContent(), 'C:\\Users\\Demo\\Videos\\capture.mp4');
  await page.locator('#success-ok').click();
  assert.equal(await page.locator('#status').textContent(), '保存完了');
  assert.equal(await page.locator('#minimize-recording').isEnabled(), true);
  await page.locator('#minimize-recording').uncheck();
  await page.locator('#record-button').click();
  assert.equal(await page.locator('#controller').isVisible(), true);
  assert.equal(await page.locator('#record-button').textContent(), '録画停止');
  assert.equal(await page.locator('#record-button').isEnabled(), true);
  for (const id of ['select-button', 'fps', 'include-cursor', 'minimize-recording']) {
    assert.equal(await page.locator(`#${id}`).isDisabled(), true);
  }
  await page.waitForFunction(() => document.getElementById('status').textContent.startsWith('録画中 '));
  await page.screenshot({ path: path.join(workRoot, 'recording-option-off.png') });
  await page.locator('#record-button').click();
  assert.equal(await page.locator('#controller').isVisible(), true);
  assert.equal(await page.locator('#record-button').isDisabled(), true);
  await page.waitForFunction(() => window.demoState.phase === 'save-dialog');
  await page.locator('#cancel-save').click();
  assert.equal(await page.locator('#minimize-recording').isDisabled(), true);
  await page.locator('#record-button').click();
  await page.locator('#save-button').click();
  await page.waitForFunction(() => window.demoState.phase === 'saved');
  await page.locator('#success-ok').click();
  assert.equal(await page.locator('#minimize-recording').isEnabled(), true);
  assert.equal(await page.locator('#minimize-recording').isChecked(), false);
  await page.evaluate(() => window.resetDemo());
  console.log('Mock interactions verified: select, FPS/cursor, minimize on/off, hotkey/button stop, cancel/retry save, and saved path.');
}

async function main() {
  await mkdir(workRoot, { recursive: true });
  const manifest = await readFile(path.join(projectRoot, 'Cargo.toml'), 'utf8');
  const version = manifest.match(/^version = "((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))"$/m)?.[1];
  if (!version) throw new Error('Cargo.toml must have a stable MAJOR.MINOR.PATCH version.');

  const browser = await chromium.launch({ headless: true, executablePath: options.executablePath, args: ['--force-color-profile=srgb'] });
  try {
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: 'reduce' });
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    const url = pathToFileURL(path.join(toolRoot, 'mock.html'));
    url.searchParams.set('version', version);
    await page.goto(url.href);
    await page.evaluate(() => document.fonts.ready);
    await verifyInteractions(page);

    const destination = options.preview ? path.join(workRoot, 'previews') : outputRoot;
    await mkdir(destination, { recursive: true });
    const hero = await page.evaluate(() => window.renderDemoFrame(4.5));
    assert.equal(hero.phase, 'ready');
    await page.locator('#desktop').screenshot({ path: path.join(destination, 'hero.png') });
    for (const [name, time, phase] of [
      ['step-select', 3.2, 'selecting'],
      ['step-ready', 4.5, 'ready'],
      ['step-save', 12.5, 'save-dialog'],
    ]) {
      const frame = await page.evaluate((t) => window.renderDemoFrame(t), time);
      assert.equal(frame.phase, phase);
      await page.screenshot({ path: path.join(destination, `${name}.png`) });
    }
    const later = await page.evaluate(() => window.renderDemoFrame(16.5));
    await page.evaluate(() => window.renderDemoFrame(0));
    const repeated = await page.evaluate(() => window.renderDemoFrame(16.5));
    assert.deepEqual(repeated, later);
    assert.deepEqual(pageErrors, []);
    console.log(`Operation mock stills: ${path.relative(projectRoot, destination)}`);
  } finally {
    await browser.close();
  }
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
