import assert from'node:assert/strict';import test from'node:test';import{readFile}from'node:fs/promises';import vm from'node:vm';import ts from'typescript';import{compile}from'svelte/compiler';import{render}from'svelte/server';import*as server from'svelte/internal/server';import*as svelte from'svelte';import{loadModules}from'./test-module-loader.mjs';
const api=(await loadModules(['./profile-retired-recovery'])).get('./profile-retired-recovery'),uuid='11111111-1111-4111-8111-111111111111',other='22222222-2222-4222-8222-222222222222',hash=`sha256:${'a'.repeat(64)}`,plain=v=>JSON.parse(JSON.stringify(v));
const reference=()=>({id:{kind:'manual'},owner:{session:uuid,generation:'1'},recordHash:hash}),provision=()=>({operation:uuid,recordHash:hash,target:2,refreshUncertain:true});
const empty=()=>({required:false,outputs:[],provisioning:[],job:null,diagnostics:[],outputQualified:false,provisioningQualified:false});const pending=()=>({...empty(),required:true,outputs:[{reference:reference(),phase:'confirmed',slots:[1,2]}],provisioning:[provision()]});
const filename=new URL('./ProfileRetiredRecovery.svelte',import.meta.url),source=await readFile(filename,'utf8');
async function harness(native, {active = true, send} = {}) {
  const requests = [];
  const controller = new api.RetiredRecoveryController(new api.RetiredRecoveryClient(async request => {
    requests.push(plain(request));
    return send ? send(request) : native;
  }));
  if (native) await controller.refresh();
  requests.length = 0;

  const effects = [], observers = [], events = new Map(), exports = {};
  const document = {
    hidden: false,
    addEventListener(name, listener) { events.set(name, listener); },
    removeEventListener(name) { events.delete(name); },
  };
  class SharedObservations {
    constructor() { observers.push(this); }
    subscribe(topic, listener) { this.topic = topic; this.listener = listener; }
    close() { this.closed = true; }
  }

  // SSR has no effect scheduler. Capture the actual callbacks and explicitly
  // drive their lifecycle so tests can deliver a baseline or lose visibility.
  const injected = source.replaceAll('$effect(()=>{', 'captureEffect(()=>{')
    .replace('</script>', 'capture({output,recoverProvision,confirm,mountStatus});</script>');
  const {js} = compile(injected, {filename:filename.pathname,generate:'server'});
  let handlers;
  vm.runInNewContext(ts.transpileModule(js.code, {compilerOptions:{
    module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,
  }}).outputText, {
    exports, document,
    capture: value => handlers = value,
    captureEffect: effect => effects.push(effect),
    require(name) {
      if (name === 'svelte/internal/server') return {...server, derived: read => read};
      if (name === 'svelte') return svelte;
      if (name === './profile-retired-recovery') return api;
      if (name === './streams/shared') return {SharedObservations};
      if (name === './gateway') return {gatewayRequest(){throw Error('Supplied controller must own every request.');}};
      throw Error(name);
    },
  });
  const html = render(exports.default, {props:{controller,active}}).body;
  const open = () => effects[0]();
  const setHidden = hidden => {
    document.hidden = hidden;
    events.get('visibilitychange')?.();
  };
  const deliver = async state => {
    observers.at(-1).listener(state);
    effects[1]();
    await new Promise(setImmediate);
  };
  const ready = async () => {
    const unmount = handlers.mountStatus();
    const stop = open();
    await deliver({status:'ready'});
    requests.length = 0;
    return () => { stop(); unmount(); };
  };
  return {controller, requests, handlers, html, observers, events, open, setHidden, deliver, ready};
}
test('actual cleanup component is invisible for clean status, missing native state cannot invent successful recovery, and rendering never mutates',async()=>{
 for(const state of [empty(),null]){const h=await harness(state);assert.ok(!h.html.includes('<section'));assert.deepEqual(h.requests,[]);}
 const h=await harness({...empty(),diagnostics:['A retained journal cannot be read.']});assert.ok(h.html.includes('A retained journal cannot be read.'));assert.ok(h.html.includes('Previous output setup recovery'));assert.ok(!h.html.includes('Apply profile'));assert.deepEqual(h.requests,[]);
});
test('actual unqualified retained output and setup records remain visible with exact ownership but cannot dispatch recovery or acknowledgement',async()=>{
 const state=pending();state.outputs[0].phase='intent';const h=await harness(state);for(const text of ['Recorded slots: 1, 2','Owner '+uuid,'uncertain','Acknowledge without output','platform qualification','Target slot 2'])assert.ok(h.html.includes(text),text);assert.match(h.html,/<input[^>]*disabled/);const close=await h.ready();await h.handlers.output(reference(),true);await h.handlers.recoverProvision(provision());assert.deepEqual(h.requests,[]);close();
});
test('actual qualified acknowledgement requires an explicit checkbox and sends only the exact unchanged native reference; confirmed recovery is separate',async()=>{
 const state={...pending(),outputQualified:true,provisioningQualified:true};state.outputs[0].phase='releasing';const h=await harness(state),close=await h.ready();await h.handlers.output(reference(),true);assert.equal(h.requests.length,0);h.handlers.confirm(reference(),true);await h.handlers.output(reference(),true);assert.deepEqual(h.requests,[{command:'profile_retired_recovery_acknowledge',reference:reference()}]);await h.handlers.recoverProvision(provision());assert.equal(h.requests[1].command,'profile_retired_recovery_provision_recover');close();const confirmed=await harness({...state,outputs:[{reference:reference(),phase:'confirmed',slots:[1]}]}),closeConfirmed=await confirmed.ready();await confirmed.handlers.output(reference());assert.equal(confirmed.requests[0].command,'profile_retired_recovery_recover');assert.ok(!confirmed.html.includes('I confirm'));closeConfirmed();
});
test('cleanup reads follow visible baselines and loss or disposal prevents recovery', async () => {
  const h = await harness({...pending(),outputQualified:true});
  const unmount = h.handlers.mountStatus();
  const stop = h.open();
  assert.equal(h.observers[0].topic, 'profiles.retired-recovery');
  await h.handlers.output(reference());
  assert.deepEqual(h.requests, [], 'The component must wait for its first baseline.');
  await h.deliver({status:'ready'});
  assert.deepEqual(h.requests.map(request => request.command), ['profile_retired_recovery_status']);
  await h.deliver({status:'lost',error:'Source closed'});
  await h.handlers.output(reference());
  assert.equal(h.requests.length, 1);
  h.setHidden(true);
  stop();
  assert.equal(h.open(), undefined);
  assert.equal(h.observers[0].closed, true);
  assert.equal(h.observers.length, 1);
  await h.handlers.output(reference());
  assert.equal(h.requests.length, 1);
  unmount();
  assert.equal(h.events.size, 0);
  h.controller.dispose();
  await h.controller.refresh();
  assert.equal(h.requests.length, 1);
  const inactive = await harness(pending(), {active:false});
  const unmountInactive = inactive.handlers.mountStatus();
  assert.equal(inactive.open(), undefined);
  await inactive.handlers.output(reference());
  assert.deepEqual(inactive.requests, []);
  unmountInactive();
});
test('actual job cancellation remains an explicit native operation while qualification is closed and does not claim restoration',async()=>{
 const state={...pending(),job:{jobId:other,operation:uuid,active:true,state:'running',message:'Native owned cleanup pending.'}},h=await harness(state);assert.ok(h.html.includes('Cancel cleanup job'));assert.ok(h.html.includes(state.job.message));assert.ok(!h.html.includes('Cleanup completed'));await h.controller.cancel();assert.deepEqual(h.requests,[{command:'profile_retired_recovery_provision_cancel',jobId:other}]);
});
