import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { DatabaseSync } from 'node:sqlite';
import Ajv from 'ajv';

const binary = resolve('target/debug', process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph');
const schema = new Ajv().compile(JSON.parse(readFileSync(new URL('../contracts/preparation-v1.schema.json', import.meta.url))));
function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'omni-prepare-'));
  const home = join(dir, 'home'), project = join(dir, 'project'); mkdirSync(project);
  const env = {...process.env, OMNI_GRAPH_HOME:home, HOME:join(dir,'shell'), USERPROFILE:join(dir,'shell')};
  const args = (...input) => ['--directory', project, ...input];
  const run = (...input) => spawnSync(binary,args(...input),{env,encoding:'utf8'});
  const asyncRun = (...input) => new Promise((resolve, reject) => {
    const process = spawn(binary,args(...input),{env}); let stdout='', stderr='';
    process.stdout.on('data',chunk=>stdout+=chunk); process.stderr.on('data',chunk=>stderr+=chunk);
    process.on('error',reject); process.on('close',status=>resolve({status,stdout,stderr}));
  });
  return {dir,home,project,run,asyncRun,close:()=>rmSync(dir,{recursive:true,force:true})};
}
function report(result, exit) {
  assert.equal(result.stderr,''); if(exit!==undefined) assert.equal(result.status,exit,result.stdout);
  const value=JSON.parse(result.stdout); assert.ok(schema(value),JSON.stringify(schema.errors));
  assert.equal(value.exit_code,result.status); return value;
}
function initialize(f) {
  assert.equal(f.run('init','--agent','none','--json').status,0);
  assert.equal(f.run('list','--json').status,0);
  return join(f.project,'.omni-graph');
}
function downgrade3(store) {
  const db=new DatabaseSync(join(store,'memory.sqlite'));
  const triggers=db.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name LIKE 'guard_%'").all();
  for(const t of triggers) db.exec(`DROP TRIGGER "${t.name}"`);
  db.exec('DROP TABLE creation_receipts');
  for(const t of triggers.filter(t=>!t.name.includes('creation_receipts'))) db.exec(t.sql.replace('<> 4','<> 3'));
  db.exec('PRAGMA user_version=3'); db.close();
}
function files(store) {
  return Object.fromEntries(readdirSync(store).sort().map(n=>[n,readFileSync(join(store,n)).toString('base64')]));
}
function failMigration2(store) {
  const db=new DatabaseSync(join(store,'memory.sqlite'));
  const triggers=db.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'").all();
  for(const t of triggers) db.exec(`DROP TRIGGER "${t.name}"`);
  db.exec('ALTER TABLE memory_search RENAME TO memory_fts');
  for(const t of triggers.filter(t=>t.name.startsWith('memories_'))) db.exec(t.sql.replaceAll('memory_search','memory_fts'));
  db.exec('CREATE TRIGGER guard_memories_INSERT BEFORE INSERT ON memories BEGIN SELECT 1; END; PRAGMA user_version=2');
  db.close();
}

test('prepare migrates existing global/project schema 3, preserves API1 and repeats without backups',()=>{
  const f=fixture(); try {
    const store=initialize(f);
    const saved=JSON.parse(f.run('remember','--title','Existing client evidence','--body','durable evidence','--json').stdout);
    downgrade3(f.home); downgrade3(store);
    const before=[files(f.home),files(store)];
    assert.equal(f.run('compatibility','--api-version','1','--json').status,11);
    assert.deepEqual([files(f.home),files(store)],before);
    const prepared=report(f.run('prepare','--api-version','1','--json'),0);
    assert.equal(prepared.ready,true); assert.deepEqual(prepared.stores.map(s=>s.action),['migrated','migrated']);
    assert.ok(prepared.compatibility.capabilities.includes('storage_preparation'));
    assert.equal(JSON.parse(f.run('show',saved.id,'--api-version','1','--json').stdout).memory.id,saved.id);
    assert.ok(JSON.parse(f.run('recall','evidence','--api-version','1','--json').stdout).results.some(r=>r.memory.id===saved.id));
    const backups=[f.home,store].map(p=>readdirSync(p).filter(n=>n.startsWith('memory.sqlite.backup-')));
    assert.ok(backups.every(names=>names.length===1));
    const again=report(f.run('prepare','--json'),0); assert.ok(again.stores.every(s=>s.action==='unchanged'));
    assert.deepEqual([f.home,store].map(p=>readdirSync(p).filter(n=>n.startsWith('memory.sqlite.backup-'))),backups);
  } finally {f.close();}
});

