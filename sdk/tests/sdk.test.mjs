import test from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  rmSync,
  chmodSync,
  readdirSync,
  copyFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { DatabaseSync } from "node:sqlite";
import { OmniGraph, OmniGraphError } from "../dist/index.js";
const binary =
  process.env.OMNI_GRAPH_SDK_BINARY ||
  resolve(
    "../target/debug",
    process.platform === "win32" ? "omni-graph.exe" : "omni-graph",
  );
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), "omni-sdk-")),
    project = join(dir, "project"),
    home = join(dir, "home");
  mkdirSync(project);
  const env = {
    ...process.env,
    OMNI_GRAPH_HOME: home,
    HOME: join(dir, "shell"),
    USERPROFILE: join(dir, "shell"),
  };
  const run = (...args) => {
    const out = spawnSync(binary, ["--directory", project, ...args], {
      env,
      encoding: "utf8",
    });
    assert.equal(out.status, 0, out.stderr || out.stdout);
    return out.stdout;
  };
  run("init", "--agent", "none", "--json");
  run("list", "--json");
  return {
    dir,
    project,
    home,
    env,
    run,
    client: (options) =>
      new OmniGraph({
        executable: binary,
        directory: project,
        env,
        ...options,
      }),
    close: () => rmSync(dir, { recursive: true, force: true }),
  };
}
function downgrade(path) {
  const db = new DatabaseSync(path);
  const triggers = db
    .prepare(
      "SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name LIKE 'guard_%'",
    )
    .all();
  for (const row of triggers) db.exec(`DROP TRIGGER "${row.name}"`);
  db.exec("DROP TABLE creation_receipts");
  for (const row of triggers.filter(
    (row) => !row.name.includes("creation_receipts"),
  ))
    db.exec(row.sql.replace("<> 4", "<> 3"));
  db.exec("PRAGMA user_version=3;");
  db.close();
}
test("real engine: typed reads, atomic links, safe keyed retries and unchanged API1 methods", async () => {
  const f = fixture();
  try {
    const client = f.client();
    const report = await client.connect();
    assert.equal(report.status, "compatible");
    const issue = await client.remember(
      { title: "SDK issue", type: "issue", body: "retry connection failure" },
      { idempotencyKey: "sdk-issue" },
    );
    const fix = await client.remember(
      {
        title: "SDK fix",
        type: "fix",
        body: "reconnect after engine upgrade",
        links: [{ from: issue.id, relation: "corrected_by" }],
      },
      { idempotencyKey: "sdk-fix" },
    );
    assert.deepEqual(
      await client.remember(
        {
          title: "SDK fix",
          type: "fix",
          body: "reconnect after engine upgrade",
          links: [{ from: issue.id, relation: "corrected_by" }],
        },
        { idempotencyKey: "sdk-fix" },
      ),
      fix,
    );
    await assert.rejects(
      client.remember(
        { title: "Different request" },
        { idempotencyKey: "sdk-fix" },
      ),
      (e) => e instanceof OmniGraphError && e.code === "idempotency_conflict",
    );
    assert.equal((await client.list()).length, 2);
    assert.equal((await client.show(issue.id)).edges[0].to, fix.id);
    assert.ok(
      (await client.search("retry")).some((x) => x.memory.id === issue.id),
    );
    assert.ok(
      (await client.recall("retry")).results.some(
        (x) => x.memory.id === issue.id,
      ),
    );
    assert.equal(
      (await client.update(fix.id, { body: "verified correction" })).body,
      "verified correction",
    );
    assert.equal(
      (
        await client.link({
          from: fix.id,
          relation: "related_to",
          to: issue.id,
        })
      ).to,
      issue.id,
    );
  } finally {
    f.close();
  }
});
test("existing SDK instance reconnects after store schema changes; manual policy remains read-only", async () => {
  const f = fixture();
  try {
    const client = f.client();
    const saved = await client.remember(
      { title: "Before update" },
      { idempotencyKey: "before" },
    );
    const db = join(f.project, ".omni-graph", "memory.sqlite");
    downgrade(db);
    const before = readFileSync(db);
    const manual = f.client({ migration: "manual" });
    await assert.rejects(
      manual.connect(),
      (e) => e.code === "connection_not_ready",
    );
    assert.deepEqual(readFileSync(db), before);
    const [first, second] = await Promise.all([
      client.reconnect(),
      client.connect(),
    ]);
    assert.equal(first.status, "compatible");
    assert.deepEqual(first, second);
    assert.equal((await client.show(saved.id)).memory.title, "Before update");
    const backups = readdirSync(join(f.project, ".omni-graph")).filter((x) =>
      x.startsWith("memory.sqlite.backup-v3-"),
    );
    assert.equal(backups.length, 1);
    await client.reconnect();
    assert.equal(
      readdirSync(join(f.project, ".omni-graph")).filter((x) =>
        x.startsWith("memory.sqlite.backup-v3-"),
      ).length,
      1,
    );
  } finally {
    f.close();
  }
});
test("missing stores require explicit initialization; unsupported schema is never downgraded", async () => {
  const f = fixture();
  try {
    rmSync(f.home, { recursive: true, force: true });
    await assert.rejects(
      f.client().connect(),
      (e) => e.code === "connection_not_ready",
    );
    assert.equal(
      (await f.client({ initialize: true }).connect()).status,
      "compatible",
    );
    const db = join(f.project, ".omni-graph", "memory.sqlite");
    const sql = new DatabaseSync(db);
    sql.exec("PRAGMA user_version=99");
    sql.close();
    const before = readFileSync(db);
    await assert.rejects(
      f.client({ initialize: true }).connect(),
      (e) => e.code === "connection_not_ready",
    );
    assert.deepEqual(readFileSync(db), before);
  } finally {
    f.close();
  }
});
function fakeFixture() {
  const dir = mkdtempSync(join(tmpdir(), "omni-sdk-fake-"));
  const executable = join(dir, "engine");
  const script = join(dir, "engine.mjs");
  writeFileSync(
    executable,
    `#!/bin/sh\nexec '${process.execPath.replaceAll("'", "'\\''")}' '${script.replaceAll("'", "'\\''")}' "$@"\n`,
  );
  chmodSync(executable, 0o755);
  return {
    dir,
    executable,
    script,
    close: () => rmSync(dir, { recursive: true, force: true }),
  };
}
const fakeOptions = { skip: process.platform === "win32" };
test(
  "missing executable, malformed response, output bound and timeout produce typed errors",
  fakeOptions,
  async () => {
    const f = fakeFixture();
    try {
      const options = {
        executable: f.executable,
        directory: f.dir,
        timeoutMs: 3000,
        maxOutputBytes: 512,
      };
      await assert.rejects(
        new OmniGraph({
          ...options,
          executable: join(f.dir, "absent"),
        }).inspect(),
        (e) => e.code === "executable_unavailable",
      );
      writeFileSync(f.script, "console.log('not json')");
      await assert.rejects(
        new OmniGraph(options).inspect(),
        (e) => e.code === "invalid_response",
      );
      writeFileSync(f.script, "console.log('x'.repeat(2048))");
      await assert.rejects(
        new OmniGraph(options).inspect(),
        (e) => e.code === "output_limit",
      );
      writeFileSync(f.script, "setInterval(()=>{},1000)");
      await assert.rejects(
        new OmniGraph({ ...options, timeoutMs: 100 }).inspect(),
        (e) => e.code === "timeout",
      );
    } finally {
      f.close();
    }
  },
);
test(
  "same configured executable path is rechecked after replacement; ambiguous writes are never replayed",
  fakeOptions,
  async () => {
    const f = fakeFixture();
    try {
      const base = {
        integration_api_version: 1,
        executable: { path: f.executable, version: "old" },
        supported_api_versions: [1],
        database: {
          read_versions: [4],
          write_versions: [4],
          migratable_versions: [3],
          migration_target: 4,
          migration_policy: "automatic_on_open_with_backup",
        },
        capabilities: ["memory_crud"],
        stores: [
          {
            path: f.dir,
            status: "compatible",
            detected_schema_version: 4,
            migration_required: false,
            migration_supported: false,
            recovery: "ready",
          },
        ],
        status: "compatible",
        exit_code: 0,
      };
      const counter = join(f.dir, "writes");
      const script = (report) =>
        `import {appendFileSync} from 'node:fs';if(process.argv.includes('compatibility'))console.log(${JSON.stringify(JSON.stringify(report))});else {appendFileSync(${JSON.stringify(counter)},'write\\n');console.log('broken response');}`;
      writeFileSync(f.script, script(base));
      const client = new OmniGraph({
        executable: f.executable,
        directory: f.dir,
      });
      assert.equal((await client.connect()).executable.version, "old");
      writeFileSync(
        f.script,
        script({ ...base, executable: { ...base.executable, version: "new" } }),
      );
      assert.equal((await client.reconnect()).executable.version, "new");
      await assert.rejects(
        client.remember({ title: "single attempt" }),
        (e) => e.code === "invalid_response",
      );
      assert.equal(readFileSync(counter, "utf8"), "write\n");
      const migration = {
        ...base,
        status: "migration_required",
        exit_code: 11,
        stores: [
          {
            ...base.stores[0],
            status: "migration_required",
            migration_required: true,
            migration_supported: true,
          },
        ],
      };
      writeFileSync(
        f.script,
        `console.log(${JSON.stringify(JSON.stringify(migration))});process.exit(11)`,
      );
      await assert.rejects(
        client.connect(),
        (e) => e.code === "unsupported_capability",
      );
    } finally {
      f.close();
    }
  },
);

