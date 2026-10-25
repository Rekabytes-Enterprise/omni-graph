import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, mkdirSync, copyFileSync, readFileSync, writeFileSync, readdirSync, rmSync, existsSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {spawnSync} from 'node:child_process';
import {DatabaseSync} from 'node:sqlite';

const binary = resolve('target/debug', process.platform === 'win32' ? 'omni-graph.exe' : 'omni-graph');
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'omni-upgrade-'));
  const home = join(root, 'memory'); const project = join(root, 'project'); mkdirSync(project);
  const env = {...process.env, HOME:join(root,'shell'), USERPROFILE:join(root,'shell'), OMNI_GRAPH_HOME:home};
  const run = (...args) => spawnSync(binary, ['--directory',project,...args], {env, encoding:'utf8'});
  const ok = (...args) => { const r=run(...args,'--json');assert.equal(r.status,0,r.stderr||r.stdout);return JSON.parse(r.stdout); };
  ok('init','--agent','none');ok('list');
  return {root,home,project,env,run,ok,close:()=>rmSync(root,{recursive:true,force:true})};
}
function schema3(path) {
  const db=new DatabaseSync(join(path,'memory.sqlite'));
  for(const {name,sql} of db.prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' AND name LIKE 'guard_%'").all()) {
    db.exec(`DROP TRIGGER "${name}"`);
    if(!name.includes('creation_receipts')) db.exec(sql.replace(/<> 4/g,'<> 3'));
  }
  db.exec('DROP TABLE creation_receipts; PRAGMA user_version=3;'); db.close();
}
function version(path) {const db=new DatabaseSync(join(path,'memory.sqlite'));try{return db.prepare('PRAGMA user_version').get().user_version;}finally{db.close();}}

test('installation prepares registered old stores for an unchanged API 1 client',()=>{
  const f=fixture();try {
    const memory=f.ok('remember','--title','Existing client memory','--body','preserved upgrade evidence');
    schema3(f.home);schema3(join(f.project,'.omni-graph'));
    assert.equal(f.run('compatibility','--api-version','1','--json').status,11);
    const installed=f.ok('install');assert.equal(installed.ready,true);
    assert.equal(installed.preparation.ready,true);
    // Legacy consumer checks compatibility before issuing ordinary API1 operations.
    const report=f.ok('compatibility','--api-version','1');assert.equal(report.status,'compatible');
    const shown=f.ok('show',memory.id,'--api-version','1');assert.equal(shown.memory.body,'preserved upgrade evidence');
    for(const path of [f.home,join(f.project,'.omni-graph')]) {
      assert.equal(version(path),4);
      assert.equal(readdirSync(path).filter(x=>x.startsWith('memory.sqlite.backup-v3-')).length,1);
    }
    f.ok('install');assert.equal(readdirSync(f.home).filter(x=>x.startsWith('memory.sqlite.backup-v3-')).length,1);
  }finally{f.close();}
});

test('registered preparation skips stale and mismatched paths without creating stores',()=>{
  const f=fixture();try {
    const registryPath=join(f.home,'registry.json');const registry=JSON.parse(readFileSync(registryPath));
    const missing=join(f.root,'removed-project');const wrong=join(f.root,'different-project');mkdirSync(wrong);mkdirSync(join(wrong,'.omni-graph'));
    const config=JSON.parse(readFileSync(join(f.project,'.omni-graph/project.json')));
    writeFileSync(join(wrong,'.omni-graph/project.json'),JSON.stringify(config));
    registry.projects.push({id:'project_'+'a'.repeat(32),name:'missing',path:missing},{id:'project_'+'b'.repeat(32),name:'wrong identity',path:wrong});
    writeFileSync(registryPath,JSON.stringify(registry));
    const report=f.ok('prepare','--registered','--initialize');
    assert.equal(report.ready,true);assert.equal(report.skipped_registrations.length,2);
    assert.equal(existsSync(missing),false);assert.equal(existsSync(join(wrong,'.omni-graph/memory.sqlite')),false);
    assert.deepEqual(JSON.parse(readFileSync(registryPath)),registry);
  }finally{f.close();}
});

