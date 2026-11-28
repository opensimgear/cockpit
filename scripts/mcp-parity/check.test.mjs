import assert from 'node:assert/strict';
import test from 'node:test';
import { discover } from './actions.mjs';
import { check, catalogue, main } from './check.mjs';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { originActions, originalFiles } from './baseline.mjs';

const ui = 'src/lib/Controls.svelte';
const initial = new Map([
  [ui, '<button onclick={save}>Save</button>'],
  ['src-tauri/src/mcp_tools/catalogue.rs', 'tool("save", "Save", vec![], &[])'],
  ['tests/ui/save.mjs', 'test("save persists", async () => { await callTool("save"); assert.equal(await read(), 1); });'],
]);
const row = Object.fromEntries(['workflow','ui','native','mcp','status','input','output','effect','guard','lifecycle','scenario'].map(field => [field, field]));
row.workflow = 'save';
row.status = 'supported';
const base = discover(initial), indexed = [...catalogue(initial)];
const run = (files, contracts = [], baseline = base, rows = [row]) => check(files, rows, baseline, contracts, indexed, ['save']);
const changed = () => new Map(initial).set(ui, initial.get(ui) + '<button on:click={reset}>Reset</button>');
const contract = files => ({ workflow: 'save', tools: ['save'], tests: [{path:'tests/ui/save.mjs',anchor:'save persists'}], actions: discover(files).filter(action => action.anchor.includes('reset')) });