test("packed SDK imports from a fresh consumer and ships usable declarations", async () => {
  const f = fixture();
  try {
    const npm = process.env.npm_execpath;
    const npmEnv = {...f.env, npm_config_cache: join(f.dir, "npm-cache")};
    assert.ok(npm, "Run this test through npm test");
    const pack = spawnSync(
      process.execPath,
      [npm, "pack", "--ignore-scripts", "--json", "--pack-destination", f.dir],
      { cwd: resolve("."), encoding: "utf8", env: npmEnv },
    );
    assert.equal(pack.status, 0, pack.stderr);
    const archive = join(f.dir, JSON.parse(pack.stdout)[0].filename);
    const consumer = join(f.dir, "consumer");
    mkdirSync(consumer);
    writeFileSync(
      join(consumer, "package.json"),
      JSON.stringify({ name: "sdk-consumer", private: true, type: "module" }),
    );
    const install = spawnSync(
      process.execPath,
      [
        npm,
        "install",
        "--offline",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
        archive,
      ],
      { cwd: consumer, encoding: "utf8", env: npmEnv },
    );
    assert.equal(install.status, 0, install.stderr);
    writeFileSync(
      join(consumer, "run.mjs"),
      `import {OmniGraph} from '@omni-graph/sdk';
      const client=new OmniGraph(${JSON.stringify({ executable: binary, directory: f.project, env: {OMNI_GRAPH_HOME:f.home, HOME:f.env.HOME, USERPROFILE:f.env.USERPROFILE} })});
      console.log((await client.connect()).status);`,
    );
    const run = spawnSync(process.execPath, ["run.mjs"], {
      cwd: consumer,
      encoding: "utf8",
    });
    assert.equal(run.status, 0, run.stderr);
    assert.equal(run.stdout.trim(), "compatible");
    writeFileSync(
      join(consumer, "check.ts"),
      `import {OmniGraph, type MemoryInput} from '@omni-graph/sdk';
      const input: MemoryInput={title:'consumer',scope:'project'};
      const client=new OmniGraph({executable:'/absolute/binary',directory:'/absolute/project',env:{CUSTOM:'value'}});
      async function use(){const result=await client.remember(input);const fields:string[]=[...result.prevention,...result.sources,result.task_id??''];return fields;}
      void use;`,
    );
    const compiler = resolve("node_modules/typescript/bin/tsc");
    const check = spawnSync(
      process.execPath,
      [
        compiler,
        "--strict",
        "--noEmit",
        "--target",
        "ES2022",
        "--module",
        "NodeNext",
        "--moduleResolution",
        "NodeNext",
        "check.ts",
      ],
      { cwd: consumer, encoding: "utf8", env: npmEnv },
    );
    assert.equal(check.status, 0, check.stdout || check.stderr);
  } finally {
    f.close();
  }
});

