import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import test from 'node:test';
import { browserPage, nativePage, until } from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_TELEMETRY_LIFETIME_BROWSER_URL;
test(`${browserUrl?'browser-dev':'native'} Debug telemetry retains its stream through query changes and closes on exit`, { timeout: 45000 }, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const request = value => page.evaluate(`import('/src/lib/gateway.ts').then(({gatewayRequest})=>gatewayRequest(${JSON.stringify(value)}))`);
  const seedGames = async () => {
    const {games} = await request({command:'game_get_state'});
    if (games.catalogue.filter(game => game.provider).length >= 2) return;
    assert.equal(process.env.OSG_TELEMETRY_LIFETIME_SEED, '1', 'Register two integrated games or opt into seeding an isolated Cockpit');
    const directory = resolve('.research/reactive/telemetry-lifetime', String(Date.now()));
    await mkdir(directory, {recursive:true});
    for (const [provider, name, filename] of [
      ['assetto-corsa', 'Assetto Corsa', 'acs.exe'],
      ['assetto-corsa-evo', 'Assetto Corsa EVO', 'AssettoCorsaEVO.exe'],
    ]) {
      if (games.catalogue.some(game => game.provider === provider)) continue;
      const installPath = resolve(directory, provider);
      await mkdir(installPath, {recursive:true});
      const executable = resolve(installPath, filename);
      await writeFile(executable, 'Telemetry catalogue fixture. Never executed.');
      await request({command:'game_add', name, installPath, executable});
    }
  };
  const navigate = view => until(() => page.evaluate(`(() => {
    const button=document.querySelector('nav[aria-label="Main navigation"] button[title="${view}"]');
    if (!button) return false;
    button.click();
    return true;
  })()`));
  const trace = () => page.evaluate('window.__telemetryLifetime');
  const summaryState = () => page.evaluate(`(()=>{
    const inspector=document.querySelector('[aria-label="Incoming game telemetry"]');
    const name=inspector?.querySelector('[aria-label="Game telemetry source"] h2')?.textContent;
    const selected=inspector?.querySelector('[aria-label="Telemetry game"]')?.selectedOptions[0]?.textContent;
    const error=inspector?.querySelector('[role="alert"]')?.textContent;
    return {name,selected,error};
  })()`);
  const currentSummary = async () => {
    try {
      await until(summaryState, ({name,selected,error}) => !!name&&selected?.startsWith(name)&&!error);
    } catch (cause) {
      throw new Error(`Telemetry summary did not settle: ${JSON.stringify(await summaryState())}`, {cause});
    }
  };
  let breakpoint;
  let hiddenOverride = false;
  const setHidden = async hidden => {
    hiddenOverride = hidden;
    await page.evaluate(`(() => {
      if (${hidden}) Object.defineProperty(document,'hidden',{configurable:true,value:true});
      else delete document.hidden;
      document.dispatchEvent(new Event('visibilitychange'));
    })()`);
  };
  try {
    await navigate('Overview');
    await seedGames();
    await until(() => page.evaluate(`import('/src/lib/games.ts').then(({getGameCatalogue})=>getGameCatalogue().filter(game=>game.provider).length>=2)`));
    await page.evaluate('window.__telemetryLifetime={opens:0,reads:[],closed:[]}');
    if (browserUrl) {
      await page.evaluate(`(() => {
        const trace=window.__telemetryLifetime, tokens=new Map(), original=window.fetch;
        trace.restore=()=>{window.fetch=original;};
        window.fetch=async function(input, init) {
          const path=new URL(input instanceof Request?input.url:input,location.href).pathname;
          const relevant=path==='/api/gateway/'||path.startsWith('/api/observe/');
          const body=relevant&&typeof init?.body==='string'?JSON.parse(init.body):null;
          if(path==='/api/gateway/'&&body?.command==='game_telemetry_inspect')trace.reads.push(body);
          const response=await original.call(this,input,init);
          if(response.ok&&path==='/api/observe/subscribe'&&body?.topic==='games.telemetry'){
            const {subscriptionId}=await response.clone().json();
            tokens.set(JSON.stringify([body.connectionId,body.requestId]),subscriptionId);
            trace.opens++;
          }
          if(response.ok&&path==='/api/observe/unsubscribe'&&body){
            const key=JSON.stringify([body.connectionId,body.requestId]);
            if(tokens.has(key)){trace.closed.push(tokens.get(key));tokens.delete(key);}
          }
          return response;
        };
      })()`);
    } else {
      await page.call('Debugger.enable');
      const {result}=await page.call('Runtime.evaluate', {expression:'window.__TAURI_INTERNALS__.invoke'});
      ({breakpointId:breakpoint}=await page.call('Debugger.setBreakpointOnFunctionCall', {
        objectId:result.objectId,
        condition:`(cmd==='observation_subscribe'&&payload?.topic==='games.telemetry'&&window.__telemetryLifetime.opens++,
          cmd==='observation_unsubscribe'&&window.__telemetryLifetime.closed.push(payload.subscriptionId),
          cmd==='gateway_request'&&payload?.request?.command==='game_telemetry_inspect'&&window.__telemetryLifetime.reads.push(payload.request),false)`,
      }));
    }
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
    await currentSummary();
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
    await currentSummary();
    const tab = name => page.evaluate(`Array.from(document.querySelectorAll('[aria-label="Debug sections"] [role="tab"]')).find(button=>button.textContent.trim()===${JSON.stringify(name)}).click()`);
    await tab('Profile inspection');
    const hidden=await until(trace,state=>state.closed.includes(observationId));
    await new Promise(resolve=>setTimeout(resolve,600));
    const settledHidden=await trace();
    assert.equal(settledHidden.opens,hidden.opens,'Hidden telemetry must not acquire another reader');
    assert.equal(settledHidden.reads.length,hidden.reads.length,'Hidden telemetry must not issue inspection reads');
    await tab('Incoming telemetry');
    const reopened=await until(trace,state=>state.opens===2&&state.reads.some(read=>read.observationId!==observationId));
    const freshId=reopened.reads.at(-1).observationId;
    assert.ok(freshId);
    assert.notEqual(freshId,observationId);
    await currentSummary();
    await setHidden(true);
    const backgrounded=await until(trace,state=>state.closed.includes(freshId));
    await new Promise(resolve=>setTimeout(resolve,600));
    const backgroundTrace=await trace();
    assert.equal(backgroundTrace.opens,backgrounded.opens);
    assert.equal(backgroundTrace.reads.length,backgrounded.reads.length);
    await setHidden(false);
    const foregrounded=await until(trace,state=>state.opens===3&&
      state.reads.some(read=>read.observationId!==observationId&&read.observationId!==freshId));
    const foregroundId=foregrounded.reads.at(-1).observationId;
    await currentSummary();
    await navigate('Overview');
    await until(trace,state=>state.closed.includes(foregroundId));
  } finally {
    try {
      if (breakpoint) await page.call('Debugger.removeBreakpoint', {breakpointId:breakpoint});
      if (!browserUrl) await page.call('Debugger.disable');
      if (hiddenOverride) await setHidden(false);
      await page.evaluate('window.__telemetryLifetime?.restore?.(); delete window.__telemetryLifetime');
    } finally { page.close(); }
  }
});
