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
    if(args.command==='controller_hid_record_list')return {recordings:[]};
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
  fail=true;await controller.close();
  assert.equal(controller.state.session,null);assert.equal(controller.state.retired.status,'replaced');
  await controller.refresh();assert.equal(controller.state.interfaces.length,0);assert.match(controller.state.error,/disconnected/);
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

test('explicit bounded recording and GET retain native outcomes; sealed archive remains exportable after close',async()=>{
  const {request,calls}=fixture();
  const archive={format:'osg-hid-recording',records:[{sequence:'1',wireHex:'0000'}]};
  const archiveBytes=Buffer.from(JSON.stringify({recording:archive}));
  const controller=new HidInspectionController(async args=>{
    if(args.command.includes('_record_')||args.command==='controller_hid_get'){
      calls.push(plain(args));return {...args,result:args.command.endsWith('_export')?{chunk:archiveBytes.subarray(args.offset,args.offset+args.limit).toString('base64'),offset:args.offset,nextOffset:Math.min(archiveBytes.length,args.offset+args.limit),totalBytes:archiveBytes.length}:args.command==='controller_hid_get'?{state:'failed',reason:'unsupported'}:{status:args.command.endsWith('_start')?'recording':'stopped'}};
    }return request(args);
  });
  await controller.select('a');await controller.open();await controller.record('start',16384,3);
  assert.equal(controller.state.recording.result.status,'recording');
  await controller.get('feature',2,64,50);assert.equal(controller.state.getResult.result.reason,'unsupported');
  const get=calls.find(row=>row.command==='controller_hid_get');assert.equal(get.generation,'generation-a');assert.ok(get.requestId);
  await controller.record('stop');await controller.close();
  assert.deepEqual(JSON.parse(Buffer.concat(await controller.exportRecording()).toString()).recording,archive);
  await controller.record('status');await controller.record('discard');assert.equal(controller.state.recordingTarget,null);
  const count=calls.length;await controller.get('input',0,1,1);assert.equal(calls.length,count);
});

test('offline replay pages rebuild native state deterministically and never open a device',async()=>{
  const calls=[];
  const controller=new HidInspectionController(async args=>{
    calls.push(plain(args));
    if(args.command.endsWith('_import'))return {replayId:'offline',metadata:{generation:'original'},descriptor:{interfaceId:'source'}};
    if(args.command.endsWith('_close'))return {replayId:'offline',closed:true};
    if(args.command.endsWith('_read'))return {replayId:'offline',records:[{sequence:args.after,wireHex:'0000'}],state:{status:'live',values:[{sample:{value:'18446744073709551615'}}]},next:String(Number(args.after)+1),hasMore:args.after==='0',positionUs:args.elapsedUs,complete:args.after!=='0'};
    throw Error('unexpected live call');
  });
  await controller.importRecording('{"format":"osg-hid-recording"}');await controller.replayRead('0','20');
  const first=plain(controller.state.current);assert.equal(controller.state.reports.result.reports.length,2);
  await controller.replayRead('0','20');assert.deepEqual(plain(controller.state.current),first);
  await controller.replayRead('0',undefined,1);assert.equal(calls.at(-1).limit,1);
  await controller.select('');assert.equal(controller.state.replay,null);
  assert.ok(calls.every(row=>row.command.includes('replay')));
  await controller.importRecording('{}');await controller.dispose();assert.equal(calls.at(-1).command,'controller_hid_replay_close');
});

test('malformed offline import reports native rejection and never establishes a capture',async()=>{
  const calls=[];const controller=new HidInspectionController(async args=>{calls.push(args);throw Error('Invalid recording');});
  await controller.importRecording('not JSON');assert.match(controller.state.error,/Invalid recording/);
  assert.equal(controller.state.session,null);assert.equal(controller.state.replay,null);assert.equal(calls.length,1);
});

test('retained recording discovery and selection require no live device open',async()=>{
  const {request,calls}=fixture();
  const controller=new HidInspectionController(async args=>{
    if(args.command==='controller_hid_record_list')return {recordings:[{interfaceId:'offline',generation:'old',status:'stopped'}]};
    if(args.command==='controller_hid_record_status'){calls.push(plain(args));return {...args,result:{status:'stopped'}};}
    return request(args);
  });
  await controller.refresh();assert.equal(controller.state.recordings.length,1);
  await controller.selectRecording('offline','old');assert.equal(controller.state.recordingTarget.generation,'old');
  assert.equal(controller.state.session,null);assert.equal(controller.state.recording.result.status,'stopped');
  assert.ok(calls.every(row=>!['controller_hid_open','controller_hid_inspect'].includes(row.command)));
});

