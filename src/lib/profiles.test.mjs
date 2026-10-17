import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import {
  DEFAULT_PROFILE_ID, MAX_PROFILE_BYTES, ProfilesClient, captureProfileSession,
  exportProfileDocument, parseProfileDocument, profileDocumentError, profileDraft, profileDraftDirty, profileContractError, profileScalarValid, profileScalarAccepts, profileAssignmentsError, profilePortIdValid, profileDocumentAssignmentsError,
} from './profiles.ts';

const clone = value => JSON.parse(JSON.stringify(value));
const blank = () => ({ schemaVersion: 2, id: DEFAULT_PROFILE_ID, name: 'Default', revision: 1,
  eligibility: { gameIds: [] }, resources: [], bindings: [], processingNodes: [], virtualOutputs: [],
  visibility: [], overrides: [], companionAssignments: [], editor: { positions: [], viewport: { x: 0, y: 0, zoom: 1 } } });
const nativeSession = () => ({ selectedProfileId: DEFAULT_PROFILE_ID, selectionMode: 'manual', state: 'stopped', appliedSnapshot: null, fault: null });
const port = (id = 'button') => ({ id, name: id, direction: 'output', contract: {delivery:'state',schema:{type:'boolean'},freshnessMs:1500,fallback:false} });
const resource = () => ({ id: 'wheel', name: 'Wheel', kind: 'device', required: true, ports: [port()] });
const withResource = () => ({ ...blank(), resources: [resource()] });
const invalid = document => assert.ok(profileDocumentError(document));

// The same versioned documents are loaded by the Rust contract tests.
for (const name of ['default', 'blank', 'mixed']) {
  test(`shared native ${name} profile fixture roundtrips without legacy adapters`, async () => {
    const source = await readFile(new URL(`../../tests/fixtures/profiles/${name}.json`, import.meta.url), 'utf8');
    const document = parseProfileDocument(source);
    assert.equal(profileDocumentError(document), '');
    assert.deepEqual(parseProfileDocument(exportProfileDocument(document)), document);
  });
}

test('imports are bounded UTF-8 JSON and reject legacy/local assignments at every level', () => {
  for (const source of ['{', 'null', '[]', ' '.repeat(MAX_PROFILE_BYTES + 1), 'é'.repeat(MAX_PROFILE_BYTES / 2 + 1)]) assert.throws(() => parseProfileDocument(source));
  for (const extra of [{ assignments: [] }, { connection: {} }, { profile: blank() }, { document: blank() }, { machineAssignments: [] }]) invalid({ ...blank(), ...extra });
  for (const document of [{ ...blank(), schemaVersion: 1 }, { ...blank(), revision: 0 }, { ...blank(), revision: Number.MAX_SAFE_INTEGER + 1 }, { ...blank(), id: 'legacy' }, { ...blank(), id: 'ABCDEF00-0000-4000-8000-000000000001' }, { ...blank(), name: '\n' }, { ...blank(), name: 'x'.repeat(81) }, { ...blank(), eligibility: { gameIds: ['dcs', 'dcs'] } }, { ...blank(), eligibility: { gameIds: ['bad game'] } }]) invalid(document);
  invalid({ ...blank(), eligibility: { gameIds: [], executable: 'C:/private/game.exe' } });
  invalid({ ...withResource(), resources: [{ ...resource(), sourceId: 'private-device' }] });
  invalid({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), path: 'private' }] }] });
  assert.ok(profileDocumentError(undefined));
  assert.ok(profileDocumentError({ value: 1n }));
  assert.ok(profileDocumentError({ ...blank(), name: 'é'.repeat(MAX_PROFILE_BYTES) }));
  assert.throws(() => exportProfileDocument({ ...blank(), sourceId: 'private' }));
});

