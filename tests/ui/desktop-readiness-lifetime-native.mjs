import assert from 'node:assert/strict';
import test from 'node:test';
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';

test('Debug desktop readiness opens only on demand and releases each native reader', {
  skip:process.env.OSG_DESKTOP_READINESS_LIFETIME_NATIVE!=='1',timeout:60000,
},async()=>{
  const page=await nativePage();
  const navigate=view=>page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="${view}"]').click()`);
  const tab=name=>page.evaluate(`[...document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')].find(item=>item.textContent.trim()===${JSON.stringify(name)}).click()`);
  const trace=()=>page.evaluate(`(()=>{const trace=window.__desktopLifetime;return {
    opens:trace.opens,closed:trace.closed,reads:trace.reads,
    id:trace.reader?.subscriptionId('profiles.desktop')
  };})()`);
  const readiness=()=>page.evaluate(`[...document.querySelectorAll('details')].find(item=>item.querySelector('summary')?.textContent.trim()==='Native runtime and desktop output details')?.querySelector('strong')?.textContent`);
  const details = () => page.evaluate(`[...document.querySelectorAll('.profile-debug details > summary')]
    .find(item => item.textContent.trim() === 'Native runtime and desktop output details').click()`);
  const visible = shown => page.evaluate(`(() => {
    if (${shown}) delete document.hidden;
    else Object.defineProperty(document, 'hidden', {configurable: true, value: true});
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  let breakpoint;
  try{
    await until(() => page.evaluate(`!!document.querySelector('nav[aria-label="Main navigation"]')`));
    await navigate('Overview');
    await page.evaluate(`(async()=>{
      const {SharedObservations}=await import('/src/lib/streams/shared.ts');
      const original=SharedObservations.prototype.subscribe;
      const trace=window.__desktopLifetime={opens:0,closed:[],reads:0,restore:()=>{SharedObservations.prototype.subscribe=original;}};
      SharedObservations.prototype.subscribe=function(topic,...args){
        if(topic==='profiles.desktop'){trace.opens++;trace.reader=this;}
        return original.call(this,topic,...args);
      };
    })()`);
    await page.call('Debugger.enable');
    const {result}=await page.call('Runtime.evaluate',{expression:'window.__TAURI_INTERNALS__.invoke'});
    ({breakpointId:breakpoint}=await page.call('Debugger.setBreakpointOnFunctionCall',{
      objectId:result.objectId,condition:`(
        cmd==='observation_unsubscribe'&&window.__desktopLifetime.closed.push(payload.subscriptionId),
        cmd==='gateway_request'&&payload?.request?.command==='profile_desktop_status'&&window.__desktopLifetime.reads++,false)`,
    }));
    await navigate('Debug');
    await until(()=>page.evaluate(`!!document.querySelector('[aria-label="Debug sections"]')`));
    assert.equal((await trace()).opens,0,'Incoming telemetry must leave desktop readiness closed');
    const ids=new Set();
    await tab('Runtime & recovery');
    await until(() => page.evaluate(`!!document.querySelector('[role="tabpanel"]:not([hidden]) details.native-details')`));
    assert.equal((await trace()).opens,0,'Collapsed details must not open a desktop readiness reader');
    await visible(false);
    await details();
    assert.equal((await trace()).opens,0,'Opening while hidden must defer the reader');
    await visible(true);
    for(let iteration=0;iteration<3;iteration++){
      const opened=await until(trace,state=>state.opens===iteration+1&&!!state.id);
      assert.ok(!ids.has(opened.id),'Reopening must acquire a fresh native reader');
      ids.add(opened.id);
      await until(readiness,state=>state==='stopped');
      if (iteration === 1) await visible(false);
      else await details();
      await until(trace,state=>state.closed.includes(opened.id));
      await until(readiness,state=>state==='Unavailable');
      if (iteration === 1) await visible(true);
      else await details();
    }
    const final=await until(trace,state=>state.opens===4&&!!state.id);
    await until(readiness,state=>state==='stopped');
    await navigate('Overview');
    await until(trace,state=>state.closed.includes(final.id));
    assert.equal((await trace()).reads,0,'Live readiness must not request detailed desktop status');
  }finally{
    try{
      await visible(true);
      if(breakpoint)await page.call('Debugger.removeBreakpoint',{breakpointId:breakpoint});
      await page.call('Debugger.disable');
      await page.evaluate('window.__desktopLifetime?.restore();delete window.__desktopLifetime;');
    }finally{page.close();}
  }
});