test(
  "same SDK client survives a real older executable being replaced by the new engine",
  { skip: !process.env.OMNI_GRAPH_SDK_LEGACY_BINARY },
  async () => {
    const f = fixture();
    try {
      const project = join(f.dir, "old-project");
      mkdirSync(project);
      const home = join(f.dir, "old-home");
      const env = { ...f.env, OMNI_GRAPH_HOME: home };
      const executable = join(
        f.dir,
        process.platform === "win32" ? "updatable.exe" : "updatable",
      );
      copyFileSync(process.env.OMNI_GRAPH_SDK_LEGACY_BINARY, executable);
      chmodSync(executable, 0o755);
      const init = spawnSync(
        executable,
        ["--directory", project, "init", "--agent", "none", "--json"],
        { env, encoding: "utf8" },
      );
      assert.equal(init.status, 0, init.stderr);
      const list = spawnSync(
        executable,
        ["--directory", project, "list", "--json"],
        { env, encoding: "utf8" },
      );
      assert.equal(list.status, 0, list.stderr);
      const client = new OmniGraph({ executable, directory: project, env });
      const before = await client.remember({
        title: "Written through older API 1",
      });
      assert.equal((await client.show(before.id)).memory.title, before.title);
      copyFileSync(binary, executable);
      chmodSync(executable, 0o755);
      assert.equal((await client.reconnect()).status, "compatible");
      assert.equal((await client.show(before.id)).memory.title, before.title);
    } finally {
      f.close();
    }
  },
);
