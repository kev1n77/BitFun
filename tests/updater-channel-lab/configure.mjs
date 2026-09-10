import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

export const labDir = dirname(fileURLToPath(import.meta.url));

export function releasePlan(repository, prefix, version) {
  if (repository !== 'kev1n77/BitFun') throw new Error('This lab only publishes to kev1n77/BitFun');
  if (!/^updater-lab-\d{8}(?:-[a-z0-9]+)?$/.test(prefix)) throw new Error('Invalid test release prefix');
  if (!['0.2.19', '0.2.20'].includes(version)) throw new Error('Only the two simulated versions are allowed');
  const receiverTag = `${prefix}-0.2.19`;
  const tag = `${prefix}-${version}`;
  const root = `https://github.com/${repository}/releases/download/${receiverTag}`;
  return {
    repository, prefix, version, tag, receiverTag,
    endpoint: `${root}/channel-legacy.json`,
    controlEndpoint: `${root}/channel-control.json`,
    downloadPage: `https://github.com/${repository}/releases/tag/${prefix}-0.2.20`,
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const version = process.argv[2];
  const plan = releasePlan(process.env.GITHUB_REPOSITORY, process.env.LAB_TAG_PREFIX, version);
  const publicKey = process.env.TAURI_UPDATER_PUBKEY?.trim();
  if (!publicKey) throw new Error('TAURI_UPDATER_PUBKEY is required');
  const config = JSON.parse(readFileSync(join(labDir, 'tauri.base.json'), 'utf8'));
  config.version = version;
  config.plugins.updater.endpoints = [plan.endpoint];
  config.plugins.updater.pubkey = publicKey;
  writeFileSync(join(labDir, 'src-tauri/tauri.conf.json'), JSON.stringify(config, null, 2) + '\n');
  const cargoPath = join(labDir, 'src-tauri/Cargo.toml');
  const cargo = readFileSync(cargoPath, 'utf8').replace(/^version = "0\.2\.(19|20)"$/m, `version = "${version}"`);
  writeFileSync(cargoPath, cargo);
  mkdirSync(join(labDir, 'out'), { recursive: true });
  writeFileSync(join(labDir, 'out/plan.json'), JSON.stringify(plan, null, 2) + '\n');
  console.log(`Configured receiver ${version}: ${plan.endpoint}`);
}