test('full scalar contracts reject unknown fields and preserve byte, unit, enum and fallback constraints', () => {
  const valid = [
    {type:'boolean'}, {type:'rgb'},
    {type:'integer',min:-1,max:8,unit:null}, {type:'number',min:0,max:2,unit:'ratio'},
    {type:'text',maxBytes:4,encoding:'utf8'}, {type:'enum',values:['up','down']},
  ];
  const values = [true,'a0FF00',0,1.5,'éé','up'];
  for (let i=0;i<valid.length;i++) {
    assert.equal(profileScalarValid(valid[i]),true);
    assert.equal(profileScalarAccepts(valid[i],values[i]),true);
    const p=port();p.contract.schema=valid[i];p.contract.fallback=values[i];
    assert.equal(profileDocumentError({...withResource(),resources:[{...resource(),ports:[p]}]}),'');
    assert.equal(profileScalarValid({...valid[i],privateField:true}),false);
  }
  for (const schema of [null,{type:'unknown'},{type:'text',maxBytes:0,encoding:'utf8'},{type:'text',maxBytes:4097,encoding:'ascii'},{type:'text',maxBytes:2,encoding:'ansi'},{type:'integer',min:0.5,max:2,unit:null},{type:'number',min:2,max:1,unit:null},{type:'number',min:0,max:Infinity,unit:null},{type:'number',min:0,max:Number.MAX_SAFE_INTEGER+1,unit:null},{type:'number',min:0,max:1,unit:''},{type:'enum',values:[]},{type:'enum',values:['a','a']},{type:'enum',values:['']},{type:'enum',values:Array(65).fill('a')}]) assert.equal(profileScalarValid(schema),false);
  for (let i=0;i<valid.length;i++) assert.equal(profileScalarAccepts(valid[i],{}),false);
  assert.equal(profileScalarAccepts({type:'text',maxBytes:3,encoding:'utf8'},'éé'),false);
  assert.equal(profileScalarAccepts({type:'text',maxBytes:4,encoding:'ascii'},'é'),false);
  assert.equal(profileScalarAccepts({type:'text',maxBytes:4,encoding:'ascii'},'a\n'),false);
  assert.equal(profileScalarAccepts({type:'text',maxBytes:4,encoding:'ascii'},''),true);
  for (const change of [{id:''},{id:'bad\nid'},{id:'é'.repeat(41)},{name:''},{direction:'both'},{contract:null},{contract:{...port().contract,freshnessMs:0}},{contract:{...port().contract,freshnessMs:600001}},{contract:{...port().contract,fallback:1}},{delivery:'state'}]) invalid({...withResource(),resources:[{...resource(),ports:[{...port(),...change}]}]});
  const unknown=port();unknown.contract.fallback=null;assert.equal(profileDocumentError({...withResource(),resources:[{...resource(),ports:[unknown]}]}),'');
  invalid({...withResource(),resources:[{...resource(),ports:[port(),port()]}]});
});

test('ordered invocations and provider actions retain explicit semantics, display and ownership constraints', () => {
  const parameter={id:'value',name:'Value',schema:{type:'text',maxBytes:16,encoding:'ascii'}};
  const invocation={delivery:'invocation',parameters:[parameter],semantics:'setter'};
  const action={delivery:'action',parameters:[parameter],semantics:'setter',endpoint:'display',readable:true,display:{columns:8,rows:2,encoding:'ascii'},ownership:{scope:'endpoint',maxLeaseMs:5000}};
  assert.equal(profileContractError(invocation,'output'),'');
  assert.ok(profileContractError(invocation,'input'));assert.ok(profileContractError(invocation,'output',true));
  assert.equal(profileContractError(action,'input',true),'');
  assert.ok(profileContractError(action,'output'));
  assert.equal(profileContractError({delivery:'event',schema:{type:'boolean'}},'output'),'');
  assert.ok(profileContractError({delivery:'event',schema:{type:'boolean'},fallback:false},'output'));
  for(const change of [{semantics:'implicit'},{parameters:[parameter,parameter]},{parameters:Array(5).fill(parameter)},{parameters:[{...parameter,private:true}]},{parameters:[{...parameter,id:''}]},{endpoint:''},{readable:null},{display:undefined},{ownership:undefined},{display:{columns:0,rows:1,encoding:'ascii'}},{display:{columns:63,rows:4,encoding:'ascii'}},{display:{columns:8,rows:2,encoding:'utf8'}},{readable:false},{semantics:'one-shot'},{parameters:[]},{ownership:{scope:'global',maxLeaseMs:1}},{ownership:{scope:'endpoint',maxLeaseMs:60001}},{ownership:{scope:'endpoint',maxLeaseMs:0}}]) assert.ok(profileContractError({...action,...change},'input'),JSON.stringify(change));
  assert.equal(profileContractError({...action,display:null,ownership:null},'input'),'');
});

