import { appendFileSync, copyFileSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateReleaseVersion } from './release-channel.mjs';

export function validateSource(run, repository, version, packageVersion) {
  const sameRepo = value => String(value || '').toLowerCase() === repository.toLowerCase();
  if (run.path !== '.github/workflows/nightly.yml' || run.head_branch !== 'main' ||
      !['workflow_dispatch', 'schedule'].includes(run.event) ||
      !sameRepo(run.repository?.full_name) || !sameRepo(run.head_repository?.full_name) ||
      run.repository?.id !== run.head_repository?.id ||
      !Number.isSafeInteger(run.id) || run.id <= 0 || !/^[a-f0-9]{40}$/.test(run.head_sha)) {
    throw new Error('Expected a main-branch nightly run from this repository.');
  }
  const match = /^(.+-nightly\.\d{8})\+([a-f0-9]{7,40})$/.exec(String(version));
  if (!match || !run.head_sha.startsWith(match[2])) {
    throw new Error('The original full nightly version must match the source commit.');
  }
  const assetVersion = validateReleaseVersion('nightly', match[1]);
  if (packageVersion !== undefined && assetVersion.split('-nightly.')[0] !== packageVersion) {
    throw new Error('The nightly version must match the original package version.');
  }
  return { source_sha: run.head_sha, nightly_version: version };
}

export function reusableArtifacts(run, jobs, artifacts) {
  return ['windows-x64', 'macos-arm64'].map(platform => {
    const producers = jobs.jobs.filter(job => job.name === `Build Nightly Artifacts / Package (${platform})`);
    if (producers.length !== 1 || producers[0].run_id !== run.id ||
        producers[0].status !== 'completed' || producers[0].conclusion !== 'success') {
      throw new Error(`The original ${platform} producer must have succeeded before reuse.`);
    }
    const matches = artifacts.artifacts.filter(artifact => artifact.name === `openbitfun-nightly-${platform}-bundle`);
    if (matches.length !== 1) throw new Error(`Expected one ${platform} artifact.`);
    const artifact = matches[0];
    if (!Number.isSafeInteger(artifact.id) || artifact.id <= 0 ||
        !Number.isSafeInteger(artifact.size_in_bytes) || artifact.size_in_bytes <= 0 || artifact.expired ||
        artifact.workflow_run?.id !== run.id || artifact.workflow_run?.head_sha !== run.head_sha ||
        artifact.workflow_run?.repository_id !== run.repository.id ||
        artifact.workflow_run?.head_repository_id !== run.repository.id) {
      throw new Error(`The ${platform} artifact is expired, empty, or from another source.`);
    }
    return artifact.id;
  });
}

export function verifyInstaller(bytes, version) {
  const offset = bytes.length >= 64 ? bytes.readUInt32LE(0x3c) : 0;
  const assetVersion = validateReleaseVersion('nightly', version.split('+')[0]);
  if (bytes.toString('ascii', 0, 2) !== 'MZ' || offset < 64 || offset + 4 > bytes.length ||
      !bytes.subarray(offset, offset + 4).equals(Buffer.from('PE\0\0')) ||
      !bytes.includes(Buffer.from(`ProductVersion\0${assetVersion}\0`, 'utf16le'))) {
    throw new Error('The Windows installer format or ProductVersion does not match the source nightly.');
  }
}

export function stagePackages(root, output, version) {
  const assetVersion = validateReleaseVersion('nightly', version.split('+')[0]);
  verifyInstaller(readFileSync(path.join(output, `OpenBitFun_${assetVersion}_windows-x86_64-installer.exe`)), version);
  const walk = directory => readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? walk(file) : entry.isFile() ? [file] : [];
  });
  const files = walk(root);
  for (const name of [`OpenBitFun_${assetVersion}_aarch64.dmg`, `OpenBitFun_${assetVersion}_amd64.deb`]) {
    const matches = files.filter(file => path.basename(file) === name);
    if (matches.length !== 1 || statSync(matches[0]).size === 0) {
      throw new Error(`Expected one nonempty package named ${name}.`);
    }
    copyFileSync(matches[0], path.join(output, name));
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const command = (file, args) => execFileSync(file, args, { encoding: 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024 });
    const api = endpoint => JSON.parse(command('gh', ['api', endpoint]));
    const repository = process.env.GITHUB_REPOSITORY;
    const runId = process.env.SOURCE_RUN_ID;
    const version = process.env.NIGHTLY_VERSION;
    if (!/^[1-9][0-9]*$/.test(runId || '')) throw new Error('A valid source run ID is required.');
    const run = api(`repos/${repository}/actions/runs/${runId}`);
    if (String(run.id) !== runId) throw new Error('The source run ID does not match.');
    validateSource(run, repository, version);
    command('git', ['merge-base', '--is-ancestor', run.head_sha, 'origin/main']);
    const sourcePackage = JSON.parse(command('git', ['show', `${run.head_sha}:package.json`]));
    const outputs = validateSource(run, repository, version, sourcePackage.version);
    if (process.argv[2] === 'ready') {
      const jobs = api(`repos/${repository}/actions/runs/${runId}/jobs?per_page=100`);
      const artifacts = api(`repos/${repository}/actions/runs/${runId}/artifacts?per_page=100`);
      outputs.artifact_ids = reusableArtifacts(run, jobs, artifacts).join(',');
    } else if (process.argv[2] === 'stage') {
      stagePackages('release-assets', 'nightly-release-assets', version);
    } else if (process.argv[2] !== 'resolve') {
      throw new Error('Expected resolve, ready, or stage.');
    }
    if (process.env.GITHUB_OUTPUT) {
      for (const [name, value] of Object.entries(outputs)) appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${value}\n`);
    }
    console.log(`Validated nightly recovery source ${runId} at ${run.head_sha}.`);
  } catch (error) {
    console.error(`[nightly-recovery] ${error.message}`);
    process.exitCode = 1;
  }
}
