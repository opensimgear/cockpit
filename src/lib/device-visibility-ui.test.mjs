import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import*as server from'svelte/internal/server';import*as svelte from'svelte';
const file=new URL('./DeviceVisibility.svelte',import.meta.url),source=await readFile(file,'utf8'),physical={id:'exact-physical-source',online:true,baselineReady:true},plain=v=>JSON.parse(JSON.stringify(v));
const status=platform=>({platform,modes:[{mode:'visible',label:'Visible',supported:true,qualified:true,reason:'Local preference only.'},{mode:'hide-from-process',label:'Hide from a process',supported:true,qualified:true,reason:'Profile-controlled hiding.'}],enabled:true,qualified:true,reason:'Native inspection evidence.',intent:{mode:'hide-from-process',executable:'C:\\Games\\Game.exe'},selected:['exact-physical-source','other-physical'],selectedCount:2,global:{inspected:true,active:true,inverse:false,hiddenCount:2,applicationCount:1,applications:['C:\\Tools\\Tool.exe']},effective:{hidden:true,configured:true,inverse:false,scope:'global'},job:null,recovery:{required:true,id:'native-recovery-session'}});
const transpile = source => ts.transpileModule(source, {
  compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022},
}).outputText;
const observationHelper = transpile(await readFile(new URL('./visible-observation.ts', import.meta.url), 'utf8'));
const flush = () => new Promise(resolve => setImmediate(resolve));

