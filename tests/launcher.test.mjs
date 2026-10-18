import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const launcher = join(root, 'bin/omni-graph.mjs');
const executable = process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph';
const binary = join(root, 'target/debug', executable);
test('launcher forwards arguments and exit status to the Rust binary', () => {
  const version = spawnSync(process.execPath, [launcher, '--version'], { env: { ...process.env, OMNI_GRAPH_BINARY: binary }, encoding: 'utf8' });
  assert.equal(version.status, 0, version.stderr);
  assert.match(version.stdout, /omni-graph 0\.1\.0/);
  const invalid = spawnSync(process.execPath, [launcher, 'not-a-command'], { env: { ...process.env, OMNI_GRAPH_BINARY: binary }, encoding: 'utf8' });
  assert.equal(invalid.status, 2);
});
test('explicit missing binary fails instead of falling back silently', () => {
  const result = spawnSync(process.execPath, [launcher], { env: { ...process.env, OMNI_GRAPH_BINARY: join(root, 'missing-binary') }, encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no native binary/);
});

test('a packed npm package runs through offline npm exec without Rust sources', { skip: !process.env.npm_execpath }, async () => {
  const { mkdtempSync, mkdirSync, cpSync, copyFileSync, readFileSync, rmSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const sandbox = mkdtempSync(join(tmpdir(), 'omni-graph-npm-'));
  try {
    const stage = join(sandbox, 'package');
    const platform = `${process.platform}-${process.arch}`;
    const native = join(stage, 'native', platform);
    mkdirSync(native, { recursive: true });
    for (const name of ['package.json', 'README.md', 'CONTRIBUTING.md', 'LICENSE']) copyFileSync(join(root, name), join(stage, name));
    for (const name of ['bin', 'skills', 'assets']) cpSync(join(root, name), join(stage, name), { recursive: true });
    copyFileSync(binary, join(native, executable));
    const env = { ...process.env, NPM_CONFIG_CACHE: join(sandbox, 'cache'), NPM_CONFIG_UPDATE_NOTIFIER: 'false' };
    delete env.OMNI_GRAPH_BINARY;
    const npm = (args, cwd) => spawnSync(process.execPath, [process.env.npm_execpath, ...args], { cwd, env, encoding: 'utf8' });
    const packed = npm(['pack', '--ignore-scripts', '--json', '--pack-destination', sandbox], stage);
    assert.equal(packed.status, 0, packed.stderr);
    const manifest = JSON.parse(packed.stdout)[0];
    assert.ok(manifest.files.some(f => f.path === `native/${platform}/${executable}`));
    assert.ok(manifest.files.some(f => f.path === 'assets/omni-graph.svg'));
    assert.ok(manifest.files.some(f => f.path === 'CONTRIBUTING.md'));
    assert.ok(!manifest.files.some(f => f.path.startsWith('src/') || f.path === 'Cargo.toml'));
    const tarball = join(sandbox, manifest.filename);
    const launched = npm(['exec', '--offline', '--yes', `--package=${tarball}`, '--', 'omni-graph', '--version'], sandbox);
    assert.equal(launched.status, 0, launched.stderr);
    const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version;
    assert.equal(launched.stdout.trim(), `omni-graph ${version}`);
    const project = join(sandbox, 'project'); mkdirSync(project);
    env.OMNI_GRAPH_HOME = join(sandbox, 'home');
    const initialized = npm(['exec', '--offline', '--yes', `--package=${tarball}`, '--', 'omni-graph', 'init', '--json'], project);
    assert.equal(initialized.status, 0, initialized.stderr);
    assert.ok(JSON.parse(initialized.stdout).project.id.startsWith('project_'));
    assert.match(readFileSync(join(project, '.agents/skills/omni-graph/SKILL.md'), 'utf8'), /name: omni-graph/);
    const installed = npm(['exec', '--offline', '--yes', `--package=${tarball}`, '--', 'omni-graph', 'install', '--json'], sandbox);
    assert.equal(installed.status, 0, installed.stderr);
    const location = JSON.parse(installed.stdout);
    assert.equal(location.home, env.OMNI_GRAPH_HOME);
    assert.equal(location.binary, join(env.OMNI_GRAPH_HOME, 'bin', executable));
    const persistent = spawnSync(location.binary, ['--version'], { encoding: 'utf8' });
    assert.equal(persistent.status, 0, persistent.stderr);
    assert.equal(persistent.stdout.trim(), `omni-graph ${version}`);
    assert.ok(JSON.parse(readFileSync(join(location.home, 'registry.json'), 'utf8')).projects.length === 1);

  } finally {
    rmSync(sandbox, { recursive: true, force: true });
  }
});
