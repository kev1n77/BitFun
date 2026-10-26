import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { validateSource, reusableArtifacts, verifyInstaller, stagePackages } from './nightly-linux-recovery.mjs';

const repository = 'test-owner/OpenBitFun';
const sourceSha = '5f34866ad1854546866c55b310b83e4bccaf3396';
const version = '1.0.7-nightly.20261026+5f34866ad';
const assetVersion = version.split('+')[0];
const platforms = ['windows-x64', 'macos-arm64'];
function fixture() {
  const run = { id: 123, head_sha: sourceSha, head_branch: 'main',
    path: '.github/workflows/nightly.yml', event: 'workflow_dispatch', conclusion: 'failure',
    repository: { id: 10, full_name: repository }, head_repository: { id: 10, full_name: repository } };
  return { run,
    jobs: { jobs: platforms.map(platform => ({ run_id: 123, name: `Build Nightly Artifacts / Package (${platform})`,
      status: 'completed', conclusion: 'success' })) },
    artifacts: { artifacts: platforms.map((platform, index) => ({ id: 40 + index,
      name: `openbitfun-nightly-${platform}-bundle`, expired: false, size_in_bytes: 100,
      workflow_run: { id: 123, head_sha: sourceSha, repository_id: 10, head_repository_id: 10 } })) } };
}
function installer() {
  const bytes = Buffer.alloc(256);
  bytes.write('MZ');
  bytes.writeUInt32LE(64, 0x3c);
  bytes.write('PE\0\0', 64);
  Buffer.from(`ProductVersion\0${assetVersion}\0`, 'utf16le').copy(bytes, 80);
  return bytes;
}

test('recovery accepts successful platform jobs from an otherwise failed nightly without relabelling source', () => {
  const { run, jobs, artifacts } = fixture();
  assert.deepEqual(validateSource(run, repository, version, '1.0.7'), {
    source_sha: sourceSha, nightly_version: version,
  });
  assert.deepEqual(reusableArtifacts(run, jobs, artifacts), [40, 41]);
});

for (const [name, mutate] of [
  ['fork run', data => { data.run.head_repository.full_name = 'fork/OpenBitFun'; }],
  ['other branch', data => { data.run.head_branch = 'feature'; }],
  ['other workflow', data => { data.run.path = '.github/workflows/ci.yml'; }],
  ['PR run', data => { data.run.event = 'pull_request'; }],
]) {
  test('rejects ' + name, () => {
    const data = fixture(); mutate(data);
    assert.throws(() => validateSource(data.run, repository, version, '1.0.7'));
  });
}
test('rejects a version with a different commit, base version or output injection', () => {
  const { run } = fixture();
  assert.throws(() => validateSource(run, repository, '1.0.7-nightly.20261026+abcdef0', '1.0.7'));
  assert.throws(() => validateSource(run, repository, version, '1.0.8'));
  assert.throws(() => validateSource(run, repository, version + '\nsource_sha=bad', '1.0.7'));
});
for (const [name, mutate] of [
  ['failed producer', data => { data.jobs.jobs[0].conclusion = 'failure'; }],
  ['running producer', data => { data.jobs.jobs[1].status = 'in_progress'; }],
  ['other producer run', data => { data.jobs.jobs[0].run_id = 124; }],
  ['missing producer', data => { data.jobs.jobs.pop(); }],
  ['missing artifact', data => { data.artifacts.artifacts.pop(); }],
  ['duplicate artifact', data => { data.artifacts.artifacts.push(structuredClone(data.artifacts.artifacts[0])); }],
  ['expired artifact', data => { data.artifacts.artifacts[0].expired = true; }],
  ['empty artifact', data => { data.artifacts.artifacts[0].size_in_bytes = 0; }],
  ['other artifact run', data => { data.artifacts.artifacts[0].workflow_run.id = 124; }],
  ['other artifact commit', data => { data.artifacts.artifacts[0].workflow_run.head_sha = 'a'.repeat(40); }],
  ['other artifact repository', data => { data.artifacts.artifacts[0].workflow_run.repository_id = 11; }],
]) {
  test('rejects reuse of ' + name, () => {
    const data = fixture(); mutate(data);
    assert.throws(() => reusableArtifacts(data.run, data.jobs, data.artifacts));
  });
}

test('stages exactly three installers and rejects incorrect or duplicate package versions', t => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'openbitfun-nightly-recovery-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const input = path.join(root, 'input');
  const output = path.join(root, 'output');
  mkdirSync(input); mkdirSync(output);
  const exe = `OpenBitFun_${assetVersion}_windows-x86_64-installer.exe`;
  const dmg = `OpenBitFun_${assetVersion}_aarch64.dmg`;
  const deb = `OpenBitFun_${assetVersion}_amd64.deb`;
  writeFileSync(path.join(output, exe), installer());
  writeFileSync(path.join(input, dmg), 'original Mac package');
  writeFileSync(path.join(input, deb), 'recovered DEB');
  writeFileSync(path.join(input, 'unwanted.rpm'), 'excluded');
  stagePackages(input, output, version);
  assert.deepEqual(readdirSync(output).sort(), [exe, dmg, deb].sort());
  assert.equal(readFileSync(path.join(output, dmg), 'utf8'), 'original Mac package');
  verifyInstaller(installer(), version);
  assert.throws(() => verifyInstaller(installer(), '1.0.7-nightly.20261027+5f34866ad'));
  assert.throws(() => verifyInstaller(Buffer.from('not a PE'), version));
  mkdirSync(path.join(input, 'duplicate'));
  writeFileSync(path.join(input, 'duplicate', deb), 'duplicate');
  assert.throws(() => stagePackages(input, output, version));
});