function harness(seed, {send, hidden = false} = {}) {
  const exports = {}, observationExports = {}, requests = [], listeners = new Map();
  const document = new EventTarget();
  document.hidden = hidden;
  vm.runInNewContext(observationHelper, {exports: observationExports, document});
  let mount, handlers, closed = false;
  const injected = source.replace('</script>', `
    status = seed;
    mode = seed.intent.mode;
    executable = intentExecutable(seed.intent);
    capture({action, refresh,
      setDraft(value) { mode = value.mode; executable = intentExecutable(value); },
      get draft() { return {mode, executable}; },
      get status() { return status; },
      get error() { return error; },
    });
  </script>`);
  const {js} = compile(injected, {filename: file.pathname, generate: 'server'});
  vm.runInNewContext(transpile(js.code), {
    exports, seed, document,
    capture(value) { handlers = value; },
    require(name) {
      if (name === 'svelte/internal/server') return server;
      if (name === 'svelte') return {...svelte, onMount: fn => mount = fn};
      if (name === './visible-observation') return observationExports;
      if (name === './streams/shared') return {SharedObservations: class {
        subscribe(topic, listener) {
          assert.ok(!listeners.has(topic));
          listeners.set(topic, listener);
          return () => listeners.delete(topic);
        }
        close() { closed = true; listeners.clear(); }
      }};
      if (name.endsWith('.svelte')) return {default: () => {}};
      if (name === './gateway') return {gatewayRequest: async request => {
        requests.push(plain(request));
        return send ? send(request) : {controllers: {visibility: seed}};
      }};
      throw Error(name);
    },
  });
  const html = render(exports.default, {props: {source: physical}}).body;
  return {
    html, handlers, requests, listeners, mount,
    notify: (topic, state) => listeners.get(topic)(state),
    setHidden(value) {
      document.hidden = value;
      document.dispatchEvent(new Event('visibilitychange'));
    },
    get closed() { return closed; },
  };
}
test('visibility subscribes, coalesces changes during a read, and cleans up',async()=>{
 const native=status('windows');let release;
 const first=new Promise(resolve=>release=resolve);
 let reads=0;
 const h=harness(native,{send:async()=>{if(++reads===1)await first;return{controllers:{visibility:native}};}});
 const cleanup=h.mount();
 assert.equal(reads,0,'Opening waits for the subscription baseline');
 h.notify('controllers.visibility',{status:'ready',frame:{kind:'snapshot'}});await h.handlers.refresh();assert.equal(reads,0,'The visibility snapshot and focus refresh wait for the inventory baseline');h.notify('system.inventory',{status:'ready'});h.notify('system.inventory',{status:'ready'});
 assert.equal(reads,1);
 release();await new Promise(resolve=>setImmediate(resolve));
 assert.equal(reads,2);
 h.setHidden(true);assert.equal(h.listeners.size,0);assert.equal(reads,2);
 h.setHidden(false);assert.equal(h.listeners.size,2);
 h.notify('system.inventory',{status:'ready'});assert.equal(reads,2);
 h.notify('controllers.visibility',{status:'ready'});assert.equal(reads,3);
 await h.handlers.refresh();assert.equal(reads,3,'Visibility changes without pending work do not read');
 cleanup();assert.equal(h.closed,true);
});
test('a passive response cannot cross hide and reveal, even when a fresh baseline arrives first', async () => {
  const native = status('windows');
  const stale = {...native, reason: 'Retired visibility response'};
  const current = {...native, reason: 'Fresh visibility response'};
  let releaseStale, releaseCurrent;
  const staleReply = new Promise(resolve => { releaseStale = resolve; });
  const currentReply = new Promise(resolve => { releaseCurrent = resolve; });
  const h = harness(native, {hidden: true, send: () => h.requests.length === 1 ? staleReply : currentReply});
  const cleanup = h.mount();
  assert.equal(h.listeners.size, 0, 'Opening hidden has no observation demand');
  h.setHidden(false);
  h.notify('controllers.visibility', {status: 'ready'});
  h.notify('system.inventory', {status: 'ready'});
  assert.equal(h.requests.length, 1);
  h.setHidden(true);
  h.setHidden(false);
  h.notify('controllers.visibility', {status: 'ready'});
  releaseStale({controllers: {visibility: stale}});
  await flush();
  assert.equal(h.handlers.status.reason, native.reason);
  assert.equal(h.requests.length, 1, 'The replacement read waits for both new baselines');
  h.notify('system.inventory', {status: 'ready'});
  assert.equal(h.requests.length, 2);
  releaseCurrent({controllers: {visibility: current}});
  await flush();
  assert.equal(h.handlers.status.reason, current.reason);
  cleanup();
  h.setHidden(true);
  h.setHidden(false);
  assert.equal(h.listeners.size, 0);
});
test('an acknowledged user save survives document hide while its old passive error is ignored', async () => {
  const native = status('windows');
  let rejectRead, finishSave;
  const read = new Promise((_, reject) => { rejectRead = reject; });
  const save = new Promise(resolve => { finishSave = resolve; });
  const h = harness(native, {send: request => request.command === 'controller_visibility_save' ? save : read});
  const cleanup = h.mount();
  h.notify('controllers.visibility', {status: 'ready'});
  h.notify('system.inventory', {status: 'ready'});
  const intent = {mode: 'visible'};
  const saving = h.handlers.action({command: 'controller_visibility_save', intent});
  h.setHidden(true);
  finishSave({controllers: {visibility: {...native, intent}}});
  await saving;
  rejectRead(new Error('Retired status read'));
  await flush();
  assert.equal(h.handlers.error, '');
  assert.deepEqual(plain(h.handlers.draft), {...intent, executable: ''});
  cleanup();
});
test('external visibility snapshots follow clean fields and preserve an unsaved draft',async()=>{
 const native=status('windows');let reply={...native,intent:{mode:'visible'}};
 const h=harness(native,{send:async()=>({controllers:{visibility:reply}})});
 const cleanup=h.mount();
 h.notify('controllers.visibility',{status:'ready',frame:{kind:'snapshot'}});h.notify('system.inventory',{status:'ready'});await new Promise(resolve=>setImmediate(resolve));
 assert.deepEqual(plain(h.handlers.draft),{mode:'visible',executable:''});
 h.handlers.setDraft({mode:'hide-from-process',executable:'C:\\Games\\Draft.exe'});
 reply={...native,intent:{mode:'hide-all'}};
 h.notify('controllers.visibility',{status:'ready',frame:{kind:'change'}});await new Promise(resolve=>setImmediate(resolve));
 assert.equal(h.handlers.status.intent.mode,'hide-all');
 assert.deepEqual(plain(h.handlers.draft),{mode:'hide-from-process',executable:'C:\\Games\\Draft.exe'});
 cleanup();
});
test('qualified visibility UI has local scope, inspection and restoration but no standalone Apply, bulk Apply or process launch forms',()=>{
 for(const platform of['windows','linux','macos']){const native=status(platform);native.launch={implemented:true,qualified:true,reason:'Previously offered isolated launch.',job:null};const h=harness(native);for(const text of['Manage HidHide setup in Settings','Saved policy','Executable path','Choose a running application','Save visibility selection','Restore visibility','Author and apply device hiding in Profiles','active global HidHide rule'])assert.ok(h.html.includes(text),text);for(const text of['Apply visibility','Apply all saved selections','Launch an isolated application','Launch isolated application','Application directory','Arguments (one per line)'])assert.ok(!h.html.includes(text),text);assert.deepEqual(h.requests,[]);}
});
test('actual retained handlers preserve exact source-scoped native preferences, process inspection and recovery without hiding or launching',async()=>{
 const native=status('windows'),h=harness(native);for(const request of[{command:'controller_visibility_save',intent:{mode:'hide-from-process',executable:'C:\\Games\\Other.exe'}},{command:'controller_visibility_enable',enabled:false},{command:'controller_visibility_inspect'},{command:'controller_visibility_processes',offset:0},{command:'controller_visibility_restore',jobId:'native-job',sessionId:'native-recovery-session'}])await h.handlers.action(request);
 assert.deepEqual(h.requests.map(v=>v.command),['controller_visibility_save','controller_visibility_enable','controller_visibility_inspect','controller_visibility_processes','controller_visibility_restore']);assert.ok(h.requests.every(v=>v.sourceId===physical.id&&!('launch'in v)&&!('sourceIds'in v)));assert.equal(h.requests[0].intent.executable,'C:\\Games\\Other.exe');assert.equal(h.requests[4].sessionId,'native-recovery-session');assert.equal(h.handlers.status,native);
});
test('startup HidHide access reports every native outcome without changing hidden rules or issuing a setup request',()=>{
 const labels={configured:'Configured automatically',unchanged:'Already configured',absent:'Not installed',unsupported:'Unsupported installation',blocked:'Blocked',error:'Setup error','not-requested':'Not requested'};
 for(const [outcome,label] of Object.entries(labels)){
  const native=status('windows');native.selfAccess={status:outcome,ready:['configured','unchanged'].includes(outcome),changed:outcome==='configured',executable:'\\Device\\Volume\\Cockpit.exe',message:'Exact startup result: '+outcome};
  const h=harness(native),container=h.html.match(/<div[^>]*data-self-access-status="[^"]+"[^>]*>/)?.[0];
  assert.ok(container,outcome);assert.ok(container.includes('aria-label="Startup HidHide access"'));
  assert.equal(/\battention\b/.test(container),['unsupported','blocked','error'].includes(outcome),outcome);
  assert.ok(h.html.includes('Startup HidHide access: '+label),outcome);assert.ok(h.html.includes(native.selfAccess.message));
  assert.ok(h.html.includes(native.selfAccess.executable));assert.equal(h.html.includes('Access was verified at startup.'),native.selfAccess.ready);
  assert.ok(h.html.includes('later HidHide changes can affect capture'));assert.ok(h.html.includes('has an active global HidHide rule'));
  assert.deepEqual(h.requests,[],'Rendering startup evidence is passive.');
 }
 const native=status('windows');native.selfAccess={status:'error',ready:false,changed:null,message:'Write acknowledgement was lost.'};
 const ambiguous=harness(native);assert.ok(ambiguous.html.includes('Startup access may have changed; its final state could not be verified.'));assert.ok(!ambiguous.html.includes('Access was verified at startup.'));assert.deepEqual(ambiguous.requests,[]);
 assert.ok(!harness(status('windows')).html.includes('Startup HidHide access:'),'Older services without this result remain usable.');
});
test('an already active isolated child retains only native cleanup and a lost restoration reply is surfaced once without replay',async()=>{
 const native=status('linux');native.qualified=false;native.launch={job:{jobId:'exact-retained-child',active:true,phase:'running',message:'An existing owned child is running.'}};const h=harness(native,{send:async()=>{throw Error('Restoration reply unavailable.');}});assert.ok(h.html.includes('Previous isolated application'));assert.ok(h.html.includes(native.launch.job.message));assert.match(h.html,/<button[^>]*>(?:<!--.*?-->)*Stop retained isolated application<\/button>/);assert.ok(!h.html.includes('Application directory'));await h.handlers.action({command:'controller_visibility_restore',jobId:native.launch.job.jobId});assert.deepEqual(h.requests,[{command:'controller_visibility_restore',jobId:'exact-retained-child',sourceId:physical.id}]);assert.match(h.handlers.error,/Restoration reply unavailable/);assert.equal(h.handlers.status.launch.job.active,true);
 const types=await readFile(new URL('./visibility.ts',import.meta.url),'utf8'),gateway=await readFile(new URL('./gateway.ts',import.meta.url),'utf8');assert.ok(!types.includes('controller_visibility_apply'));assert.ok(!types.includes('controller_visibility_launch'));assert.ok(!types.includes('IsolatedLaunch'));assert.ok(!gateway.includes('launch?:'));
});