test('unchanged audited actions pass; markup-only edits do not need opaque fingerprints', () => {
  assert.deepEqual(run(initial), []);
  assert.deepEqual(run(new Map(initial).set(ui, initial.get(ui).replace('Save</button>', 'Persist</button>'))), []);
});
test('loop-generated catalogue tools are indexed along with extended catalogue modules', () => {
  const files = new Map(initial);
  files.set('src-tauri/src/mcp_tools/companion_catalogue.rs', 'for (name,description) in [("arm","Arm"),("disarm","Disarm")] {tools.push(tool(name,description,vec![],&[]));}');
  assert.deepEqual([...catalogue(files)].sort(), ['arm','disarm','save']);
});
test('baseline cannot grandfather new actions and reviewed workflows cannot disappear', () => {
  const files = changed();
  assert.match(run(files).join('\n'), /Unmapped UI\/native action/);
  assert.match(run(initial,[],base,[]).join('\n'), /Missing reviewed inventory workflow/);
  assert.match(run(initial,[],base,[row,{...row,workflow:'new workflow'}]).join('\n'), /New inventory workflow/);
  const source = initial.get(ui);
  const runGit = (_git,args) => args[0] === 'ls-tree' ? Buffer.from(ui+'\0') : Buffer.from('abc blob '+Buffer.byteLength(source)+'\n'+source+'\n');
  assert.deepEqual(originActions('.',runGit),base);
  assert.equal(originalFiles('.',runGit).get(ui),source);
  assert.throws(()=>originActions('.',()=>{throw Error('no history');}),/fetch full git history/);
  assert.throws(()=>originActions('.',(_git,args)=>args[0]==='ls-tree'?Buffer.from(ui+'\0'):Buffer.from('missing\n')),/Invalid original blob/);
  assert.throws(()=>originActions('.',(_git,args)=>args[0]==='ls-tree'?Buffer.from(ui+'\0'):Buffer.from('abc blob 99\nx')),/Truncated original blob/);
});
test('new legacy and modern UI handlers fail until contract and behavioral test map them', () => {
  const files = changed();
  assert.match(run(files).join('\n'), /Unmapped UI\/native action/);
  assert.deepEqual(run(files, [contract(files)]), []);
  files.set(ui, files.get(ui) + '<input onchange={() => update({ value: "}" })} />');
  assert.match(run(files, [contract(changed())]).join('\n'), /onchange/);
  files.set(ui, initial.get(ui) + '<Editor onpublish={publish} />');
  assert.match(run(files).join('\n'), /onpublish/);
  files.set(ui, initial.get(ui) + '<form on:submit|preventDefault={publish} />');
  assert.match(run(files).join('\n'), /preventDefault/);
});
test('native commands, gateway variants and exported TS async wrappers are discovered', () => {
  const files = new Map(initial);
  files.set('src-tauri/src/commands.rs', '#[tauri::command] pub async fn reset() {}');
  files.set('src-tauri/src/gateways/new.rs', 'match command { "factory_reset" | "reset" => reset(), _ => () }');
  files.set('src/lib/actions.ts', 'export async function clear() {}\nexport const rename = async () => {}\nrequest("destroy");');
  const errors = run(files).join('\n');
  for (const name of ['factory_reset','reset','clear','rename','destroy']) assert.ok(errors.includes(name));
});
test('literal gateway command properties expose new behavior inside an existing handler body', () => {
  const files = new Map(initial).set(ui, initial.get(ui)+'<script>function save() { gatewayRequest({ command: "new_task" }); }</script>');
  assert.match(run(files).join('\n'), /new_task/);
  files.set('src/lib/actions.ts', 'const save = () => gatewayRequest({ command: \'new_native_task\' });');
  assert.match(run(files).join('\n'), /new_native_task/);
});
test('retired baseline actions are allowed; stale contract actions, workflows, tests and tools fail', () => {
  assert.deepEqual(run(new Map(initial).set(ui, '')), []);
  const files = changed(), valid = contract(files);
  assert.match(run(initial, [valid]).join('\n'), /stale action/);
  assert.match(run(files, [{...valid, workflow:'removed'}]).join('\n'), /Stale contract workflow/);
  assert.match(run(files, [{...valid, tools:['missing']}]).join('\n'), /absent MCP tool/);
  assert.match(run(files, [{...valid, tests:[{path:'tests/ui/save.mjs',anchor:'removed test'}]}]).join('\n'), /stale behavioral test/);
  const removed = new Map(initial);
  removed.delete('src-tauri/src/mcp_tools/catalogue.rs');
  assert.match(run(removed).join('\n'), /Stale MCP catalogue/);
});
test('empty metadata and assertion-free scenarios cannot qualify new action', () => {
  const files = changed();
  files.set('tests/ui/save.mjs', 'test("save persists", async () => callTool("save"));');
  assert.match(run(files, [contract(files)]).join('\n'), /no behavioral assertion/);
  assert.match(run(initial, [], base, [{...row, lifecycle:''}]).join('\n'), /missing lifecycle/);
  assert.match(run(initial, [], base, [{...row, status:'invented'}]).join('\n'), /invalid status/);
});
test('an explicit parity gap requires a complete proposed contract and test', () => {
  const files = changed();
  const proposed = {name:'reset',input:'empty object',output:'operation result',effect:'reset saved value',guard:'write authorization',lifecycle:'idempotent operation identity'};
  const planned = {...contract(files), tools:[], proposedTools:[proposed]};
  assert.deepEqual(run(files,[planned],base,[{...row,status:'missing'}]),[]);
  assert.match(run(files,[{...planned,proposedTools:[{name:'reset'}]}],base,[{...row,status:'missing'}]).join('\n'), /missing input/);
});
test('CLI reads bounded source roots and every inventory shard; fails on an added action', () => {
  const root = mkdtempSync(join(tmpdir(), 'mcp-parity-'));
  const write = (path, text) => {
    mkdirSync(join(root, path, '..'), {recursive:true});
    writeFileSync(join(root,path), text);
  };
  const json = (path, value) => write(path, JSON.stringify(value));
  const log = console.log, error = console.error;
  const messages = [];
  console.log = console.error = message => messages.push(message);
  try {
    for (const [path, source] of initial) write(path, source);
    write('.research/broken.ts', 'onclick={unexpected}');
    write('unrelated/ignored.ts', 'export async function unexpected() {}');
    json('docs/mcp-parity/inventory-first.json', [row]);
    json('docs/mcp-parity/inventory-third.json', [{...row,workflow:'other'}]);
    json('docs/mcp-parity/catalogue.json', indexed);
    json('docs/mcp-parity/contracts.json', []);
    json('docs/mcp-parity/workflows.json', ['save','other']);
    const cli = () => main([root], {originActions:base});
    assert.equal(cli(), 0);
    assert.ok(messages.some(message => message.includes('2 workflows')));
    write(ui, changed().get(ui));
    assert.equal(cli(), 1);
    assert.ok(messages.some(message => message.includes('Unmapped UI/native action')));
    json('docs/mcp-parity/contracts.json', [contract(changed())]);
    assert.equal(cli(), 0);
    json('docs/mcp-parity/inventory-first.json', []);
    assert.throws(cli, /Empty parity inventory/);
  } finally {
    console.log = log;
    console.error = error;
    rmSync(root, {recursive:true,force:true});
  }
});
