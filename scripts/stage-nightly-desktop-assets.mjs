import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const formats = {
  'linux-x64': ['.deb', '.rpm', '.AppImage'],
  'linux-arm64': ['.deb', '.rpm', '.AppImage'],
  'macos-arm64': ['.dmg'],
  'macos-x64': ['.dmg'],
};

function filesUnder(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = join(directory, entry.name);
    if (entry.isSymbolicLink()) throw new Error(`Unexpected symbolic link: ${file}`);
    return entry.isDirectory() ? filesUnder(file) : [file];
  });
}

export function stageNightlyDesktopAssets(sourceDir, outputDir) {
  const selected = [];
  const names = new Set();
  // Validate every producer before modifying the publication directory.
  for (const [platform, extensions] of Object.entries(formats)) {
    const directory = join(sourceDir, `openbitfun-nightly-${platform}-bundle`);
    if (!existsSync(directory)) throw new Error(`Missing nightly artifact: ${platform}`);
    const files = filesUnder(directory);
    for (const extension of extensions) {
      const matches = files.filter(file => file.endsWith(extension));
      if (matches.length !== 1) throw new Error(`Expected one ${platform} ${extension} installer, found ${matches.length}`);
      const file = matches[0];
      const name = basename(file);
      if (readFileSync(file).length === 0) throw new Error(`Empty installer: ${file}`);
      if (names.has(name) || existsSync(join(outputDir, name)) || existsSync(join(outputDir, `${name}.sha256`))) {
        throw new Error(`Duplicate publication asset: ${name}`);
      }
      names.add(name);
      selected.push(file);
    }
  }
  mkdirSync(outputDir, { recursive: true });
  for (const file of selected) {
    const name = basename(file);
    const digest = createHash('sha256').update(readFileSync(file)).digest('hex');
    copyFileSync(file, join(outputDir, name));
    writeFileSync(join(outputDir, `${name}.sha256`), `${digest}  ${name}\n`);
  }
  return selected.map(file => basename(file));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [sourceDir, outputDir] = process.argv.slice(2);
  if (!sourceDir || !outputDir) throw new Error('Expected artifact source and publication directories');
  const staged = stageNightlyDesktopAssets(sourceDir, outputDir);
  console.log(`Staged ${staged.length} Linux and macOS installers with SHA-256 checksums.`);
}
