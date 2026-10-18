import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { stageNightlyDesktopAssets } from './stage-nightly-desktop-assets.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'nightly-desktop-assets-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, 'source');
  const output = join(root, 'output');
  const installers = [];
  for (const platform of ['linux-x64', 'linux-arm64', 'macos-arm64', 'macos-x64']) {
    const directory = join(source, `openbitfun-nightly-${platform}-bundle`, 'target/release/bundle');
    mkdirSync(directory, { recursive: true });
    for (const extension of platform.startsWith('linux') ? ['.deb', '.rpm', '.AppImage'] : ['.dmg']) {
      const file = join(directory, `OpenBitFun_1.0.6-nightly.20261018_${platform}${extension}`);
      writeFileSync(file, `installer ${platform} ${extension}`);
      installers.push(file);
    }
    writeFileSync(join(directory, 'internal-resource'), 'must not be published');
  }
  return { source, output, installers };
}

test('stages every architecture with verifiable checksums and retains Windows assets', t => {
  const { source, output, installers } = fixture(t);
  mkdirSync(output);
  writeFileSync(join(output, 'windows.exe'), 'existing Windows installer');
  const names = stageNightlyDesktopAssets(source, output);
  assert.equal(names.length, 8);
  for (const [index, name] of names.entries()) {
    const data = readFileSync(installers[index]);
    assert.deepEqual(readFileSync(join(output, name)), data);
    assert.equal(readFileSync(join(output, `${name}.sha256`), 'utf8'),
      `${createHash('sha256').update(data).digest('hex')}  ${name}\n`);
  }
  assert.equal(readFileSync(join(output, 'windows.exe'), 'utf8'), 'existing Windows installer');
  assert.equal(existsSync(join(output, 'internal-resource')), false);
});

for (const scenario of ['missing producer', 'missing format', 'empty installer', 'duplicate name']) {
  test(`rejects ${scenario} before staging any packages`, t => {
    const { source, output, installers } = fixture(t);
    if (scenario === 'missing producer') rmSync(join(source, 'openbitfun-nightly-macos-x64-bundle'), { recursive: true });
    if (scenario === 'missing format') rmSync(installers[0]);
    if (scenario === 'empty installer') writeFileSync(installers.at(-1), '');
    if (scenario === 'duplicate name') {
      mkdirSync(output);
      const name = installers[0].split(/[\\/]/).at(-1);
      writeFileSync(join(output, name), 'already staged');
    }
    assert.throws(() => stageNightlyDesktopAssets(source, output), /Missing nightly artifact|Expected one|Empty installer|Duplicate publication asset/);
    if (scenario !== 'duplicate name') assert.equal(existsSync(output), false);
  });
}