test('every explicit operation has a strict portable shape and no hidden processing override', () => {
  const document=withResource();
  const node={id:'processor',name:'Processor',operation:{kind:'identity'},inputs:[{...port('input'),direction:'input'}],outputs:[port('output')]};
  const operations=[
    ...['identity','not','invert','gate','any','all','event-to-state'].map(kind=>({kind})),
    {kind:'constant',value:1},{kind:'scale',factor:2,offset:1},
    {kind:'range-map',inputMin:0,inputMax:1,outputMin:0,outputMax:32767,rounding:'nearest'},
    {kind:'unit-convert',from:'ft',to:'m',rounding:'none'},
    {kind:'deadzone',center:0,radius:1,rounding:'nearest'},{kind:'curve',points:[{input:0,output:0},{input:1,output:1}],rounding:'none'},
    {kind:'compare',comparison:'greater',threshold:1},{kind:'hysteresis',comparison:'greater-equal',press:10,release:9},
    {kind:'priority',order:['input']},{kind:'event-edge',edge:'rising'},{kind:'select',whenTrue:1,whenFalse:0},
    {kind:'format',decimals:2,prefix:'ALT ',suffix:' m',rounding:'nearest',overflow:'reject'},
    {kind:'fallback',unknown:0,stale:1,unavailable:2},{kind:'action-pack',arguments:[]},{kind:'set-on-change',arguments:['input']},
    {kind:'script',source:'x',parameters:{name:'kept semantic',gain:2}},
  ];
  const rules=[
    ...['merge','chord','gate','toggle'].map(kind=>({kind})),
    {kind:'axis',deadZone:0,invert:false,scale:1000,curve:[]},{kind:'split',positive:true},
    {kind:'button-axis',range:'unipolar',pressed:32767,released:0},{kind:'threshold',press:10,release:9},
    {kind:'gesture',gesture:'hold',holdUs:1000,doubleUs:1000,pulseUs:1000},
    {kind:'pulse',durationUs:1000,repeatUs:0},{kind:'macro',steps:[{afterUs:1000,pressed:false}]},
  ];
  operations.push(...rules.map(rule=>({kind:'builtin',rule,inputs:['input']})));
  for(const operation of operations) {
    assert.equal(profileDocumentError({...document,processingNodes:[{...node,operation}]}),'',JSON.stringify(operation));
    invalid({...document,processingNodes:[{...node,operation:{...operation,backend:'private'}}]});
  }
  for(const operation of [null,{kind:'trigger'},{kind:'deadzone',center:0,radius:1},{kind:'curve',points:[{input:0,output:0},{input:1,output:1}]},{kind:'scale',factor:Infinity,offset:0},{kind:'hysteresis',comparison:'bad',press:0,release:1},{kind:'priority',order:['x','x']},{kind:'script',source:'',parameters:{}},{kind:'script',source:'x'.repeat(16385),parameters:{}},{kind:'script',source:'x',parameters:{secret:{raw:true}}},{kind:'format',decimals:7,prefix:'',suffix:'',rounding:'none',overflow:'reject'}]) invalid({...document,processingNodes:[{...node,operation}]});
  invalid({...document,overrides:[{resourceId:'wheel',enabled:true,debounceMs:10,invertPorts:[]}]});
  assert.equal(profileDocumentError({...document,overrides:[{resourceId:'wheel',enabled:false}]}),'');
  invalid({...document,overrides:[{resourceId:'wheel',enabled:null}]});
});

test('graph structure, explicit layouts and editor metadata stay bounded', () => {
  const document=withResource(), node={id:'gain',name:'Gain',operation:{kind:'identity'},inputs:[port('in')],outputs:[port('out')]};
  invalid({...document,processingNodes:[{...node,id:'wheel'}]});
  invalid({...document,resources:Array(65).fill(resource())});
  invalid({...document,processingNodes:Array(257).fill(node)});
  const virtual={id:'controller',name:'Controller',required:true,ports:[{...port(),direction:'input'}],layout:{buttons:['button'],axes:[],hats:[]}};
  assert.equal(profileDocumentError({...document,virtualOutputs:[virtual]}),'');
  for(const layout of [{buttons:[],axes:[],hats:[]},{buttons:['button','button'],axes:[],hats:[]},{buttons:[],axes:[{portId:'axis',range:'bipolar',fallback:-32769}],hats:[]},{buttons:[],axes:[{portId:'axis',range:'unipolar',fallback:1}],hats:['hat','hat']}]) invalid({...document,virtualOutputs:[{...virtual,layout}]});
  assert.equal(profileDocumentError({...document,virtualOutputs:[{...virtual,layout:{buttons:['button'],axes:[{portId:'axis',range:'bipolar',fallback:-1}],hats:['hat']}}]}),'');
  invalid({...document,virtualOutputs:Array(17).fill(virtual)});
  invalid({...document,virtualOutputs:[{...virtual,slot:1}]});
  const manyPorts=Array.from({length:128},(_,i)=>port('p'+i));
  invalid({...blank(),resources:Array.from({length:9},(_,i)=>({...resource(),id:'r'+i,ports:manyPorts}))});
  const binding={id:'route',from:{nodeId:'wheel',portId:'button'},to:{nodeId:'gain',portId:'in'}};
  assert.equal(profileDocumentError({...document,processingNodes:[node],bindings:[binding],visibility:[{resourceId:'wheel',mode:'hidden-while-active'}],overrides:[{resourceId:'wheel',enabled:false}],companionAssignments:[{resourceId:'tablet',publicationHash:'sha256:'+'a'.repeat(64)}],editor:{positions:[{nodeId:'wheel',x:-5,y:20}],viewport:{x:0,y:0,zoom:2}}}),'');
  for(const change of [{bindings:[binding,binding]},{bindings:[{...binding,from:{...binding.from,sourceId:'private'}}]},{visibility:[{resourceId:'wheel',mode:'hide-all'}]},{companionAssignments:[{resourceId:'tablet',publicationHash:'unhashed'}]},{editor:{positions:[],viewport:{x:0,y:0,zoom:0}}},{editor:{positions:[{nodeId:'wheel',x:1000001,y:0}],viewport:{x:0,y:0,zoom:1}}}]) invalid({...document,...change});
});

