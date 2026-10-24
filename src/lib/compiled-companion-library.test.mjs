import test from 'node:test';
import assert from 'node:assert/strict';
import { CompiledWorkspaceStore, emptyCompiledWorkspace, newCompiledDraft, blankWidgetSource, mergeWidgetSources, replaceWorkspaceSources, validateCompiledDraft, compilePanelDraft, publicationMatchesDraft, designerId, compiledIndexedDbStorage } from './compiled-companion-library.ts';
import { createToggleSource } from '../../packages/widget-compiler/src/seeds.ts';

function memory(initial) { let value = initial, fail = false; return { read: async () => structuredClone(value), write: async next => { if (fail) throw new Error('Disk full'); value = structuredClone(next); }, get value() { return value; }, set fail(v) { fail = v; } }; }
function draftFor(source = createToggleSource()) { const draft = newCompiledDraft('Test', 640, 400); draft.widgets.push({ id: 'switch', pageId: draft.pages[0].id, label: 'Toggle', sourceId: source.id, parameters: {}, bounds: { x: .1, y: .1, width: .2, height: .16 } }); return draft; }
test('empty storage seeds one plain Toggle and reopens the same workspace', async () => {
  const storage = memory();
  const store = new CompiledWorkspaceStore(storage), loaded = await store.load();
  assert.equal('schemaVersion' in loaded, false); assert.deepEqual(loaded.sources.map(s => s.title), ['Toggle']); assert.deepEqual(loaded.drafts, []);
  assert.deepEqual(await store.load(), loaded);
  assert.equal((await new CompiledWorkspaceStore(memory()).load()).sources.length, 1);
  assert.match(designerId('instance'), /^instance-[a-f0-9-]+$/);

});
test('source bank replaces by stable ID without keeping revisions', () => {
  const toggle = createToggleSource(), next = { ...structuredClone(toggle), title: 'Power' }, blank = blankWidgetSource();
  assert.equal(blank.scene[0].kind, 'rectangle');assert.equal(blank.scene[0].fill,'none');
  let bank = mergeWidgetSources([toggle], [next, blank, next]); assert.equal(bank.length, 2);
  assert.deepEqual(bank.map(s => s.title), ['Power', 'New widget']);
  assert.equal(mergeWidgetSources(bank, [{ ...next, title: 'Changed' }])[0].title, 'Changed');
  assert.equal(toggle.title, 'Toggle');
  for (const field of ['revision', 'formatVersion']) assert.throws(() => mergeWidgetSources(bank, [{ ...next, [field]: 1 }]));
  assert.throws(() => mergeWidgetSources([], [{ ...blank, scene: [{ kind: 'html' }] }]), /primitive/);
  assert.throws(() => mergeWidgetSources([], Array.from({ length: 257 }, (_, i) => ({ ...toggle, id: `source-${i}` }))), /budget/);
});
test('draft validation rejects invalid references, dimensions, parameters and placements', () => {
  const source = createToggleSource(), draft = draftFor(source);
  assert.deepEqual(validateCompiledDraft(draft, [source]), draft);
  for (const mutate of [
    d => d.id = 'bad id', d => d.title = '', d => d.extra = true, d => d.pages = [], d => d.pages[0].width = 319,
    d => d.widgets[0].id = d.pages[0].id, d => d.widgets[0].pageId = 'missing', d => d.widgets[0].sourceId = 'missing',
    d => d.widgets[0].revision = 999, d => d.widgets[0].parameters = { missing: 1 }, d => d.widgets[0].bounds.width = 0,
    d => d.pages[0].background='red', d => d.pages[0].background=123, d => d.widgets[0].bounds.x = 2, d => d.widgets[0].bounds.height = Infinity,
  ]) { const d = structuredClone(draft); mutate(d); assert.throws(() => validateCompiledDraft(d, [source]), mutate.toString()); }
});
test('save captures caller data, validates publication/source together, and leaves last committed state after failure', async () => {
  const storage = memory(), store = new CompiledWorkspaceStore(storage), workspace = await store.load();
  workspace.drafts = [newCompiledDraft('First')];
  const pending = store.save(workspace); workspace.drafts[0].title = 'Mutated after capture';
  assert.equal((await pending).drafts[0].title, 'First');
  const committed = structuredClone(storage.value); storage.fail = true;
  await assert.rejects(store.save({ ...committed, drafts: [] }), /Disk full/); assert.deepEqual(storage.value, committed);
  storage.fail = false; await store.save(committed); assert.equal((await store.load()).drafts[0].title, 'First');
  for (const mutate of [
    w => w.schemaVersion = 1, w => w.sources = {}, w => w.drafts = Array.from({ length: 21 }, () => newCompiledDraft()),
    w => w.drafts.push(w.drafts[0]), w => w.publications = [], w => w.publications.bad = [null],
    w => w.publications['bad id'] = [], w => w.publications.tooMany = Array(101).fill(null), w => w.extra = true,
  ]) { const w = structuredClone(committed); mutate(w); await assert.rejects(store.save(w)); assert.deepEqual(storage.value, committed); }
  await assert.rejects(new CompiledWorkspaceStore(memory(null)).load());
});
test('panel compilation bakes exact placement size and supports static silent target, with cancellation before commit', async () => {
  const source = createToggleSource(), draft = draftFor(source), progress = [];
  draft.pages[0].background='#194c62';
  const publication = await compilePanelDraft(draft, [source], { motion: false, audio: false, onProgress: (...value) => progress.push(value) });
  assert.equal(publication.document.presentation.viewports[0].background,'#194c62');assert.deepEqual(progress, [[1, 1]]); assert.equal(publication.document.presentation.artifacts[0].artifact.width, 128);
  assert.equal(publication.document.presentation.artifacts[0].artifact.motions.length, 0);
  const storage = memory(), store = new CompiledWorkspaceStore(storage), workspace = await store.load();
  await store.save({ ...workspace, drafts: [draft], publications: { [draft.id]: [publication] } }); assert.equal((await store.load()).publications[draft.id][0].publicationHash, publication.publicationHash);
  assert.equal(await publicationMatchesDraft(draft, [source], publication), true);
  for (const mutate of [d => d.title = 'Edited', d => d.pages[0].width = 800, d => d.pages[0].background='#ffffff', d => d.widgets[0].label = 'Edited', d => d.widgets[0].bounds.x = .2, d => d.widgets[0].id = 'different', d => d.widgets[0].parameters.label = 'Edited', d => d.widgets.pop()]) { const d = structuredClone(draft); mutate(d); assert.equal(await publicationMatchesDraft(d, [source], publication), false); }
  assert.equal(await publicationMatchesDraft(draft, [{ ...source, title: 'Changed source' }], publication), false);
  const saved = { ...workspace, drafts: [draft], publications: { [draft.id]: [publication] } };
  assert.equal(replaceWorkspaceSources(saved, [source]).publications[draft.id].length, 1);
  const changed = replaceWorkspaceSources(saved, [{ ...source, title: 'Changed source' }]);
  assert.equal(changed.publications[draft.id], undefined);
  assert.equal(saved.publications[draft.id].length, 1);
  await assert.rejects(store.save({ ...workspace, drafts: [{ ...draft, title: 'Stale' }], publications: { [draft.id]: [publication] } }), /earlier panel/);
  await assert.rejects(store.save({ ...workspace, publications: { wrong: [publication] } }), /different panel/);
  await assert.rejects(compilePanelDraft(newCompiledDraft(), [source]), /every page/);
  const controller = new AbortController(); controller.abort(); await assert.rejects(compilePanelDraft(draft, [source], { signal: controller.signal }), /cancelled/);
  const during = new AbortController(); await assert.rejects(compilePanelDraft(draft, [source], { onProgress: () => during.abort(), signal: during.signal }), /cancelled/);
});
test('storage reports unavailable IndexedDB instead of claiming a save succeeded', async () => {
  const storage = compiledIndexedDbStorage(null); await assert.rejects(storage.read(), /unavailable/); await assert.rejects(storage.write(emptyCompiledWorkspace()), /unavailable/);
});

