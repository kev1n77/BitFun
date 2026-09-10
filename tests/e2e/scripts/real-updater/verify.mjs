import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const [phase, outDir] = process.argv.slice(2);
assert.ok(['before', 'notification', 'after'].includes(phase));
assert.ok(outDir);
await mkdir(outDir, { recursive: true });
const base = 'http://127.0.0.1:4445';
const expectedNotes = (await readFile(new URL('./update-notes.txt', import.meta.url), 'utf8')).trim();
let session;
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

async function request(route, data) {
  const response = await fetch(base + route, {
    method: data === undefined ? 'GET' : 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: data === undefined ? undefined : JSON.stringify(data),
    signal: AbortSignal.timeout(130000),
  });
  const payload = await response.json();
  if (!response.ok || payload.value?.error) throw new Error(JSON.stringify(payload));
  return payload.value;
}

async function until(label, action, timeout = 120000) {
  const deadline = Date.now() + timeout;
  let last;
  while (Date.now() < deadline) {
    try { const result = await action(); if (result) return result; }
    catch (error) { last = error; }
    await sleep(2000);
  }
  throw new Error(`${label} timed out: ${last?.message ?? 'condition remained false'}`);
}

const execute = (script, args = []) => request(`/session/${session}/execute/sync`, { script, args });

async function invoke(command) {
  return request(`/session/${session}/execute/async`, {
    script: `const done = arguments[arguments.length - 1];
      window.__TAURI__.core.invoke(arguments[0], {request: {}})
        .then(value => done({value}), error => done({error: String(error)}));`,
    args: [command],
  }).then(result => {
    if (result.error) throw new Error(result.error);
    return result.value;
  });
}

async function snapshot(name) {
  const png = await request(`/session/${session}/screenshot`);
  const bytes = Buffer.from(png, 'base64');
  assert.equal(bytes.subarray(0, 8).toString('hex'), '89504e470d0a1a0a');
  await writeFile(path.join(outDir, `${name}.png`), bytes);
  const html = await execute('return document.documentElement.outerHTML;');
  await writeFile(path.join(outDir, `${name}.html`), html);
}

try {
  await until('Installed BitFun WebDriver startup', () => request('/status'));
  session = (await request('/session', { capabilities: { alwaysMatch: {} } })).sessionId;
  await request(`/session/${session}/timeouts`, { script: 120000 });
  await until('Real BitFun frontend', () => execute('return !!window.__TAURI__?.core?.invoke && document.body.innerText.length > 30;'));
  const version = await invoke('get_app_version');
  assert.equal(version, phase === 'after' ? '0.2.20' : '0.2.19');
  const update = await until('Published update channel', async () => {
    const result = await invoke('check_for_updates');
    return result.updateAvailable === (phase === 'notification') ? result : null;
  });
  assert.equal(update.currentVersion, version);
  const evidence = { phase, nativeVersion: version, update, testedAt: new Date().toISOString() };
  if (phase === 'notification') {
    assert.equal(update.latestVersion, '0.2.20');
    assert.equal(update.releaseNotes.replaceAll('\r\n', '\n'), expectedNotes.replaceAll('\r\n', '\n'));
    // This is the shipped DailyAppUpdateGate and UpdateAvailableDialog.
    // Do not inject a replacement UI, fake update responses, or alter React state.
    evidence.dialogText = await until('Original BitFun update dialog', () => execute(`
      const root = document.querySelector('[data-bf-component="update"][data-bf-part="availableRoot"]');
      return root && root.getBoundingClientRect().height > 0 && root.innerText.includes('OpenBitFun 1.0') ? root.innerText : null;`));
    assert.ok(evidence.dialogText.includes('0.2.19'));
    assert.ok(evidence.dialogText.includes('0.2.20'));
    assert.ok(evidence.dialogText.includes('无法通过当前版本直接升级'));
    await snapshot('real-bitfun-notification');
    await writeFile(path.join(outDir, `${phase}.json`), JSON.stringify(evidence, null, 2));
    // Click the real dialog's last action: background install. This invokes the
    // production update store, IPC command, minisign check and NSIS installer.
    const clicked = await execute(`
      const actions = document.querySelector('[data-bf-component="update"][data-bf-part="actions"]');
      const buttons = actions ? Array.from(actions.querySelectorAll('button')) : [];
      const button = buttons.at(-1);
      if (!button || button.disabled) return false;
      button.click(); return true;`);
    assert.equal(clicked, true);
  } else {
    await snapshot(`real-bitfun-${phase}`);
    await writeFile(path.join(outDir, `${phase}.json`), JSON.stringify(evidence, null, 2));
  }
  console.log(`Real BitFun ${phase} verification passed (${version}).`);
} catch (error) {
  if (session) {
    try { await snapshot(`failure-${phase}`); } catch {}
  }
  await writeFile(path.join(outDir, `failure-${phase}.json`), JSON.stringify({ error: String(error), stack: error.stack }, null, 2));
  throw error;
}
