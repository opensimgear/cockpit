// Run against a disposable, already-running Windows Cockpit. The children are
// authored Windows processes. No simulator, hardware, or privilege change.
import assert from 'node:assert/strict';
import {execFile, spawn} from 'node:child_process';
import {once} from 'node:events';
import {copyFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {promisify} from 'node:util';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_GAME_PROCESS_BOUNDARY_BROWSER_URL;
const transport = browserUrl ? 'browser-dev' : 'native';
const gatewayPath = browserUrl ? '/api/gateway/' : '/gateway_request';
const enabled = process.platform === 'win32' && (browserUrl || process.env.OSG_GAME_PROCESS_BOUNDARY_NATIVE === '1');
const qualifyPreference = process.env.OSG_GAME_PROCESS_PREFERENCE_ACCEPTANCE === '1';
const execute = promisify(execFile);
const binding = '__osgProcessBoundaryReport';

test(`${transport} Windows game process boundaries reach the Active game header and the shared read model`, {
  skip:!enabled, timeout:120000,
}, async () => {
  const root = resolve('.research/reactive/process-boundary', String(Date.now()));
  const registered = resolve(root, 'registered'), decoy = resolve(root, 'different-path');
  const executable = resolve(registered, 'OsgProcessBoundaryFixture.exe');
  const wrongExecutable = resolve(decoy, 'OsgProcessBoundaryFixture.exe');
  const replacement = resolve(registered, 'replacement', 'OsgProcessBoundaryFixture.exe');
  await mkdir(registered, {recursive:true});
  await mkdir(decoy, {recursive:true});
  await mkdir(resolve(registered, 'replacement'), {recursive:true});
  await execute('powershell.exe', ['-NoProfile','-NonInteractive','-Command',
    "$ErrorActionPreference='Stop'; Add-Type -TypeDefinition 'public static class OsgProcessBoundaryFixture { public static void Main() { System.Threading.Thread.Sleep(System.Threading.Timeout.Infinite); } }' -OutputAssembly $env:OSG_PROCESS_BOUNDARY_FIXTURE -OutputType WindowsApplication"],
  {windowsHide:true,env:{...process.env,OSG_PROCESS_BOUNDARY_FIXTURE:executable}});
  await copyFile(executable, wrongExecutable);
  await copyFile(executable, replacement);

  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const name = 'OSG process boundary fixture';
  let child, gameId, tracking = false, dispose = () => {};
  const transitions = {stream:[],header:[]}, reads = [];
  const request = value => page.evaluate(`import('/src/lib/gateway.ts')
    .then(({gatewayRequest}) => gatewayRequest(${JSON.stringify(value)}))`);
  const processPreference = async () => (await request({command:'ui_preferences_get',key:'game-process-events'})).uiPreferences;
  const navigate = view => until(() => page.evaluate(`(() => {
    const label = ${JSON.stringify(view)};
    const button = [...document.querySelectorAll('nav[aria-label="Main navigation"] button, button.sidebar-link')]
      .find(node => node.title === label || node.textContent.trim() === label);
    if (!button) return false;
    button.click();
    return true;
  })()`));
  const start = async path => {
    child = spawn(path, {windowsHide:true,stdio:'ignore'});
    await once(child,'spawn');
  };
  const stop = async () => {
    if (!child) return;
    if (child.exitCode === null && child.signalCode === null) {
      const exited = once(child,'exit');
      child.kill();
      await exited;
    }
    child = undefined;
  };
  const times = [];
  const boundary = async (running, change) => {
    const next = transitions.stream.length + 1, at = performance.now();
    await change();
    await until(() => transitions.stream.length >= next && transitions.header.length >= next,
      Boolean, 5000);
    const stream = transitions.stream[next-1], header = transitions.header[next-1];
    assert.equal(stream.running,running);
    assert.equal(header.running,running);
    return {running,streamMs:stream.at-at,headerMs:header.at-at};
  };
  try {
    await until(() => page.evaluate(`Boolean(document.querySelector('.header-statuses'))`));
    const {games} = await request({command:'game_add',name,installPath:registered,executable});
    gameId = games.addedGameId;
    assert.ok(gameId);
    assert.equal(games.catalogue.find(game => game.id === gameId)?.name,name);
    if (qualifyPreference) {
      assert.equal((await processPreference()).value,false,'Use an isolated Cockpit with the default preference');
      await navigate('Settings');
      const selector = JSON.stringify('[role="switch"][aria-label="Use Windows process events"]');
      for (const enabled of [true,false]) {
        await until(() => page.evaluate(`(() => {
          const control = document.querySelector(${selector});
          if (!control || control.disabled) return false;
          control.click();
          return true;
        })()`));
        await until(() => page.evaluate(`document.querySelector(${selector})?.getAttribute('aria-checked') === "${enabled}"`));
        assert.equal((await processPreference()).value,enabled);
      }
      await navigate('Overview');
    }
    await page.call('Runtime.enable');
    await page.call('Runtime.addBinding',{name:binding});
    await page.call('Network.enable',{maxPostDataSize:65536});
    dispose = page.listen(({method,params}) => {
      if (method === 'Runtime.bindingCalled' && params.name === binding) {
        const {surface,running} = JSON.parse(params.payload);
        transitions[surface].push({running,at:performance.now()});
      }
      if (tracking && method === 'Network.requestWillBeSent' && params.request.postData
          && new URL(params.request.url).pathname === gatewayPath) {
        const body = JSON.parse(params.request.postData);
        const command = (body.request ?? body).command;
        if (command === 'game_get_state' || command === 'game_read_model_page') reads.push(command);
      }
    });
    await page.evaluate(`(async()=>{
      const {observeGamesState}=await import('/src/lib/game-read-models/shared.ts');
      const trace=window.__osgProcessBoundary={};
      const emit=(surface,running)=>{
        if(trace[surface]===running)return;
        trace[surface]=running;
        window[${JSON.stringify(binding)}](JSON.stringify({surface,running}));
      };
      const gameId=${JSON.stringify(gameId)}, name=${JSON.stringify(name)};
      const header=document.querySelector('.header-statuses');
      const update=()=>{
        const label=header.querySelector('.header-status .badge-copy strong')?.textContent??'';
        emit('header',label.split(', ').includes(name));
      };
      const observer=new MutationObserver(update);
      observer.observe(header,{subtree:true,childList:true,characterData:true});
      update();
      const stop=observeGamesState(state=>{
        if(state.status==='ready'&&state.value.catalogue.some(game=>game.id===gameId)){
          emit('stream',state.value.detectedGames.some(game=>game.gameId===gameId));
        }
      });
      trace.close=()=>{observer.disconnect();stop();};
    })()`);
    await until(() => transitions.header.length === 1 && transitions.stream.length === 1);
    assert.equal(transitions.header[0].running,false);
    assert.equal(transitions.stream[0].running,false);
    tracking = true;
    await request({command:'game_get_state'}); // Positive control for the CDP trace.
    assert.ok(reads.includes('game_get_state'),'CDP did not capture the control request');
    reads.length = 0;
    await start(wrongExecutable);
    await delay(1400);
    await stop();
    await delay(650);
    assert.deepEqual(transitions.stream.map(item=>item.running),[false]);
    assert.deepEqual(transitions.header.map(item=>item.running),[false]);

    for (let round = 0; round < 8; round++) {
      for (const [running,change] of [[true,()=>start(executable)],[false,stop]]) {
        times.push(await boundary(running, change));
        await delay(130 + round * 13);
      }
    }
    assert.equal(transitions.stream.length,17,'Expected one visible change per authored boundary');

    await boundary(true, () => start(executable));
    // A catalogue commit may legitimately load a changed immutable page.
    // Keep the zero-query assertion scoped to process changes.
    tracking = false;
    const replacementBoundary = await boundary(false, () => request({command:'game_save',gameId,executable:replacement}));
    await delay(1100);
    assert.equal(child.exitCode, null, 'The previous executable must still be running when its identity is withdrawn');
    assert.equal(child.signalCode, null);
    assert.equal(transitions.stream.length,19);
    tracking = true;
    await stop();
    await delay(650);
    assert.equal(transitions.stream.length,19, 'The retired executable exit must leave the new registration quiet');
    await boundary(true, () => start(replacement));
    await boundary(false, stop);

    assert.deepEqual(transitions.stream.map(item=>item.running),transitions.header.map(item=>item.running));
    assert.equal(transitions.stream.length,21,'Expected only the authored process and catalogue boundaries');
    assert.deepEqual(reads,[],'Process transitions must arrive without repeated game state or page queries');
    const evidence = {transport,wrongPathIgnored:true,preferenceToggle:qualifyPreference,reads,times,replacementBoundary,headerTransitions:20,streamTransitions:20};
    await writeFile(resolve(root,'evidence.json'), JSON.stringify(evidence,null,2) + '\n');
    console.log(JSON.stringify(evidence));
  } finally {
    dispose();
    try {
      await stop();
      await page.evaluate('window.__osgProcessBoundary?.close(); delete window.__osgProcessBoundary');
      if (gameId) await request({command:'game_remove',gameId});
      if (qualifyPreference) {
        const preference = await processPreference();
        if (preference.value) await request({command:'ui_preferences_save',key:'game-process-events',
          expectedRevision:preference.revision,value:false});
      }
    } finally { page.close(); }
  }
});