test('draft and captured session snapshots never alias their source or each other', () => {
  const document = withResource(), editor = profileDraft(document);
  editor.document.resources[0].name = 'Draft wheel';
  assert.equal(profileDraftDirty(editor), true);
  assert.equal(editor.baseDocument.resources[0].name, 'Wheel');
  assert.equal(document.resources[0].name, 'Wheel');
  assert.equal(profileDraftDirty(profileDraft(document)), false);
  const session = nativeSession();
  session.appliedSnapshot = { profileId: document.id, revision: document.revision, semanticHash: 'sha256:' + 'a'.repeat(64) };
  const captured = captureProfileSession(session);
  session.appliedSnapshot.revision = 99;
  assert.equal(captured.appliedSnapshot.revision, 1);
  assert.ok(Object.isFrozen(captured)); assert.ok(Object.isFrozen(captured.appliedSnapshot));
  assert.throws(() => { captured.appliedSnapshot.revision = 3; }, TypeError);
});

test('client storage CRUD and editor opening never select or apply a runtime', async () => {
  let saved = blank(); const session = nativeSession(), calls = [];
  const client = new ProfilesClient(async request => {
    calls.push(clone(request));
    if (request.command === 'profile_save') saved = { ...clone(request.document), revision: saved.revision + 1 };
    if (request.command === 'profile_create' || request.command === 'profile_duplicate' || request.command === 'profile_import') saved = { ...clone(request.document ?? saved), id: '00000000-0000-4000-8000-000000000002', name: request.name ?? saved.name, revision: 1 };
    if (request.command === 'profile_rename') saved = { ...saved, name: request.name, revision: saved.revision + 1 };
    return { document: saved, session, profiles: [{ id: saved.id, name: saved.name, revision: saved.revision, isDefault: true, semanticHash: 'a'.repeat(64) }], diagnostics: [], readOnly: false, deletedId: request.id };
  });
  assert.equal(client.editor, null); assert.equal(client.session, null);
  await client.status(); await client.list();
  const before = clone(client.session);
  const editor = await client.open(saved.id);
  editor.document.name = 'Local edit';
  assert.equal(client.editor.document.name, 'Default');
  client.updateDraft(editor.document);
  assert.equal(client.editor.document.name, 'Local edit');
  assert.deepEqual(client.session, before);
  await client.saveDraft();
  assert.equal(profileDraftDirty(client.editor), false);
  assert.equal(client.editor.baseRevision, 2);
  assert.deepEqual(client.session, before);
  assert.equal(calls.filter(call => call.command === 'profile_save').length, 1);
  assert.equal(calls.at(-1).expectedRevision, 1);
  const exported = await client.export(saved.id);
  const imported = await client.import(exported, 'Imported');
  assert.equal(imported.name, 'Imported');
  await client.import(exported);
  await client.create('New'); await client.rename(saved.id, 'Renamed', saved.revision);
  const duplicated = await client.duplicate(saved.id, 'Copy');
  duplicated.editor.viewport.zoom = 3;
  assert.equal(saved.editor.viewport.zoom, 1);
  await client.duplicate(saved.id); await client.delete(saved.id, saved.revision);
  assert.ok(calls.every(call => !['profile_select', 'profile_apply', 'mapping_virtual_start', 'controller_visibility_apply'].includes(call.command)));
  assert.deepEqual(client.session, before);
  client.close(); assert.equal(client.editor, null);
});

