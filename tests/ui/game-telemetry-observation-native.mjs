import assert from 'node:assert/strict';
import test from 'node:test';
import { nativePage, until } from '../../scripts/reactive/cdp.mjs';

test('Debug telemetry retains its stream through query changes and closes on exit', { timeout: 45000 }, async () => {
  const page = await nativePage();
  const navigate = view => page.evaluate(`document.querySelector('nav[aria-label="Main navigation"] button[title="${view}"]').click()`);
  const trace = () => page.evaluate('window.__telemetryLifetime');
  const currentSummary = () => page.evaluate(`(()=>{
    const inspector=document.querySelector('[aria-label="Incoming game telemetry"]');
    const name=inspector?.querySelector('[aria-label="Game telemetry source"] h2')?.textContent;
    const selected=inspector?.querySelector('[aria-label="Telemetry game"]')?.selectedOptions[0]?.textContent;
    return !!name&&selected?.startsWith(name)&&!inspector.querySelector('[role="alert"]');
  })()`);
  let breakpoint;
  try {
    await navigate('Overview');
    await page.evaluate('window.__telemetryLifetime={opens:0,reads:[],closed:[]}');
    await page.call('Debugger.enable');
    const {result}=await page.call('Runtime.evaluate', {expression:'window.__TAURI_INTERNALS__.invoke'});
    ({breakpointId:breakpoint}=await page.call('Debugger.setBreakpointOnFunctionCall', {
      objectId:result.objectId,
      condition:`(cmd==='observation_subscribe'&&payload?.topic==='games.telemetry'&&window.__telemetryLifetime.opens++,
        cmd==='observation_unsubscribe'&&window.__telemetryLifetime.closed.push(payload.subscriptionId),
        cmd==='gateway_request'&&payload?.request?.command==='game_telemetry_inspect'&&window.__telemetryLifetime.reads.push(payload.request),false)`,
    }));
    await navigate('Debug');
    const first=await until(trace,state=>state.opens===1&&state.reads.length>0);
    const observationId=first.reads[0].observationId;
    assert.ok(observationId);
    await page.evaluate(`(()=>{
      const search=document.querySelector('[aria-label="Search game telemetry"]');
      search.value='speed';search.dispatchEvent(new Event('input',{bubbles:true}));
    })()`);
    const filtered=await until(trace,state=>state.reads.some(read=>read.search==='speed'));
    assert.equal(filtered.opens,1);
    assert.ok(filtered.reads.every(read=>read.observationId===observationId));
    assert.ok(!filtered.closed.includes(observationId));
    await until(currentSummary);
    const gameId=await page.evaluate(`(()=>{
      const game=document.querySelector('[aria-label="Telemetry game"]');
      const choice=[...game.options].find(option=>option.value&&option.value!==game.value);
      if(!choice)throw Error('The fixture needs two available game choices');
      game.value=choice.value;game.dispatchEvent(new Event('change',{bubbles:true}));
      return choice.value;
    })()`);
    const switched=await until(trace,state=>state.reads.some(read=>read.gameId===gameId&&!read.search));
    assert.equal(switched.opens,1);
    assert.ok(switched.reads.every(read=>read.observationId===observationId));
    await until(currentSummary);
    const tab = name => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')).find(button=>button.textContent.trim()===${JSON.stringify(name)}).click()`);
    await tab('Profile inspection');
    await until(trace,state=>state.closed.includes(observationId));
    await tab('Incoming telemetry');
    const reopened=await until(trace,state=>state.opens===2&&state.reads.some(read=>read.observationId!==observationId));
    const freshId=reopened.reads.at(-1).observationId;
    assert.ok(freshId);
    assert.notEqual(freshId,observationId);
    await until(currentSummary);
    await navigate('Overview');
    await until(trace,state=>state.closed.includes(freshId));
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', {breakpointId:breakpoint});
      await page.call('Debugger.disable');
      await page.evaluate('delete window.__telemetryLifetime');
    } finally { page.close(); }
  }
});
