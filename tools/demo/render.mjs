import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from 'playwright';
import ffmpeg from 'ffmpeg-static';
import ffprobe from 'ffprobe-static';

const toolRoot = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(toolRoot, '../..');
const workRoot = path.join(toolRoot, '.work');
const outputRoot = path.join(projectRoot, 'docs/assets');
const width = 1024;
const height = 704;
const fps = 24;
const duration = 20;
const frameCount = fps * duration;
const gifWidth = 960;
const gifFps = 12;
const options = { preview: false, executablePath: undefined };
for (let i = 2; i < process.argv.length; i++) {
  const option = process.argv[i];
  if (option === '--preview') options.preview = true;
  else if (option === '--browser-executable' && process.argv[i + 1]) options.executablePath = path.resolve(process.argv[++i]);
  else throw new Error(`Unknown or incomplete option: ${option}`);
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: projectRoot, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (part) => { stdout += part; });
    child.stderr.on('data', (part) => { stderr += part; });
    child.once('error', reject);
    child.once('close', (code) => code === 0 ? resolve(stdout) : reject(new Error(`${path.basename(command)} exited ${code}: ${stderr}`)));
  });
}

async function verifyInteractions(page) {
  await page.evaluate(() => window.resetDemo());
  assert.equal(await page.locator('#record-button').isDisabled(), true);
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
  await page.evaluate(() => window.resetDemo());
  console.log('Mock interactions verified: select, FPS/cursor, record, hotkey stop, cancel/retry save, and saved path.');
}

async function inspectMetadata(file, expected) {
  const data = JSON.parse(await run(ffprobe.path, ['-v', 'error', '-count_frames', '-show_streams', '-show_format', '-of', 'json', file]));
  const video = data.streams.find((stream) => stream.codec_type === 'video');
  assert.equal(video.width, expected.width);
  assert.equal(video.height, expected.height);
  assert.equal(Number(video.nb_read_frames), expected.frames);
  // GIF stores centisecond delays per frame, rather than a container duration.
  let actualDuration;
  if (file.endsWith('.gif')) {
    const timing = JSON.parse(await run(ffprobe.path, ['-v', 'error', '-show_frames', '-show_entries', 'frame=pkt_duration_time', '-of', 'json', file]));
    assert.equal(timing.frames.length, expected.frames);
    actualDuration = timing.frames.reduce((total, frame) => {
      const delay = Number(frame.pkt_duration_time);
      assert.ok(Number.isFinite(delay) && delay > 0, 'Each GIF frame must have a positive delay.');
      return total + delay;
    }, 0);
  } else {
    actualDuration = Number(data.format.duration);
  }
  assert.ok(Math.abs(actualDuration - duration) < 0.1, `Unexpected duration: ${actualDuration}`);
  if (file.endsWith('.mp4')) {
    assert.equal(video.codec_name, 'h264');
    assert.equal(video.pix_fmt, 'yuv420p');
    assert.equal(data.streams.length, 1, 'The demo must have no audio track.');
  }
  await run(ffmpeg, ['-v', 'error', ...(file.endsWith('.gif') ? ['-ignore_loop', '1'] : []), '-i', file, '-f', 'null', '-']);
  return { file: path.relative(projectRoot, file), width: video.width, height: video.height, duration: Number(actualDuration.toFixed(2)), frames: Number(video.nb_read_frames), bytes: (await stat(file)).size };
}