test('selection, mode, pause and explicit recovery consume native state without activating', async () => {
  const calls = [], session = nativeSession();
  const client = new ProfilesClient(async request => {
    calls.push(request);
    if (request.command === 'profile_select') session.selectedProfileId = request.id;
    if (request.command === 'profile_selection_mode') session.selectionMode = request.mode;
    if (request.command === 'profile_pause') session.state = request.paused ? 'paused' : 'stopped';
    return { session };
  });
  await client.select('other');
  assert.equal(client.session.selectedProfileId, 'other');
  assert.equal(calls.at(-1).selectionSource, 'manual');
  await client.selectionMode('automatic'); await client.select(DEFAULT_PROFILE_ID, 'automatic');
  await client.pause(); assert.equal(client.session.state, 'paused');
  await client.pause(false); assert.equal(client.session.state, 'stopped');
  session.state = 'faulted'; session.fault = 'Test fault';
  await client.status(); assert.equal(client.session.fault, 'Test fault');
  await client.reset(); assert.deepEqual(calls.at(-1), { command: 'profile_reset', confirm: true });
  assert.ok(calls.every(call => call.command !== 'profile_apply'));
});

test('client errors and invalid imports leave drafts, session and transport untouched', async () => {
  let fail = false, count = 0;
  const client = new ProfilesClient(async () => { count++; if (fail) throw Error('revision conflict'); return { document: blank(), session: nativeSession() }; });
  await assert.rejects(client.saveDraft(), /Open a profile/);
  assert.throws(() => client.updateDraft(blank()), /Open this profile/);
  await client.open(DEFAULT_PROFILE_ID);
  assert.throws(() => client.updateDraft({ ...blank(), id: 'different' }));
  assert.throws(() => client.updateDraft({ ...blank(), revision: 2 }));
  const before = clone(client.editor), session = clone(client.session), previous = count;
  await assert.rejects(client.import(JSON.stringify({ ...blank(), assignments: [{ sourceId: 'private' }] })));
  await assert.rejects(client.save({ ...blank(), name: '' }));
  assert.equal(count, previous);
  fail = true;
  await assert.rejects(client.saveDraft(), /revision conflict/);
  await assert.rejects(client.import(JSON.stringify(blank())), /revision conflict/);
  assert.deepEqual(client.editor, before); assert.deepEqual(client.session, session);
  const missing = new ProfilesClient(async () => ({}));
  await assert.rejects(missing.get(DEFAULT_PROFILE_ID), /missing its document/);
  assert.deepEqual(await missing.status(), {}); assert.equal(missing.session, null);
});

test('a save completing after another draft opens cannot replace the newly opened editor', async () => {
  let finish;
  const first = blank(), second = { ...blank(), id: '00000000-0000-4000-8000-000000000003', name: 'Other' };
  const client = new ProfilesClient(async request => request.command === 'profile_save'
    ? new Promise(resolve => { finish = () => resolve({ document: { ...first, revision: 2 } }); })
    : { document: request.id === second.id ? second : first });
  await client.open(first.id);
  const saving = client.saveDraft();
  await client.open(second.id);
  finish(); await saving;
  assert.equal(client.editor.document.id, second.id);
});

test('edits made during a save rebase onto its committed revision and can save again', async () => {
  let document = blank(), finish, delayed = true;
  const requests = [];
  const client = new ProfilesClient(async request => {
    requests.push(clone(request));
    if (request.command === 'profile_save') {
      if (request.expectedRevision !== document.revision) throw Error('revision conflict');
      if (delayed) await new Promise(resolve => { finish = resolve; });
      document = { ...clone(request.document), revision: document.revision + 1 };
    }
    return { document };
  });
  await client.open(document.id);
  client.updateDraft({ ...client.editor.document, name: 'First edit' });
  const saving = client.saveDraft();
  const newer = client.editor.document;
  newer.name = 'Later edit'; newer.editor.viewport.zoom = 2;
  client.updateDraft(newer);
  finish(); await saving;
  assert.equal(document.name, 'First edit');
  assert.equal(client.editor.document.name, 'Later edit');
  assert.equal(client.editor.document.editor.viewport.zoom, 2);
  assert.equal(client.editor.baseDocument.name, 'First edit');
  assert.equal(client.editor.baseDocument.editor.viewport.zoom, 1);
  assert.equal(client.editor.document.revision, 2);
  assert.equal(client.editor.baseRevision, 2);
  assert.equal(profileDraftDirty(client.editor), true);
  delayed = false;
  await client.saveDraft();
  assert.equal(requests.at(-1).expectedRevision, 2);
  assert.equal(document.name, 'Later edit');
  assert.equal(document.revision, 3);
  assert.equal(profileDraftDirty(client.editor), false);
});

test('a pending save does not rebase a freshly reopened editor for the same profile', async () => {
  const document = blank(); let finish;
  const client = new ProfilesClient(async request => request.command === 'profile_save'
    ? new Promise(resolve => { finish = () => resolve({ document: { ...document, revision: 2 } }); })
    : { document });
  await client.open(document.id);
  const saving = client.saveDraft();
  await client.open(document.id);
  finish(); await saving;
  assert.equal(client.editor.baseRevision, 1);
  assert.equal(client.editor.document.revision, 1);
});

