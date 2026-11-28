import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const {HidInspectionController}= (await loadModules(['./controller-hid'])).get('./controller-hid');
const plain=value=>JSON.parse(JSON.stringify(value));
const fixture=()=>{
  const calls=[];
  const request=async args=>{
    calls.push(plain(args));
    if(args.command==='controller_hid_list')return {interfaces:[{interfaceId:'a',product:'Throttle'}],failures:[],truncated:false};
    if(args.command==='controller_hid_catalog')return {snapshots:[{interfaceId:'b',status:'last-observed',availability:'offline'}]};
    if(args.command==='controller_hid_inspect')return {interfaceId:args.interfaceId,contractFingerprint:'contract-a',capabilities:{reports:[]}};
    if(args.command==='controller_hid_open')return {interfaceId:args.interfaceId,generation:'generation-a',contractFingerprint:'contract-a'};
    if(args.command==='controller_hid_close')return {...args,result:{closed:true}};
    if(args.command==='controller_hid_state')return {...args,result:{status:'known',values:[{value:42}]}};
    if(args.command==='controller_hid_reports')return {...args,result:{reports:[{sequence:'1',wireHex:'0001'}],gap:true,terminal:null}};
    if(args.command==='controller_hid_snapshot')return {interfaceId:args.interfaceId,status:'last-observed',availability:'offline',detail:{capabilities:{}}};
    throw Error('Unexpected command');
  };
  return {calls,request,controller:new HidInspectionController(request)};
};
test('inventory and explicit selection never open; selected capture preserves native state and report evidence',async()=>{
  const {controller,calls}=fixture();
  await controller.refresh();
  assert.equal(controller.state.interfaces[0].product,'Throttle');
  assert.equal(controller.state.snapshots[0].availability,'offline');
  await controller.select('a');
  assert.equal(controller.state.detail.contractFingerprint,'contract-a');
  assert.equal(calls.some(call=>call.command==='controller_hid_open'),false);
  await controller.open();
  await controller.read();
  assert.equal(controller.state.current.result.values[0].value,42);
  assert.equal(controller.state.reports.result.gap,true);
  assert.deepEqual(calls.find(call=>call.command==='controller_hid_open'),{command:'controller_hid_open',interfaceId:'a',contractFingerprint:'contract-a'});
  await controller.select('b',true);
  assert.equal(controller.state.session,null);
  assert.equal(controller.state.detail.status,'last-observed');
  assert.ok(calls.findIndex(call=>call.command==='controller_hid_close')<calls.findIndex(call=>call.command==='controller_hid_snapshot'));
  const count=calls.length;await controller.open();assert.equal(calls.length,count);
});
test('selection replacement and teardown close a late open without displaying its generation',async()=>{
  for(const action of ['select','dispose']){
    const fixtureValue=fixture();const {calls,request}=fixtureValue;let finish;
    const controller=new HidInspectionController(args=>args.command==='controller_hid_open'?new Promise(resolve=>{calls.push(plain(args));finish=resolve;}):request(args));
    await controller.select('a');const opening=controller.open();
    if(action==='select')await controller.select('b');else await controller.dispose();
    finish({interfaceId:'a',generation:'late',contractFingerprint:'contract-a'});await opening;
    assert.equal(controller.state.session,null);
    assert.deepEqual(calls.at(-1),{command:'controller_hid_close',interfaceId:'a',generation:'late'});
    assert.equal(controller.state.selected,action==='select'?'b':'');
  }
});
test('errors, mismatched generations and stale reads cannot appear as current selected state',async()=>{
  const {request}=fixture();let fail=false;
  const controller=new HidInspectionController(async args=>{
    if(fail)throw Error('Native disconnected');
    const value=await request(args);
    return args.command==='controller_hid_state'?{...value,generation:'stale'}:value;
  });
  await controller.select('a');await controller.open();await controller.read();
  assert.match(controller.state.error,/generation/);assert.equal(controller.state.current,null);
  fail=true;await controller.close();assert.match(controller.state.error,/disconnected/);
  assert.equal(controller.state.session,null);await controller.refresh();assert.equal(controller.state.interfaces.length,0);
  await controller.select('a');assert.match(controller.state.error,/disconnected/);
});
test('a capture opened by another client shares its native generation and is never closed by this view',async()=>{
  for(const action of ['close','select','dispose']){
    const {calls,request}=fixture();
    const controller=new HidInspectionController(async args=>args.command==='controller_hid_list'
      ?{interfaces:[{interfaceId:'a',captureGeneration:'shared-generation',contractFingerprint:'contract-a'}]}:request(args));
    await controller.refresh();await controller.select('a');await controller.open();await controller.read();
    assert.equal(controller.state.session.generation,'shared-generation');assert.equal(controller.state.ownsSession,false);
    if(action==='select')await controller.select('b');else await controller[action]();
    assert.ok(calls.some(call=>call.command==='controller_hid_state'&&call.generation==='shared-generation'));
    assert.ok(calls.every(call=>!['controller_hid_open','controller_hid_close'].includes(call.command)));
  }
});
test('successive reads advance by the returned sequence and a new generation resets the cursor',async()=>{
  const {request,calls}=fixture();
  const controller=new HidInspectionController(async args=>{
    if(args.command==='controller_hid_reports'){
      calls.push(plain(args));return {...args,result:{reports:[{sequence:(BigInt(args.after)+1n).toString(),wireHex:'0000'}],gap:false,terminal:null}};
    }
    return request(args);
  });
  await controller.select('a');await controller.open();await controller.read();await controller.read();
  assert.deepEqual(calls.filter(call=>call.command==='controller_hid_reports').map(call=>call.after),['0','1']);
  assert.equal(controller.state.reports.result.reports[0].sequence,'2');
  assert.equal(controller.state.reports.result.reports[0].wireHex,'0000');
  await controller.close();await controller.open();await controller.read();
  assert.equal(calls.filter(call=>call.command==='controller_hid_reports').at(-1).after,'0');
  await controller.dispose();assert.equal(calls.at(-1).command,'controller_hid_close');
});