test('installation refuses a registered future store before migrating another store',()=>{
  const f=fixture();try {
    schema3(f.home);const projectStore=join(f.project,'.omni-graph');
    const db=new DatabaseSync(join(projectStore,'memory.sqlite'));db.exec('PRAGMA user_version=999');db.close();
    const before=readFileSync(join(projectStore,'memory.sqlite'));
    const r=f.run('install','--json');assert.notEqual(r.status,0);
    const report=JSON.parse(r.stdout);assert.equal(report.ready,false);assert.equal(report.installed,false);
    assert.equal(version(f.home),3);assert.deepEqual(readFileSync(join(projectStore,'memory.sqlite')),before);
    assert.equal(existsSync(join(f.home,'bin')),false);
  }finally{f.close();}
});


test('late installation failure retains the new engine to resume partial migrations',()=>{
  const f=fixture();try {
    const saved=f.ok('remember','--title','Survives partial install','--body','Keep this evidence');
    schema3(f.home);const store=join(f.project,'.omni-graph');
    const db=new DatabaseSync(join(store,'memory.sqlite'));
    const triggers=db.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger'").all();
    for(const t of triggers) db.exec(`DROP TRIGGER "${t.name}"`);
    db.exec('ALTER TABLE memory_search RENAME TO memory_fts');
    for(const t of triggers.filter(t=>t.name.startsWith('memories_'))) db.exec(t.sql.replaceAll('memory_search','memory_fts'));
    db.exec('CREATE TRIGGER guard_memories_INSERT BEFORE INSERT ON memories BEGIN SELECT 1; END; PRAGMA user_version=2');
    db.close();
    const failed=f.run('install','--json');assert.notEqual(failed.status,0);
    const report=JSON.parse(failed.stdout);assert.equal(report.installed,true);assert.equal(report.ready,false);
    assert.ok(existsSync(report.binary));
    assert.equal(spawnSync(report.binary,['--version'],{env:f.env,encoding:'utf8'}).status,0);
    assert.equal(version(f.home),4);assert.equal(version(store),2);
    assert.equal(report.preparation.stores.at(-1).action,'failed');
    assert.ok(readdirSync(store).some(x=>x.startsWith('memory.sqlite.backup-v2-')));
    // Remove the injected schema-2 collision and resume using the retained engine.
    const repair=new DatabaseSync(join(store,'memory.sqlite'));
    repair.exec('DROP TRIGGER guard_memories_INSERT; DROP TABLE creation_receipts');repair.close();
    const resumed=spawnSync(report.binary,['prepare','--registered','--initialize','--json'],{env:f.env,cwd:f.project,encoding:'utf8'});
    assert.equal(resumed.status,0,resumed.stderr||resumed.stdout);assert.equal(JSON.parse(resumed.stdout).ready,true);
    const shown=spawnSync(report.binary,['--directory',f.project,'show',saved.id,'--json'],{env:f.env,encoding:'utf8'});
    assert.equal(shown.status,0,shown.stderr);assert.equal(JSON.parse(shown.stdout).memory.body,'Keep this evidence');
  }finally{f.close();}
});


test('historical binary-only self-update needs one explicit preparation before old-client readiness',()=>{
  const f=fixture();try {
    const saved=f.ok('remember','--title','Historical updater transition');
    schema3(f.home);schema3(join(f.project,'.omni-graph'));
    // Released 0.2.7 self-update only replaces the executable. Model that exact
    // boundary without a network download: it cannot execute newly added hooks.
    mkdirSync(join(f.home,'bin'),{recursive:true});
    const selected=join(f.home,'bin',process.platform==='win32'?'omni-graph.exe':'omni-graph');
    copyFileSync(binary,selected);
    const run=(...args)=>spawnSync(selected,['--directory',f.project,...args,'--json'],{env:f.env,encoding:'utf8'});
    assert.equal(run('compatibility','--api-version','1').status,11);
    const prepared=run('prepare','--registered','--initialize');
    assert.equal(prepared.status,0,prepared.stderr||prepared.stdout);
    assert.equal(run('compatibility','--api-version','1').status,0);
    assert.equal(JSON.parse(run('show',saved.id,'--api-version','1').stdout).memory.id,saved.id);
  }finally{f.close();}
});
