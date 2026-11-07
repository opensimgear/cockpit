import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import test from 'node:test';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

async function installHarness(page) {
  return page.evaluate(`(async()=>{
    const {Channel,invoke}=await import('/node_modules/@tauri-apps/api/core.js');
    const {gatewayRequest}=await import('/src/lib/gateway.ts');
    const {ControllerInputObservations}=await import('/src/lib/controller-observations.ts');
    const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
    const {SharedObservations}=await import('/src/lib/streams/shared.ts');
    const {controllers}=await gatewayRequest({command:'controller_list',offset:0});
    const source=controllers.sources.find(row=>row.online&&row.baselineReady);
    if(!source)throw Error('An attached controller with a native baseline is required.');
    const topic='controllers.input.'+source.id, sessions=[];
    const shared=new SharedObservations({open(topic,observer,signal){
      const session=sessions.at(-1);
      createNativeObservationTransport({
        async invoke(command,args){
          if(command==='observation_acknowledge'){
            session.receipts++;
            if(session.mode==='receipt'){
              session.heldReceipt={...args};
              return new Promise((resolve,reject)=>{
                session.releaseReceipt=async()=>{
                  try{resolve(await invoke(command,args));return {accepted:true};}
                  catch(error){reject(error);return {accepted:false,error:String(error)};}
                };
              });
            }
          }
          try{
            const result=await invoke(command,args);
            if(command==='observation_subscribe')session.token=result;
            if(command==='observation_acknowledge')session.accepted++;
            if(command==='observation_unsubscribe')session.closed=true;
            return result;
          }catch(error){session.errors.push(String(error));throw error;}
        },
        channel(receive){
          return new Channel(value=>{
            if(session.mode==='frame'&&!session.releaseFrame){
              session.heldFrame=value.subscriptionId;
              session.releaseFrame=()=>receive(value);
            }else receive(value);
          });
        },
      }).open(topic,observer,signal);
    }});
    const controller=new ControllerInputObservations(shared);
    window.__delayedController={
      sessions, shared, topic,
      open(mode){
        const session={mode,receipts:0,accepted:0,errors:[],states:0};
        sessions.push(session);
        session.stop=controller.observe(source.id,state=>{
          session.states++;
          session.status=state.status;
          if(state.status==='lost'||state.status==='error')session.errors.push(state.error??state.status);
          if(state.status==='ready'){
            session.generation=state.detail.source.generation;
            session.baselineReady=state.detail.source.baselineReady;
          }
        });
      },
      read(){
        return {current:shared.subscriptionId(topic),sessions:sessions.map(({
          stop,releaseFrame,releaseReceipt,...state
        })=>state)};
      },
    };
    return {backend:source.backend,controlCount:source.controlCount};
  })()`);
}

test('retired controller preview frame and receipt cannot affect a reopened native preview', {
  skip:process.env.OSG_CONTROLLER_DELAYED_CALLBACK_NATIVE!=='1', timeout:45000,
}, async()=>{
  const page=await nativePage();
  const read=()=>page.evaluate('window.__delayedController.read()');
  const open=mode=>page.evaluate(`window.__delayedController.open(${JSON.stringify(mode)})`);
  const close=async index=>{
    await page.evaluate(`window.__delayedController.sessions[${index}].stop()`);
    return until(read,state=>state.sessions[index].closed);
  };
  try{
    await until(()=>page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    const source=await installHarness(page);
    await open('frame');
    const {sessions:[first]}=await until(read,state=>state.sessions[0].token&&state.sessions[0].heldFrame);
    assert.equal(first.heldFrame,first.token,'The delayed frame must come from the real native subscription');
    assert.equal(first.status,'connecting');
    await close(0);

    await open('receipt');
    const {sessions:[,second]}=await until(read,state=>state.sessions[1].status==='ready'&&state.sessions[1].heldReceipt);
    assert.notEqual(second.token,first.token);
    assert.equal(second.heldReceipt.subscriptionId,second.token);
    assert.equal(second.baselineReady,true);
    const frameRelease=await page.evaluate(`(()=>{
      const probe=window.__delayedController;
      const before=probe.read();
      probe.sessions[0].releaseFrame();
      return {before,after:probe.read()};
    })()`);
    assert.deepEqual(frameRelease.after,frameRelease.before,'The old native frame must not publish or acknowledge');
    await close(1);

    await open('live');
    const {sessions:[,,current]}=await until(read,state=>state.sessions[2].status==='ready'&&state.sessions[2].accepted>0);
    assert.notEqual(current.token,first.token);
    assert.notEqual(current.token,second.token);
    assert.equal(current.baselineReady,true);
    const staleReceipt=await page.evaluate('window.__delayedController.sessions[1].releaseReceipt()');
    assert.equal(staleReceipt.accepted,false);
    assert.match(staleReceipt.error,/observation subscription is not owned/i);
    const after=await read();
    assert.equal(after.current,current.token);
    assert.equal(after.sessions[2].status,'ready');
    assert.equal(after.sessions[2].baselineReady,true);
    assert.equal(after.sessions[2].generation,current.generation);
    assert.deepEqual(after.sessions.map(session=>session.errors),[[],[],[]]);
    assert.equal(after.sessions[1].accepted,0);
    await close(2);

    await mkdir('.research/reactive/controller-delayed-callback',{recursive:true});
    await writeFile('.research/reactive/controller-delayed-callback/evidence.json',JSON.stringify({
      source, distinctNativeSubscriptions:3, allUnsubscribed:true,
      delayedFrameIgnored:true, retiredReceiptRejected:staleReceipt.error, reopenedReady:true,
      physicalReportGeneration:current.generation,
    },null,2)+'\n');
  }finally{
    await page.evaluate(`(()=>{
      const probe=window.__delayedController;
      for(const session of probe?.sessions??[])session.stop();
      probe?.shared.close();
      delete window.__delayedController;
    })()`).catch(()=>{});
    page.close();
  }
});