async function main() {
  await mkdir(workRoot, { recursive: true });
  // Keep Chromium profiles and scratch frames on the same explicit work volume.
  process.env.TMPDIR = workRoot;
  process.env.TMP = workRoot;
  process.env.TEMP = workRoot;
  const manifest = await readFile(path.join(projectRoot, 'Cargo.toml'), 'utf8');
  const version = manifest.match(/^version = "((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))"$/m)?.[1];
  if (!version) throw new Error('Cargo.toml must have a stable MAJOR.MINOR.PATCH version.');
  if (!options.preview) {
    if (!ffmpeg || !ffprobe.path) throw new Error('FFmpeg and FFprobe are required; run npm ci in tools/demo.');
    await stat(ffmpeg);
    await stat(ffprobe.path);
  }
  const browser = await chromium.launch({ headless: true, executablePath: options.executablePath, args: ['--force-color-profile=srgb'] });
  const frameRoot = await mkdtemp(path.join(workRoot, 'frames-'));
  try {
    const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: 'reduce' });
    const pageErrors = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    const url = pathToFileURL(path.join(toolRoot, 'mock.html'));
    url.searchParams.set('version', version);
    await page.goto(url.href);
    await page.evaluate(() => document.fonts.ready);
    await verifyInteractions(page);
    const previews = path.join(workRoot, 'previews');
    await mkdir(previews, { recursive: true });
    for (const [name, time, phase] of [
      ['start', 0, 'idle'], ['selection', 3.2, 'selecting'], ['ready', 4.5, 'ready'],
      ['recording', 8.5, 'recording'], ['save', 12.5, 'save-dialog'],
      ['result', 16.5, 'saved'], ['loop', 19.8, 'idle'],
    ]) {
      const frame = await page.evaluate((t) => window.renderDemoFrame(t), time);
      assert.equal(frame.phase, phase);
      await page.screenshot({ path: path.join(previews, `${name}.png`) });
    }
    // Time sampling must not depend on earlier frames, including the loop reset.
    const later = await page.evaluate(() => window.renderDemoFrame(16.5));
    await page.evaluate(() => window.renderDemoFrame(0));
    const repeated = await page.evaluate(() => window.renderDemoFrame(16.5));
    assert.deepEqual(repeated, later);
    assert.equal((await page.evaluate(() => window.renderDemoFrame(20))).phase, 'idle');
    assert.deepEqual(pageErrors, []);
    console.log(`Start, intermediate, result and loop previews: ${path.relative(projectRoot, previews)}`);
    if (options.preview) return;

    for (let frame = 0; frame < frameCount; frame++) {
      await page.evaluate((t) => window.renderDemoFrame(t), frame / fps);
      await page.screenshot({ path: path.join(frameRoot, `frame-${String(frame).padStart(5, '0')}.png`) });
      if ((frame + 1) % 120 === 0) console.log(`Captured ${frame + 1}/${frameCount} frames.`);
    }
    assert.equal((await readdir(frameRoot)).length, frameCount);
    assert.deepEqual(pageErrors, []);
    await browser.close();
    await mkdir(outputRoot, { recursive: true });
    const mp4 = path.join(outputRoot, 'demo.mp4');
    const gif = path.join(outputRoot, 'demo.gif');
    const input = ['-y', '-v', 'error', '-framerate', String(fps), '-i', path.join(frameRoot, 'frame-%05d.png')];
    await run(ffmpeg, [...input, '-c:v', 'libx264', '-preset', 'medium', '-crf', '20', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', mp4]);
    const filters = `fps=${gifFps},scale=${gifWidth}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle`;
    await run(ffmpeg, [...input, '-filter_complex', filters, '-loop', '0', gif]);
    const metadata = [
      await inspectMetadata(mp4, { width, height, frames: frameCount }),
      await inspectMetadata(gif, { width: gifWidth, height: height * gifWidth / width, frames: duration * gifFps }),
    ];
    // Decode previews from the encoded files, rather than trusting source screenshots.
    for (const [name, time] of [['start', 0], ['selection', 3.2], ['recording', 8.5], ['result', 16.5], ['loop', 19.8]]) {
      for (const [kind, file] of [['mp4', mp4], ['gif', gif]]) {
        await run(ffmpeg, ['-y', '-v', 'error', ...(kind === 'gif' ? ['-ignore_loop', '1'] : []), '-ss', String(time), '-i', file, '-frames:v', '1', path.join(previews, `${kind}-${name}.png`)]);
      }
    }
    await writeFile(path.join(workRoot, 'metadata.json'), JSON.stringify({ version, source: 'operation mock', artifacts: metadata }, null, 2) + '\n');
    console.log(JSON.stringify(metadata, null, 2));
    console.log('Both outputs decoded to the end; encoded previews saved for visual inspection.');
  } finally {
    await browser.close();
    await rm(frameRoot, { recursive: true, force: true });
  }
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
