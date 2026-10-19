import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const packageVersion = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
const unix = process.platform !== 'win32';
function fixture(corrupt) {
  const temp = mkdtempSync(join(tmpdir(), 'omni-graph-installer-'));
  const downloads = join(temp, 'downloads');
  const mock = join(temp, 'tools');
  mkdirSync(downloads); mkdirSync(mock);
  const machine = spawnSync('uname', ['-m'], { encoding: 'utf8' }).stdout.trim();
  const platform = `${process.platform}-${['arm64', 'aarch64'].includes(machine) ? 'arm64' : 'x64'}`;
  const asset = `omni-graph-${platform}.tar.gz`;
  const archived = spawnSync('tar', ['-czf', join(downloads, asset), '-C', join(root, 'target/debug'), 'omni-graph'], { encoding: 'utf8' });
  assert.equal(archived.status, 0, archived.stderr);
  const digest = corrupt ? '0'.repeat(64) : createHash('sha256').update(readFileSync(join(downloads, asset))).digest('hex');
  writeFileSync(join(downloads, 'SHA256SUMS'), `${digest}  ${asset}\n`);
  // Local transport fixture: never contacts GitHub or bypasses the checksum step.
  writeFileSync(join(mock, 'curl'), `#!${process.execPath}
const fs = require('node:fs'); const path = require('node:path');
const args = process.argv.slice(2); const url = args.at(-1);
if (!url.startsWith('https://github.com/Rekabytes-Enterprise/omni-graph/releases/')) process.exit(2);
fs.appendFileSync(process.env.INSTALL_TEST_LOG, url + '\\n');
fs.copyFileSync(path.join(process.env.INSTALL_TEST_DOWNLOADS, url.split('/').at(-1)), args[args.indexOf('--output') + 1]);
`, { mode: 0o755 });
  return { temp, env: { ...process.env, PATH: `${mock}:${process.env.PATH}`, OMNI_GRAPH_HOME: join(temp, 'user memory'), OMNI_GRAPH_VERSION: packageVersion, INSTALL_TEST_LOG: join(temp, 'requests'), INSTALL_TEST_DOWNLOADS: downloads } };
}

test('curl installer verifies a pinned release and installs into the user memory home', { skip: !unix }, () => {
  const f = fixture(false);
  try {
    const installed = spawnSync('sh', [join(root, 'install.sh')], { env: f.env, encoding: 'utf8' });
    assert.equal(installed.status, 0, installed.stderr);
    const binary = join(f.env.OMNI_GRAPH_HOME, 'bin/omni-graph');
    assert.ok(existsSync(binary));
    assert.ok(existsSync(join(f.env.OMNI_GRAPH_HOME, 'memory.sqlite')));
    assert.deepEqual(JSON.parse(readFileSync(join(f.env.OMNI_GRAPH_HOME, 'registry.json'), 'utf8')), { clients: [], projects: [] });
    const version = spawnSync(binary, ['--version'], { encoding: 'utf8' });
    assert.equal(version.status, 0, version.stderr);
    assert.match(version.stdout, /omni-graph/);
    assert.ok(readFileSync(f.env.INSTALL_TEST_LOG, 'utf8').includes(`/releases/download/v${packageVersion}/`));
    assert.match(installed.stdout, /export PATH=/);
    delete f.env.OMNI_GRAPH_VERSION;
    const latest = spawnSync('sh', [join(root, 'install.sh')], { env: f.env, encoding: 'utf8' });
    assert.equal(latest.status, 0, latest.stderr);
    assert.match(readFileSync(f.env.INSTALL_TEST_LOG, 'utf8'), /\/releases\/latest\/download\//);
  } finally { rmSync(f.temp, { recursive: true, force: true }); }
});

test('curl installer rejects a corrupt archive before changing the destination', { skip: !unix }, () => {
  const f = fixture(true);
  try {
    const rejected = spawnSync('sh', [join(root, 'install.sh')], { env: f.env, encoding: 'utf8' });
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /checksum mismatch/);
    assert.ok(!existsSync(f.env.OMNI_GRAPH_HOME));
  } finally { rmSync(f.temp, { recursive: true, force: true }); }
});
