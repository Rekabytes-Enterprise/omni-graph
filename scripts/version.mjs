import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const stable = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const cargoPattern = /(^\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m;
const lockPattern = /(\[\[package\]\]\r?\nname = "omni-graph"\r?\nversion = ")([^"]+)(")/;
function parts(version) {
  if (!stable.test(version)) throw new Error(`Expected a stable version such as 0.1.1, got ${version}`);
  const parsed = version.split('.').map(Number);
  if (!parsed.every(Number.isSafeInteger)) throw new Error('Version components exceed safe integer range.');
  return parsed;
}
function source(directory) {
  const packageText = readFileSync(join(directory, 'package.json'), 'utf8');
  const pkg = JSON.parse(packageText);
  const cargo = readFileSync(join(directory, 'Cargo.toml'), 'utf8');
  const lock = readFileSync(join(directory, 'Cargo.lock'), 'utf8');
  parts(pkg.version);
  if (cargo.match(cargoPattern)?.[2] !== pkg.version || lock.match(lockPattern)?.[2] !== pkg.version) {
    throw new Error('package.json, Cargo.toml, and the Omni-graph entry in Cargo.lock must have the same version.');
  }
  return { pkg, cargo, lock };
}
export function readVersion(directory = root) { return source(directory).pkg.version; }
export function nextVersion(current, requested) {
  const before = parts(current);
  let version;
  if (['patch', 'minor', 'major'].includes(requested)) {
    const index = { major: 0, minor: 1, patch: 2 }[requested];
    const next = [...before];
    next[index]++;
    for (let i = index + 1; i < 3; i++) next[i] = 0;
    version = next.join('.');
  } else {
    version = String(requested ?? '').replace(/^v/, '');
  }
  const after = parts(version);
  const different = after.findIndex((value, i) => value !== before[i]);
  if (different < 0 || after[different] < before[different]) throw new Error(`Version must increase beyond ${current}.`);
  return version;
}
export function bumpVersion(directory, requested) {
  const { pkg, cargo, lock } = source(directory);
  const previous = pkg.version;
  const version = nextVersion(previous, requested);
  const updatedCargo = cargo.replace(cargoPattern, (_, start, old, end) => start + version + end);
  const updatedLock = lock.replace(lockPattern, (_, start, old, end) => start + version + end);
  // Validate every input before writing; only the package's own lock entry changes.
  writeFileSync(join(directory, 'package.json'), JSON.stringify({ ...pkg, version }, null, 2) + '\n');
  writeFileSync(join(directory, 'Cargo.toml'), updatedCargo);
  writeFileSync(join(directory, 'Cargo.lock'), updatedLock);
  return { previous, version };
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const action = process.argv[2];
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/version.mjs check|patch|minor|major|VERSION');
    if (action === 'check') console.log(`Version ${readVersion()}: package.json, Cargo.toml, Cargo.lock agree.`);
    else {
      const result = bumpVersion(root, action);
      console.log(`${result.previous} → ${result.version}\nUpdated package.json, Cargo.toml, and Cargo.lock. No commit or tag was created.`);
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