test('authoritative inventory reconnect and replacement retire historical generation without closing replacements',async()=>{
  for(const mode of ['removed','replaced','contract']){
    const {request,calls}=fixture();let changed=false;
    const controller=new HidInspectionController(async args=>{
      if(args.command==='controller_hid_list')return {interfaces:changed?(mode==='removed'?[]:[{interfaceId:'a',captureGeneration:mode==='replaced'?'generation-new':'generation-a',contractFingerprint:mode==='contract'?'contract-new':'contract-a'}]):[{interfaceId:'a'}],failures:[],truncated:false};
      if(args.command==='controller_hid_record_list')return {recordings:[{interfaceId:'a',generation:'generation-a',status:'stopped',reason:{kind:'disconnected'}}]};
      if(args.command==='controller_hid_record_start')return {...args,result:{status:'recording'}};
      return request(args);
    });
    await controller.select('a');await controller.open();await controller.read();await controller.record('start');changed=true;
    await controller.refresh();
    assert.equal(controller.state.session,null);assert.equal(controller.state.current,null);assert.equal(controller.state.reports,null);
    assert.equal(controller.state.retired.generation,'generation-a');assert.equal(controller.state.retired.status,mode==='removed'?'disconnected':'replaced');
    assert.equal(controller.state.recording.result.status,'stopped');assert.equal(controller.state.recordingTarget.generation,'generation-a');
    const count=calls.length;await controller.open();await controller.read();assert.equal(calls.length,count);
    await controller.dispose();assert.ok(calls.every(row=>row.command!=='controller_hid_close'));
  }
});

test('terminal state and stale generation responses retire capture; transient read failures preserve its identity',async()=>{
  for(const mode of ['terminal','stale','offline','transient']){
    const {request,calls}=fixture();let changed=false;
    const controller=new HidInspectionController(async args=>{
      if(changed&&args.command==='controller_hid_state'){
        if(mode==='terminal')return {...args,result:{status:'offline',terminal:{kind:'disconnected'}}};
        throw Error(mode==='stale'?'Stale HID generation':mode==='offline'?'Offline: no open HID session':'Temporary gateway timeout');
      }return request(args);
    });
    await controller.select('a');await controller.open();await controller.read();changed=true;await controller.read();
    assert.equal(controller.state.current,null);
    if(mode==='transient'){assert.equal(controller.state.session.generation,'generation-a');assert.equal(controller.state.retired,null);assert.match(controller.state.error,/timeout/);}
    else{assert.equal(controller.state.session,null);assert.ok(controller.state.retired);await controller.dispose();assert.ok(calls.every(row=>row.command!=='controller_hid_close'));}
  }
});

test('invalid import preserves live capture and current offline replay; late imported handles are retired',async()=>{
  const {request}=fixture();let fail=true,finish;const calls=[];
  const controller=new HidInspectionController(async args=>{
    calls.push(plain(args));
    if(args.command==='controller_hid_replay_import'){if(fail)throw Error('Invalid recording');return new Promise(resolve=>{finish=resolve;});}
    if(args.command==='controller_hid_replay_close')return {...args,closed:true};
    return request(args);
  });
  await controller.select('a');await controller.open();await controller.read();
  const current=controller.state.current;await controller.importRecording('broken');
  assert.equal(controller.state.current,current);assert.equal(controller.state.session.generation,'generation-a');
  assert.ok(calls.every(row=>row.command!=='controller_hid_close'));
  fail=false;const importing=controller.importRecording('{}');await controller.select('b');
  finish({replayId:'late',descriptor:{}});await importing;
  assert.equal(controller.state.replay,null);assert.equal(calls.at(-1).command,'controller_hid_replay_close');
  let loaded=false;const offlineCalls=[];
  const offline=new HidInspectionController(async args=>{
    offlineCalls.push(args);
    if(loaded)throw Error('Invalid recording');loaded=true;
    return {replayId:'kept',descriptor:{interfaceId:'source'}};
  });
  await offline.importRecording('{}');const priorReplay=offline.state.replay;
  await offline.importRecording('invalid');assert.equal(offline.state.replay,priorReplay);
  assert.ok(offlineCalls.every(row=>row.command==='controller_hid_replay_import'));
});
