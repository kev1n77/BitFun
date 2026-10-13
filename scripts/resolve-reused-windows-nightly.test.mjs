import assert from 'node:assert/strict';
import test from 'node:test';
import { resolveReusedWindowsNightly, verifyWindowsNightlyInstaller } from './resolve-reused-windows-nightly.mjs';

function fixture() {
  const sha = '3a76a4cfc2fc4a63a7b1da1d7b8d69cb902c7a25';
  return {
    repository: 'test-owner/OpenBitFun',
    packageVersion: '1.0.5',
    version: '1.0.5-nightly.20261013+3a76a4cfc',
    run: {
      id: 123, status: 'completed', conclusion: 'success',
      path: '.github/workflows/nightly.yml', head_branch: 'main', head_sha: sha,
      event: 'workflow_dispatch',
      repository: { id: 10, full_name: 'test-owner/OpenBitFun' },
      head_repository: { id: 10, full_name: 'test-owner/OpenBitFun' },
    },
    artifacts: { artifacts: [{
      id: 456, name: 'openbitfun-nightly-windows-x64-bundle',
      expired: false, size_in_bytes: 100,
      workflow_run: { id: 123, head_sha: sha, repository_id: 10, head_repository_id: 10 },
    }] },
  };
}

test('reuses the original source, version and artifact instead of the publisher revision', () => {
  const input = fixture();
  assert.deepEqual(resolveReusedWindowsNightly(input), {
    nightly_version: input.version,
    source_sha: input.run.head_sha,
    source_run_id: '123',
    artifact_id: '456',
  });
});

for (const [name, mutate] of [
  ['failed source', input => { input.run.conclusion = 'failure'; }],
  ['unfinished source', input => { input.run.status = 'in_progress'; }],
  ['another workflow', input => { input.run.path = '.github/workflows/ci.yml'; }],
  ['another branch', input => { input.run.head_branch = 'test'; }],
  ['pull request event', input => { input.run.event = 'pull_request'; }],
  ['fork source', input => { input.run.head_repository.full_name = 'fork/OpenBitFun'; }],
  ['another repository', input => { input.run.repository.full_name = 'another/OpenBitFun'; }],
  ['expired artifact', input => { input.artifacts.artifacts[0].expired = true; }],
  ['empty artifact', input => { input.artifacts.artifacts[0].size_in_bytes = 0; }],
  ['another run artifact', input => { input.artifacts.artifacts[0].workflow_run.id = 124; }],
  ['another commit artifact', input => { input.artifacts.artifacts[0].workflow_run.head_sha = 'a'.repeat(40); }],
  ['another repository artifact', input => { input.artifacts.artifacts[0].workflow_run.repository_id = 11; }],
  ['missing Windows artifact', input => { input.artifacts.artifacts = []; }],
  ['multiple Windows artifacts', input => { input.artifacts.artifacts.push(structuredClone(input.artifacts.artifacts[0])); }],
  ['mismatched version', input => { input.version = '1.0.6-nightly.20261013+3a76a4cfc'; }],
  ['mismatched version source', input => { input.version = '1.0.5-nightly.20261013+abcdef0'; }],
  ['missing original version', input => { input.version = ''; }],
  ['injected version', input => { input.version += '\nsource_sha=bad'; }],
]) {
  test('rejects ' + name + ' before publication', () => {
    const input = fixture();
    mutate(input);
    assert.throws(() => resolveReusedWindowsNightly(input));
  });
}

test('validates the installer product version before renaming or publishing it', () => {
  const bytes = Buffer.alloc(256);
  bytes.write('MZ', 0);
  bytes.writeUInt32LE(64, 0x3c);
  bytes.write('PE\0\0', 64);
  Buffer.from('ProductVersion\0' + '1.0.5-nightly.20261013' + '\0', 'utf16le').copy(bytes, 80);
  verifyWindowsNightlyInstaller(bytes, '1.0.5-nightly.20261013+3a76a4cfc');
  assert.throws(() => verifyWindowsNightlyInstaller(bytes, '1.0.5-nightly.20261014+3a76a4cfc'), /ProductVersion/);
  assert.throws(() => verifyWindowsNightlyInstaller(Buffer.from('invalid'), '1.0.5-nightly.20261013'), /PE executable/);
});