test('recovery builds only the original Linux source on Ubuntu 24.04 and reuses pinned Windows/Mac artifact IDs', () => {
  const require = createRequire(import.meta.url);
  const yaml = require('yaml');
  const read = file => yaml.parse(readFileSync(new URL('../' + file, import.meta.url), 'utf8'));
  const producer = read('.github/workflows/nightly-artifacts.yml');
  const platformsMap = JSON.parse(/fromJSON\('([^']+)'\)/.exec(producer.jobs.package['runs-on'])[1]);
  assert.equal(platformsMap['linux-x64'], 'ubuntu-24.04');
  assert.equal(platformsMap['linux-arm64'], 'ubuntu-24.04-arm');
  assert.equal(platformsMap['macos-arm64'], 'macos-15');
  assert.equal(platformsMap['windows-x64'], 'windows-latest');
  const workflow = read('.github/workflows/nightly-recover-linux.yml');
  const build = workflow.jobs['build-linux'];
  assert.equal(build.with.checkout_ref, '${{ needs.resolve.outputs.source_sha }}');
  assert.deepEqual(JSON.parse(build.with.desktop_platforms), ['linux-x64']);
  assert.equal(build.with.linux_bundles, 'deb');
  for (const name of ['build_macos_cli', 'build_linux_binaries', 'build_relay_image']) assert.equal(build.with[name], false);
  assert.notEqual(workflow.concurrency.group, workflow.jobs.publish.concurrency.group);
  assert.equal(workflow.jobs.publish.concurrency['cancel-in-progress'], false);
  const download = workflow.jobs.publish.steps.find(step => step.name === 'Download original successful packages');
  assert.equal(download.with['artifact-ids'], '${{ steps.reused.outputs.artifact_ids }}');
  assert.equal(download.with['run-id'], '${{ inputs.source_run_id }}');
  assert.equal(download.with['github-token'], '${{ github.token }}');
});

test('publication resumes drafts, uploads before cleanup, and leaves old assets intact if upload fails', t => {
  const require = createRequire(import.meta.url);
  const yaml = require('yaml');
  const workflow = yaml.parse(readFileSync(new URL('../.github/workflows/nightly-recover-linux.yml', import.meta.url), 'utf8'));
  const step = workflow.jobs.publish.steps.find(step => step.name === 'Publish recovered nightly');
  const root = mkdtempSync(path.join(os.tmpdir(), 'openbitfun-recovery-publish-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const scenario of ['published', 'draft', 'new', 'list-error', 'upload-error']) {
    const cwd = path.join(root, scenario);
    mkdirSync(cwd); mkdirSync(path.join(cwd, 'nightly-release-assets'));
    writeFileSync(path.join(cwd, 'nightly-release-assets', 'installer.exe'), 'fixture');
    writeFileSync(path.join(cwd, 'pages.json'), JSON.stringify([
      scenario === 'new' ? [] : [{ id: 42, tag_name: 'nightly', draft: scenario === 'draft' }],
    ]));
    writeFileSync(path.join(cwd, 'release.json'), JSON.stringify({ assets: [
      { id: 1, name: 'old-installer.exe' }, { id: 2, name: 'installer.exe' },
    ] }));
    const result = spawnSync('bash', ['-c', `
      git() { printf '%s\\trefs/tags/nightly\\n' "$SOURCE_SHA"; }
      gh() {
        printf '%s\\n' "$*" >> calls
        case "$1 $2" in
          "api repos/test/repo/releases?per_page=100")
            [[ "$SCENARIO" != list-error ]] || return 1
            cat pages.json ;;
          "api repos/test/repo/releases/42") cat release.json ;;
          "api --method") return 0 ;;
          "release view") printf '42' ;;
          "release upload") [[ "$SCENARIO" != upload-error ]] ;;
          "release create" | "release edit") return 0 ;;
          *) echo "Unexpected GitHub request: $*" >&2; return 1 ;;
        esac
      }
      ${step.run}
    `], { cwd, encoding: 'utf8', windowsHide: true, env: { ...process.env,
      SCENARIO: scenario, GITHUB_REPOSITORY: 'test/repo', SOURCE_SHA: sourceSha,
      SOURCE_RUN_ID: '123', GITHUB_RUN_ID: '124', NIGHTLY_VERSION: version } });
    const calls = readFileSync(path.join(cwd, 'calls'), 'utf8');
    assert.doesNotMatch(calls, /releases\/tags\//);
    if (scenario.endsWith('-error')) {
      assert.notEqual(result.status, 0);
      assert.doesNotMatch(calls, /--method DELETE|--draft=false/);
    } else {
      assert.equal(result.status, 0, `${scenario}: ${result.stderr}`);
      assert.equal(calls.includes('release create'), scenario === 'new');
      assert.ok(calls.indexOf('release upload') < calls.indexOf('--method DELETE'));
      assert.ok(calls.indexOf('--method DELETE') < calls.indexOf('--draft=false'));
      assert.match(calls, /DELETE repos\/test\/repo\/releases\/assets\/1/);
      assert.doesNotMatch(calls, /DELETE repos\/test\/repo\/releases\/assets\/2/);
    }
  }
});