test('late editor loads and closed editors cannot replace newer user choices', async () => {
  const pending = new Map();
  const client = new ProfilesClient(request => new Promise(resolve => { pending.set(request.id, resolve); }));
  const first = client.open('first'), second = client.open('second');
  pending.get('second')({ document: { ...blank(), name: 'Second' } }); await second;
  pending.get('first')({ document: { ...blank(), name: 'First' } }); await first;
  assert.equal(client.editor.document.name, 'Second');
  const closing = client.open('third'); client.close();
  pending.get('third')({ document: blank() }); await closing;
  assert.equal(client.editor, null);
});

test('successful removal clears only its own editor and failed removal preserves the draft', async () => {
  let fail = false;
  const client = new ProfilesClient(async request => {
    if (fail) throw Error('storage unavailable');
    return request.command === 'profile_get' ? { document: blank() } : { deletedId: request.id };
  });
  await client.open(DEFAULT_PROFILE_ID);
  await client.delete('other', 1); assert.ok(client.editor);
  fail = true;
  await assert.rejects(client.delete(DEFAULT_PROFILE_ID, 1), /storage unavailable/); assert.ok(client.editor);
  fail = false;
  await client.delete(DEFAULT_PROFILE_ID, 1); assert.equal(client.editor, null);
});

test('non-finite draft values cannot normalize into null and reach storage', async () => {
  let calls = 0;
  const client = new ProfilesClient(async () => { calls++; return { document: blank() }; });
  await assert.rejects(client.save({ ...withResource(), resources: [{ ...resource(), ports: [{ ...port(), contract:{...port().contract,freshnessMs:NaN} }] }] }));
  assert.equal(calls, 0);
});

test('invalid enable policy is rejected before cloning and preserves the draft', async () => {
  const document={...withResource(),overrides:[{resourceId:'wheel',enabled:true}]},requests=[];
  const client=new ProfilesClient(async request=>{requests.push(request);return {document};});
  await client.open(document.id);const before=client.editor;
  for(const value of [NaN,Infinity,null]) {
    const invalid=client.editor.document;invalid.overrides[0].enabled=value;
    assert.throws(()=>client.updateDraft(invalid),/runtime overrides/);
    assert.throws(()=>profileDraft(invalid),/runtime overrides/);assert.deepEqual(client.editor,before);
  }
  await client.saveDraft();assert.equal(requests.at(-1).document.overrides[0].enabled,true);
});

test('local assignment CRUD captures exact native IDs without exporting or activating', async () => {
  const assignments=[{resourceId:'wheel',sourceId:'native:opaque',ports:[{profilePortId:'button',observedPortId:'button:0',expectedContractHash:'sha256:'+'a'.repeat(64)}]}],calls=[];
  const client=new ProfilesClient(async request=>{calls.push(clone(request));return {document:withResource(),assignments:clone(assignments),session:nativeSession()};});
  assert.equal(profileAssignmentsError(assignments),'');
  const read=await client.getAssignments(DEFAULT_PROFILE_ID);read.assignments[0].sourceId='changed';assert.equal(assignments[0].sourceId,'native:opaque');
  await client.saveAssignments(DEFAULT_PROFILE_ID,1,assignments);
  assert.deepEqual(calls.map(c=>c.command),['profile_assignments_get','profile_assignments_save']);
  assert.equal(exportProfileDocument(withResource()).includes('native:opaque'),false);
  const malformed=[null,[{...assignments[0],ports:undefined}],[{...assignments[0],sourceId:''}],[{...assignments[0],ports:[{...assignments[0].ports[0],expectedContractHash:'stale'}]}],[assignments[0],assignments[0]],[{...assignments[0],ports:[assignments[0].ports[0],assignments[0].ports[0]]}]];
  for(const value of malformed) {assert.ok(profileAssignmentsError(value));await assert.rejects(client.saveAssignments(DEFAULT_PROFILE_ID,1,value));}
  await assert.rejects(client.saveAssignments(DEFAULT_PROFILE_ID,0,assignments));assert.equal(calls.length,2);
  const invalidClient=new ProfilesClient(async()=>({document:blank()}));await assert.rejects(invalidClient.getAssignments(DEFAULT_PROFILE_ID),/Malformed/);await assert.rejects(invalidClient.saveAssignments(DEFAULT_PROFILE_ID,1,assignments),/Malformed/);
});

