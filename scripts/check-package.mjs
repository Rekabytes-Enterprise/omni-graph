import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
if (!cargo.includes(`version = "${pkg.version}"`)) throw new Error('Cargo/npm versions must match.');
for (const platform of ['darwin-arm64', 'darwin-x64', 'linux-x64', 'linux-arm64', 'win32-x64']) {
  const file = join(root, 'native', platform, platform.startsWith('win32') ? 'omni-graph.exe' : 'omni-graph');
  if (!existsSync(file)) throw new Error(`Missing release binary: ${file}. Download the assembled release artifact before publishing.`);
}
console.log(`Release ${pkg.version}: all native binaries present.`);
