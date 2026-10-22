import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, readFileSync, writeFileSync, readdirSync, rmSync, chmodSync, statSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve, toNamespacedPath} from 'node:path';
import {spawnSync} from 'node:child_process';
import Ajv from 'ajv';
import { DatabaseSync } from 'node:sqlite';

const ajv = new Ajv();
const reportSchema = ajv.compile(JSON.parse(readFileSync(new URL('../contracts/compatibility-v1.schema.json', import.meta.url))));
const errorSchema = ajv.compile(JSON.parse(readFileSync(new URL('../contracts/error-v1.schema.json', import.meta.url))));
const binary = resolve('target/debug', process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph');
function fixture() {
  const dir = mkdtempSync(join(tmpdir(),'omni-contract-'));
  const home = join(dir,'home'); const project = join(dir,'project'); mkdirSync(project);
  const run = (...args) => spawnSync(binary, ['--directory',project,...args], {encoding:'utf8',env:{...process.env, OMNI_GRAPH_HOME:home, HOME:join(dir,'shell'), USERPROFILE:join(dir,'shell')}});
  return {dir,home,project,run,close:()=>rmSync(dir,{recursive:true,force:true})};
}
function check(out, schema, exit) {
  assert.equal(out.status,exit,out.stderr || out.stdout);
  assert.equal(out.stderr,'');
  const value=JSON.parse(out.stdout); assert.ok(schema(value),JSON.stringify(schema.errors)); return value;
}
test('discovery missing/compatible, capabilities, exact executable and selection',()=>{
 const f=fixture(); try {
   const missing=check(f.run('compatibility','--json'),reportSchema,10);
   assert.equal(missing.stores[0].path,f.home); assert.deepEqual(readdirSync(f.dir),['project']);
   check(f.run('compatibility','--store',join(f.dir,'absent'),'--json'),reportSchema,10);
   assert.equal(f.run('init','--agent','none','--json').status,0);
   assert.equal(f.run('list','--json').status,0);
   const report=check(f.run('compatibility','--api-version','1','--json'),reportSchema,0);
   assert.equal(toNamespacedPath(report.executable.path),toNamespacedPath(binary)); assert.equal(report.stores.length,2);
   assert.deepEqual(report.database.read_versions,[3]); assert.deepEqual(report.database.migratable_versions,[1,2]);
   assert.ok(report.capabilities.includes('writer_fencing'));
   assert.equal(check(f.run('compatibility','--scope','global','--json'),reportSchema,0).stores.length,1);
   const selected=check(f.run('compatibility','--scope','project','--json'),reportSchema,0).stores[0].path;
   // Windows short names (RUNNER~1) and canonical names can identify the same directory.
   const identity=path=>{const s=statSync(path,{bigint:true});return [s.dev,s.ino];};
   assert.deepEqual(identity(selected),identity(join(f.project,'.omni-graph')));
 }finally{f.close();}
});
test('corruption and permission errors are schema-valid and do not alter files',()=>{
 const f=fixture(); try {
   mkdirSync(f.home); const db=join(f.home,'memory.sqlite'); writeFileSync(db,'secret memory text, invalid SQLite');
   const before=readFileSync(db);
   const report=check(f.run('compatibility','--json'),reportSchema,14); assert.equal(report.status,'invalid_database');
   assert.ok(!JSON.stringify(report).includes('secret memory text')); assert.deepEqual(readFileSync(db),before); assert.deepEqual(readdirSync(f.home),['memory.sqlite']);
   if(process.platform!=='win32' && process.getuid?.()!==0){chmodSync(db,0);try {assert.equal(check(f.run('compatibility','--json'),reportSchema,13).status,'permission_denied');}finally{chmodSync(db,0o600);}}
 }finally{f.close();}
});
test('API negotiation and structured error opt-in preserve legacy stderr behavior',()=>{
 const f=fixture(); try {
   assert.equal(check(f.run('compatibility','--api-version','99','--json'),errorSchema,17).error.code,'unsupported_api_version');
   assert.equal(check(f.run('compatibility','--bogus','--json'),errorSchema,2).error.code,'invalid_arguments');
   assert.equal(check(f.run('compatibility','--scope','project','--json'),errorSchema,16).error.code,'store_selection_failed');
   check(f.run('show','mem_missing','--api-version','1','--json'),errorSchema,1);
   const query=f.run('search','compatibility','--bogus','--json'); assert.equal(query.stdout,''); assert.ok(query.stderr.includes('error:'));
   const legacy=f.run('show','mem_missing','--json'); assert.equal(legacy.status,1); assert.equal(legacy.stdout,''); assert.equal(typeof JSON.parse(legacy.stderr).error,'string');
   assert.deepEqual(readdirSync(f.dir),['project']);
 }finally{f.close();}
});

test('newer schema is reported and refused with stable structured code, preserving files',()=>{
 const f=fixture(); try {
   assert.equal(f.run('init','--agent','none','--json').status,0);
   assert.equal(f.run('list','--json').status,0);
   const store=join(f.project,'.omni-graph'); const db=join(store,'memory.sqlite');
   const bytes=readFileSync(db); bytes.writeUInt32BE(999,60); writeFileSync(db,bytes);
   const before=readdirSync(store).sort();
   const report=check(f.run('compatibility','--scope','project','--json'),reportSchema,12);
   assert.equal(report.stores[0].detected_schema_version,999); assert.equal(report.status,'unsupported_schema');
   const error=check(f.run('list','--api-version','1','--json'),errorSchema,12);
   assert.equal(error.error.code,'unsupported_schema');
   assert.deepEqual(readFileSync(db),bytes); assert.deepEqual(readdirSync(store).sort(),before);
 }finally{f.close();}
});

test('migration requirement is read-only and failed migration returns a validated recoverable error',()=>{
 const f=fixture(); try {
   assert.equal(f.run('init','--agent','none','--json').status,0);
   assert.equal(f.run('list','--json').status,0);
   const store=join(f.project,'.omni-graph'); const db=join(store,'memory.sqlite');
   // Build a schema-2 fixture with a deliberate name collision that fails late
   // in migration, after a valid backup and earlier transactional DDL.
   const sql = new DatabaseSync(db);
   const triggers = sql.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'").all();
   for (const trigger of triggers) sql.exec(`DROP TRIGGER "${trigger.name}"`);
   sql.exec('ALTER TABLE memory_search RENAME TO memory_fts');
   for (const trigger of triggers.filter(t=>t.name.startsWith('memories_'))) sql.exec(trigger.sql.replaceAll('memory_search','memory_fts'));
   sql.exec('CREATE TRIGGER guard_memories_INSERT BEFORE INSERT ON memories BEGIN SELECT 1; END; PRAGMA user_version=2');
   sql.close();
   const bytes=readFileSync(db);
   const before=readdirSync(store).sort();
   const report=check(f.run('compatibility','--scope','project','--json'),reportSchema,11);
   assert.equal(report.status,'migration_required'); assert.equal(report.stores[0].migration_supported,true);
   assert.deepEqual(readFileSync(db),bytes); assert.deepEqual(readdirSync(store).sort(),before);
   const error=check(f.run('list','--api-version','1','--json'),errorSchema,1);
   assert.equal(error.error.code,'migration_failed');
   assert.equal(check(f.run('compatibility','--scope','project','--json'),reportSchema,11).stores[0].detected_schema_version,2);
   assert.ok(readdirSync(store).some(name=>name.startsWith('memory.sqlite.backup-v2-')));
 }finally{f.close();}
});