test('native argument port identities remain exact while node IDs keep logical naming rules', () => {
  for (const id of ['0', 'gain:0', 'trigger', 'native argument', 'é'.repeat(40)]) {
    assert.equal(profilePortIdValid(id), true);
    const p={...port(),id};
    const document={...blank(),resources:[{...resource(),ports:[p]}],
      bindings:[{id:'edge',from:{nodeId:'wheel',portId:id},to:{nodeId:'pack',portId:id}}],
      processingNodes:[{id:'pack',name:'Pack',operation:{kind:'set-on-change',arguments:[id]},
        inputs:[{...p,direction:'input'}],outputs:[{id:'request',name:'Request',direction:'output',contract:{delivery:'invocation',parameters:[{id,name:'Argument',schema:{type:'boolean'}}],semantics:'setter'}}]}]};
    assert.equal(profileDocumentError(document),'');
    assert.deepEqual(parseProfileDocument(exportProfileDocument(document)),document);
    assert.equal(profileAssignmentsError([{resourceId:'wheel',sourceId:'native:resource',ports:[{profilePortId:id,observedPortId:'native:0',expectedContractHash:'sha256:'+'a'.repeat(64)}]}]),'');
    assert.equal(profileDocumentError({...blank(),virtualOutputs:[{id:'controller',name:'Controller',required:true,ports:[{...p,direction:'input'}],layout:{buttons:[id],axes:[],hats:[]}}]}),'');
  }
  for(const id of ['', 'bad\nport', 'bad\u0085port', 'é'.repeat(41)]) {
    assert.equal(profilePortIdValid(id),false);
    invalid({...withResource(),resources:[{...resource(),ports:[{...port(),id}]}]});
    assert.ok(profileAssignmentsError([{resourceId:'wheel',sourceId:'native:resource',ports:[{profilePortId:id,observedPortId:'native:0',expectedContractHash:'sha256:'+'a'.repeat(64)}]}]));
  }
  invalid({...withResource(),resources:[{...resource(),id:'0'}]});
  invalid({...withResource(),bindings:[{id:'route',from:{nodeId:'gain:0',portId:'button'},to:{nodeId:'wheel',portId:'button'}}]});
});

test('retained script static budgets match the actual evaluator envelope', () => {
  const node={id:'script',name:'Script',operation:{kind:'script',source:'x',parameters:{}},
    inputs:[{...port('input'),direction:'input'}],outputs:[port('output')]};
  const document={...blank(),processingNodes:[node]};
  assert.equal(profileDocumentError(document),'');
  for(const change of [{inputs:[]},{inputs:Array.from({length:33},(_,i)=>({...port('in'+i),direction:'input'}))},
    {outputs:[]},{outputs:Array.from({length:17},(_,i)=>port('out'+i))},
    {operation:{...node.operation,source:'x'.repeat(16385)}},
    {operation:{...node.operation,parameters:{text:'é'.repeat(2045)}}},
    {operation:{...node.operation,parameters:{numeric:Number.MAX_SAFE_INTEGER+1}}}]) invalid({...document,processingNodes:[{...node,...change}]});
  assert.equal(profileDocumentError({...document,processingNodes:[{...node,operation:{...node.operation,parameters:{text:'é'.repeat(2000)}}}]}),'');
});


const editorSummaries = document => [{id:DEFAULT_PROFILE_ID,name:'Default',revision:document.id===DEFAULT_PROFILE_ID?document.revision:1,isDefault:true,semanticHash:'sha256:'+'a'.repeat(64)},...(document.id===DEFAULT_PROFILE_ID?[]:[{id:document.id,name:document.name,revision:document.revision,isDefault:false,semanticHash:'sha256:'+'b'.repeat(64)}])];

test('editor save commits graph and local pins in one typed request without selecting or changing drafts', async () => {
  const pins=[{resourceId:'wheel',sourceId:'native:opaque',ports:[{profilePortId:'button',observedPortId:'button:0',expectedContractHash:'sha256:'+'a'.repeat(64)}]}];
  const created={...withResource(),id:'4bef570c-7bfa-4fac-85d4-e84b4239bd3d',name:'Flight'}, calls=[];
  const client=new ProfilesClient(async request=>{calls.push(clone(request));const saved={...clone(request.document),revision:request.expectedRevision===null?1:request.expectedRevision+1};return {document:saved,assignments:clone(request.assignments),profiles:editorSummaries(saved),session:nativeSession()};});
  const first=await client.editorSave(created,null,pins);assert.equal(first.document.revision,1);assert.deepEqual(first.assignments,pins);assert.equal(first.profiles.find(p=>p.id===created.id).semanticHash,"sha256:"+"b".repeat(64));assert.equal(client.editor,null);
  first.assignments[0].sourceId='mutated';assert.equal(pins[0].sourceId,'native:opaque');
  const second=await client.editorSave({...created,name:'Edited graph'},1,pins);assert.equal(second.document.revision,2);
  assert.deepEqual(calls.map(call=>call.command),['profile_editor_save','profile_editor_save']);assert.equal(calls[0].expectedRevision,null);assert.equal(calls[1].expectedRevision,1);assert.equal(client.session.selectedProfileId,DEFAULT_PROFILE_ID);assert.equal(client.session.state,'stopped');assert.equal(exportProfileDocument(second.document).includes('native:opaque'),false);
  const zero={...created,resources:[{...resource(),ports:[]}]};assert.equal(profileDocumentAssignmentsError(zero,[{resourceId:'wheel',sourceId:'native:seed',ports:[]}]),'');assert.equal(profileDocumentAssignmentsError(created,[]),'');
});

