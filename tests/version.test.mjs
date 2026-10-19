import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readVersion, nextVersion, bumpVersion } from '../scripts/version.mjs';

test('stable version bumps handle patch, minor, major, and explicit versions', () => {
  assert.equal(nextVersion('0.1.0', 'patch'), '0.1.1');
  assert.equal(nextVersion('0.1.9', 'minor'), '0.2.0');
  assert.equal(nextVersion('0.9.9', 'major'), '1.0.0');
  assert.equal(nextVersion('0.1.0', 'v0.3.2'), '0.3.2');
  for (const invalid of ['0.1.0', '0.0.9', '1.01.0', '1.2', '1.2.3-beta', 'wat', undefined]) {
    assert.throws(() => nextVersion('0.1.0', invalid));
  }
});
test('version bump updates all three sources and preserves unrelated lock entries', () => {
  const root = mkdtempSync(join(tmpdir(), 'omni-graph-version-'));
  try {
    writeFileSync(join(root, 'package.json'), '{"name":"omni-graph","version":"0.1.0"}\n');
    writeFileSync(join(root, 'Cargo.toml'), '[package]\nname = "omni-graph"\nversion = "0.1.0"\n[dependencies]\nother = "0.1.0"\n');
    writeFileSync(join(root, 'Cargo.lock'), 'version = 4\n\n[[package]]\nname = "omni-graph"\nversion = "0.1.0"\n\n[[package]]\nname = "other"\nversion = "0.1.0"\n');
    assert.deepEqual(bumpVersion(root, 'patch'), { previous: '0.1.0', version: '0.1.1' });
    assert.equal(readVersion(root), '0.1.1');
    assert.match(readFileSync(join(root, 'Cargo.lock'), 'utf8'), /name = "other"\nversion = "0.1.0"/);
    assert.match(readFileSync(join(root, 'Cargo.toml'), 'utf8'), /other = "0.1.0"/);
    const before = readFileSync(join(root, 'Cargo.toml'), 'utf8');
    assert.throws(() => bumpVersion(root, '0.1.1'));
    assert.equal(readFileSync(join(root, 'Cargo.toml'), 'utf8'), before);
    writeFileSync(join(root, 'package.json'), '{"version":"0.2.0"}\n');
    assert.throws(() => bumpVersion(root, 'patch'), /same version/);
    assert.equal(readFileSync(join(root, 'Cargo.toml'), 'utf8'), before);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
