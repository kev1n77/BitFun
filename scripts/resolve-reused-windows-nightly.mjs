#!/usr/bin/env node
import { appendFileSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateReleaseVersion } from './release-channel.mjs';

export function verifyWindowsNightlyInstaller(bytes, version) {
  const assetVersion = validateReleaseVersion('nightly', String(version).split('+')[0]);
  const offset = bytes.length >= 64 ? bytes.readUInt32LE(0x3c) : 0;
  if (bytes.toString('ascii', 0, 2) !== 'MZ' || offset < 64 || offset + 4 > bytes.length ||
      !bytes.subarray(offset, offset + 4).equals(Buffer.from('PE\0\0'))) {
    throw new Error('The Windows nightly installer is not a valid PE executable.');
  }
  // Windows VERSIONINFO strings are UTF-16LE. Validate the packaged product
  // version before applying a release filename supplied by the dispatch caller.
  if (!bytes.includes(Buffer.from('ProductVersion\0' + assetVersion + '\0', 'utf16le'))) {
    throw new Error('The installer ProductVersion does not match the original nightly version.');
  }
}

export function resolveReusedWindowsNightly({ run, artifacts, repository, version, packageVersion }) {
  const sameRepository = value => String(value || '').toLowerCase() === repository.toLowerCase();
  if (run.status !== 'completed' || run.conclusion !== 'success') {
    throw new Error('Reusing a Windows nightly requires a completed successful run.');
  }
  if (run.path !== '.github/workflows/nightly.yml' || run.head_branch !== 'main' ||
      !['schedule', 'workflow_dispatch'].includes(run.event) ||
      !sameRepository(run.repository?.full_name) || !sameRepository(run.head_repository?.full_name)) {
    throw new Error('The source must be a main-branch Nightly Build from this repository.');
  }
  if (!Number.isSafeInteger(run.id) || run.id <= 0 || !/^[0-9a-f]{40}$/.test(run.head_sha)) {
    throw new Error('The source run has invalid commit or run metadata.');
  }
  const match = /^(.+-nightly\.\d{8})\+([0-9a-f]{7,40})$/.exec(String(version || ''));
  if (!match || !run.head_sha.startsWith(match[2])) {
    throw new Error('reuse_version must include the original nightly version and source commit suffix.');
  }
  const assetVersion = validateReleaseVersion('nightly', match[1]);
  if (assetVersion.split('-nightly.')[0] !== packageVersion) {
    throw new Error('The reused nightly version does not match the source package version.');
  }
  const matches = (artifacts.artifacts || []).filter(
    artifact => artifact.name === 'openbitfun-nightly-windows-x64-bundle',
  );
  if (matches.length !== 1) {
    throw new Error('Expected exactly one Windows nightly bundle in the successful source run.');
  }
  const artifact = matches[0];
  if (artifact.expired || !Number.isSafeInteger(artifact.id) || artifact.id <= 0 ||
      !Number.isSafeInteger(artifact.size_in_bytes) || artifact.size_in_bytes <= 0 ||
      artifact.workflow_run?.id !== run.id || artifact.workflow_run?.head_sha !== run.head_sha ||
      artifact.workflow_run?.repository_id !== run.repository.id ||
      artifact.workflow_run?.head_repository_id !== run.repository.id) {
    throw new Error('The Windows artifact is expired, empty, or does not belong to the source run.');
  }
  return {
    nightly_version: version,
    source_sha: run.head_sha,
    source_run_id: String(run.id),
    artifact_id: String(artifact.id),
  };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv[2] === '--verify-installer') {
      verifyWindowsNightlyInstaller(readFileSync(process.argv[3]), process.argv[4]);
      console.log('Verified the Windows nightly installer format and original product version.');
    } else {
      const readJson = file => JSON.parse(readFileSync(file, 'utf8'));
      const outputs = resolveReusedWindowsNightly({
        run: readJson('nightly-source-run.json'),
        artifacts: readJson('nightly-source-artifacts.json'),
        packageVersion: readJson('nightly-source-package.json').version,
        repository: process.env.GITHUB_REPOSITORY,
        version: process.env.REUSE_VERSION,
      });
      if (!process.env.GITHUB_OUTPUT) throw new Error('GITHUB_OUTPUT is required.');
      for (const [key, value] of Object.entries(outputs)) {
        appendFileSync(process.env.GITHUB_OUTPUT, key + '=' + value + '\n');
      }
      console.log('Validated Windows nightly artifact ' + outputs.artifact_id +
        ' from run ' + outputs.source_run_id + ' at ' + outputs.source_sha + '.');
    }
  } catch (error) {
    console.error('[windows-nightly] ' + error.message);
    process.exitCode = 1;
  }
}