test('desktop defaults use native SQLite exclusively and return merged native sources',async()=>{
  const previous=globalThis.__TAURI_INTERNALS__;
  let value=null,revision=0,blocked=false,reads=0;
  try {
    globalThis.__TAURI_INTERNALS__={invoke:async(command,{request})=>{
      assert.equal(command,'persistence_workspace');
      if(blocked)throw Error('SQLite busy');
      if(request.operation==='read'){reads++;return {revision,libraryRevision:revision,value:structuredClone(value)};}
      assert.equal(request.expectedRevision,revision);
      value=structuredClone(request.value);
      const source=blankWidgetSource();source.id='external-mcp-source';
      value.sources=mergeWidgetSources(value.sources,[source]);
      return {revision:++revision,libraryRevision:revision,value:structuredClone(value)};
    }};
    const store=new CompiledWorkspaceStore();const loaded=await store.load();
    assert.equal(loaded.sources.some(s=>s.id==='external-mcp-source'),true);
    const saved=await store.save(loaded);assert.deepEqual(saved,value);
    assert.equal(reads,1);blocked=true;
    await assert.rejects(store.load(),/SQLite busy/);
    await assert.rejects(store.save(saved),/SQLite busy/);
  } finally {if(previous)globalThis.__TAURI_INTERNALS__=previous;else delete globalThis.__TAURI_INTERNALS__;}
});

test('new widgets and the capsule compile on a transparent surface, including state transitions',async()=>{
 const {compileWidget,decodeCompiledFrame}=await import('../../packages/widget-compiler/src/index.ts');
 const blank=await compileWidget(blankWidgetSource(),{width:80,height:40});
 assert.ok(decodeCompiledFrame(blank.artifact,blank.assets,blank.artifact.frames[0].id).every(value=>value===0));
 const toggle=await compileWidget(createToggleSource(),{width:120,height:72});
 for(const frame of toggle.artifact.frames){const rgba=decodeCompiledFrame(toggle.artifact,toggle.assets,frame.id);assert.equal(rgba[3],0);assert.equal(rgba.at(-1),0);assert.ok(rgba.some((v,i)=>i%4===3&&v>0),'Artwork remains visible');}

});
