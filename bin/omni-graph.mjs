#!/usr/bin/env node
import { existsSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const executable = process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph';
const platform = `${process.platform}-${process.arch}`;
const candidates = process.env.OMNI_GRAPH_BINARY
  ? [resolve(process.env.OMNI_GRAPH_BINARY)]
  : [join(root, 'native', platform, executable)];

// Source-checkout convenience only. Published packages do not contain Cargo.toml.
if (!process.env.OMNI_GRAPH_BINARY && existsSync(join(root, 'Cargo.toml'))) {
  candidates.unshift(join(root, 'target', 'release', executable), join(root, 'target', 'debug', executable));
}
const binary = candidates.find(existsSync);
if (!binary) {
  console.error(`omni-graph: no native binary available for ${platform}.`);
  if (existsSync(join(root, 'Cargo.toml'))) console.error('In a source checkout, run cargo build first.');
  else console.error('Supported release platforms: macOS x64/arm64, Linux x64/arm64, Windows x64. Reinstall a release containing your platform.');
  process.exit(1);
}
const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: false });
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => { if (!child.killed) child.kill(signal); });
}
child.on('error', (error) => {
  console.error(`omni-graph: could not launch native binary: ${error.message}`);
  process.exitCode = 1;
});
child.on('exit', (code, signal) => {
  process.exitCode = code ?? (signal === 'SIGINT' ? 130 : 1);
});