test('atomic editor save rejects incomplete pins, invalid revisions and protected builtin labels before transport', async () => {
  let calls=0;const client=new ProfilesClient(async()=>{calls++;return {}});const document=withResource();
  const pins=[{resourceId:'wheel',sourceId:'native:opaque',ports:[{profilePortId:'button',observedPortId:'button:0',expectedContractHash:'sha256:'+'a'.repeat(64)}]}];
  for(const value of [null,[{...pins[0],ports:[]}],[{...pins[0],resourceId:'missing'}],[{...pins[0],ports:[{...pins[0].ports[0],profilePortId:'missing'}]}]]) await assert.rejects(client.editorSave(document,1,value));
  for(const revision of [0,2,true,'1',NaN,Number.MAX_SAFE_INTEGER+1]) await assert.rejects(client.editorSave(document,revision,pins));
  await assert.rejects(client.editorSave(document,null,pins));await assert.rejects(client.editorSave({...document,revision:2,id:'4bef570c-7bfa-4fac-85d4-e84b4239bd3d'},null,pins));await assert.rejects(client.editorSave({...document,name:'Renamed builtin'},1,pins));await assert.rejects(client.editorSave({...document,schemaVersion:1},1,pins));assert.equal(calls,0);
});

test('atomic editor save validates complete native acknowledgement and preserves session after failures', async () => {
  const document={...withResource(),id:'4bef570c-7bfa-4fac-85d4-e84b4239bd3d'}, pins=[];
  let reply={document,assignments:pins,session:nativeSession()}, fail=false;
  const client=new ProfilesClient(async request=>{if(fail) throw Error('revision conflict');return request.command==='profile_status'?{session:nativeSession()}:reply});await client.status();const session=client.session;
  for(const bad of [{assignments:pins,session:nativeSession()},{document,session:nativeSession()},{document,assignments:pins},{document:{...document,id:DEFAULT_PROFILE_ID},assignments:pins,session:nativeSession()},{document:{...document,revision:2},assignments:pins,session:nativeSession()}]){reply=bad;await assert.rejects(client.editorSave(document,null,pins));assert.deepEqual(client.session,session);}
  fail=true;await assert.rejects(client.editorSave(document,null,pins),/revision conflict/);assert.deepEqual(client.session,session);assert.equal(client.editor,null);
});


test('atomic save requires bounded real matching native profile summaries', async () => {
  const document={...withResource(),id:'4bef570c-7bfa-4fac-85d4-e84b4239bd3d',name:'Flight'};
  const valid={document,assignments:[],profiles:editorSummaries(document),session:nativeSession()};
  let reply=valid;const client=new ProfilesClient(async()=>reply);
  assert.equal((await client.editorSave(document,null,[])).profiles.length,2);
  for(const profiles of [undefined,[],[valid.profiles[0]],Array(33).fill(valid.profiles[0]),[valid.profiles[0],{...valid.profiles[1],name:'Different'}],[valid.profiles[0],{...valid.profiles[1],revision:2}],[valid.profiles[0],{...valid.profiles[1],isDefault:true}],[valid.profiles[0],{...valid.profiles[1],semanticHash:''}],[valid.profiles[0],valid.profiles[1],valid.profiles[1]]]) {reply={...valid,profiles};await assert.rejects(client.editorSave(document,null,[]));}
});

test('canonical action contracts preserve an explicit restoration policy without changing legacy shape', () => {
  const contract={delivery:'action',parameters:[],semantics:'setter',endpoint:'lamp',readable:true,display:null,ownership:{scope:'endpoint',maxLeaseMs:5000}};
  assert.equal(profileContractError(contract,'input'),'');
  assert.equal(profileContractError({...contract,ownership:{...contract.ownership,restorePolicy:'if-unchanged'}},'input'),'');
  for(const restorePolicy of [null,undefined,'always',true]) assert.match(profileContractError({...contract,ownership:{...contract.ownership,restorePolicy}},'input'),/ownership/);
  assert.match(profileContractError({...contract,ownership:{...contract.ownership,restorePolicy:'if-unchanged',secret:'no'}},'input'),/ownership/);
});