test('missing and legacy stores require initialize, global preparation needs no project',()=>{
 const f=fixture(); try {
   const refused=report(f.run('prepare','--scope','global','--json'),10);
   assert.equal(refused.stores[0].action,'blocked'); assert.deepEqual(readdirSync(f.dir),['project']);
   assert.equal(report(f.run('prepare','--scope','global','--initialize','--json'),0).stores[0].action,'initialized');
   const legacy=join(f.dir,'legacy'); mkdirSync(legacy);
   const text=JSON.stringify({schema_version:1,memories:[],edges:[]});writeFileSync(join(legacy,'graph.json'),text);
   assert.equal(report(f.run('prepare','--store',legacy,'--json'),11).stores[0].action,'blocked');
   assert.deepEqual(readdirSync(legacy),['graph.json']);
   assert.equal(report(f.run('prepare','--store',legacy,'--initialize','--json'),0).stores[0].action,'imported');
   assert.equal(readFileSync(join(legacy,'graph.json'),'utf8'),text);
 } finally {f.close();}
});

test('a future store blocks every planned migration without mutating files',()=>{
 const f=fixture(); try {
   const store=initialize(f); downgrade3(f.home);
   const path=join(store,'memory.sqlite'), bytes=readFileSync(path);bytes.writeUInt32BE(999,60);writeFileSync(path,bytes);
   const before=[files(f.home),files(store)];
   const value=report(f.run('prepare','--initialize','--json'),12);
   assert.equal(value.ready,false);assert.deepEqual(value.stores.map(s=>s.action),['not_attempted','blocked']);
   assert.deepEqual([files(f.home),files(store)],before);
 } finally {f.close();}
});

test('partial migration reports completed stores and preserves failed-store backup',()=>{
 const f=fixture(); try {
   const store=initialize(f); downgrade3(f.home);failMigration2(store);
   const value=report(f.run('prepare','--json'),11);
   assert.equal(value.ready,false);assert.deepEqual(value.stores.map(s=>s.action),['migrated','failed']);
   assert.equal(value.stores[1].error.code,'migration_failed');
   assert.equal(value.stores[0].after.status,'compatible');assert.equal(value.stores[1].after.detected_schema_version,2);
   assert.ok(readdirSync(store).some(n=>n.startsWith('memory.sqlite.backup-v2-')));
   const again=report(f.run('prepare','--json'),11);
   assert.equal(again.stores[0].action,'unchanged');assert.equal(again.stores[1].action,'failed');
 } finally {f.close();}
});

test('concurrent preparation safely completes one shared migration',async()=>{
 const f=fixture(); try {
   initialize(f); downgrade3(f.home);
   const outcomes=await Promise.all(Array.from({length:4},()=>f.asyncRun('prepare','--scope','global','--json')));
   // A changing discovery snapshot may legitimately report busy; a bounded caller
   // retry must reach ready without duplicate migration or lost records.
   for(const output of outcomes) { const value=report(output); assert.ok(value.ready || value.compatibility.status==='busy'); }
   assert.equal(report(f.run('prepare','--scope','global','--json'),0).ready,true);
   assert.equal(readdirSync(f.home).filter(n=>n.startsWith('memory.sqlite.backup-v3-')).length,1);
 } finally {f.close();}
});

test('invalid legacy content remains intact and is never replaced by an empty database',()=>{
 const f=fixture(); try {
   const legacy=join(f.dir,'legacy');mkdirSync(legacy);
   const text='private invalid legacy contents';writeFileSync(join(legacy,'graph.json'),text);
   const value=report(f.run('prepare','--store',legacy,'--initialize','--json'),11);
   assert.equal(value.ready,false);assert.equal(value.stores[0].action,'failed');
   assert.ok(!JSON.stringify(value).includes(text));
   assert.equal(readFileSync(join(legacy,'graph.json'),'utf8'),text);
   assert.ok(!readdirSync(legacy).includes('memory.sqlite'));
 } finally {f.close();}
});
