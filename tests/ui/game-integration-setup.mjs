// Frontend-only acceptance. Gateway replies below are authored fixtures;
// these assertions do not prove native installation, telemetry or hardware behavior.
// Start a private server first: TAURI_ENV_PLATFORM=ui-test pnpm ui:dev --port 5187
// OSG_PLAYWRIGHT_MODULE can point at an already installed Playwright package.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { readFile } from 'node:fs/promises';

const require = createRequire(import.meta.url);
const playwrightModule = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(playwrightModule);
const { expect } = require(playwrightModule + '/test');
const endpoint = process.env.OSG_SETUP_UI_URL ?? 'http://127.0.0.1:5187';
const artifacts = resolve(process.env.OSG_SETUP_UI_ARTIFACTS ?? '.research/setup-ui-acceptance');
const folder = String.raw`C:\Users\Pilot\Saved Games\DCS`;
const alternate = String.raw`D:\Profiles\Saved Games\DCS.openbeta`;

test('native picker uses the selected game, preserves paths on cancellation, and keeps X-Plane tiles distinct from ACR', {timeout:30000},async()=>{
  const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
  const page=await browser.newPage();const calls=[];let choice=null;let pickerError=false;
  const catalogue=[{id:'dcs',name:'DCS World',provider:'dcs-bios'},{id:'assetto-corsa-rally',name:'Assetto Corsa Rally',provider:'assetto-corsa-rally'},{id:'x-plane-11',name:'X-Plane 11',provider:'x-plane'},{id:'x-plane-12',name:'X-Plane 12',provider:'x-plane'}].map(g=>({...g,installed:true,launchSupported:true,executable:null}));
  const games={catalogue,profiles:[],defaultProfiles:{},session:{phase:'idle'}};
  await page.exposeFunction('__osgNativeInvoke',async(command,args)=>{
    calls.push({command,args});
    if(command==='pick_game_executable'){if(pickerError)throw new Error('Cannot open file picker');return choice;}
    if(command==='gateway_request'){
      const r=args.request;
      if(r.command==='game_installation'){const game=catalogue.find(g=>g.id===r.gameId);game.executable=r.executable;game.source=r.executable?'manual':'not-found';}
      return {ok:true,state:{running:false,needsReconnect:false,modules:[],ports:[],games,integration:{running:false},dcsBios:{running:false}}};
    }
    throw new Error(`Unexpected native command: ${command}`);
  });
  await page.addInitScript(()=>{window.isTauri=true;window.__TAURI_INTERNALS__={invoke:(command,args)=>window.__osgNativeInvoke(command,args)};});
  try{
    await page.goto(endpoint);const nav=page.getByRole('navigation',{name:'Main navigation'});await nav.getByRole('button',{name:'Games',exact:true}).click();
    for(const game of catalogue){
      const tile=page.getByRole('button',{name:`Open ${game.name} details`,exact:true});
      if(game.provider==='x-plane'){await expect(tile.locator('.game-wordmark')).toContainText('X-PLANE');await expect(tile.locator('.art-category')).toHaveText('Flight');}
      await tile.click();const path=page.getByLabel('Game executable',{exact:true});await expect(path).toHaveAttribute('readonly','');
      const filename=game.provider==='x-plane'?'X-Plane.exe':game.id==='dcs'?'DCS.exe':'acr.exe';await expect(path).toHaveAttribute('placeholder',`Choose ${filename}`);
      choice=`D:/Games/${game.name}/${filename}`;await page.getByRole('button',{name:'Browse…',exact:true}).click();await expect(path).toHaveValue(choice);
      const selected=calls.filter(c=>c.command==='pick_game_executable').at(-1);assert.equal(selected.args.gameId,game.id);
      const count=calls.filter(c=>c.command==='gateway_request'&&c.args.request.command==='game_installation').length;
      choice=null;await page.getByRole('button',{name:'Browse…',exact:true}).click();await expect(page.getByRole('button',{name:'Browse…',exact:true})).toBeEnabled();
      assert.equal(calls.filter(c=>c.command==='gateway_request'&&c.args.request.command==='game_installation').length,count);await expect(path).toHaveValue(game.executable);
      await page.getByRole('button',{name:'All games',exact:true}).click();
    }
    await page.getByRole('button',{name:'Open X-Plane 12 details',exact:true}).click();
    catalogue[3].installed=false;catalogue[3].executable=null;catalogue[3].source='manual';
    await expect(page.getByLabel('Game executable',{exact:true})).toHaveValue('');
    await page.getByRole('button',{name:'Use automatic discovery',exact:true}).click();
    assert.equal(catalogue[3].source,'not-found');await expect(page.getByRole('button',{name:'Use automatic discovery',exact:true})).toHaveCount(0);
    pickerError=true;await page.getByRole('button',{name:'Browse…',exact:true}).click();
    await expect(page.getByText(/Cannot open file picker/)).toBeVisible();await expect(page.getByRole('button',{name:'Browse…',exact:true})).toBeEnabled();
  }finally{await browser.close();}
});

test('frontend X-Plane setup, version-scoped profile capture and portable indicator/display editing', {timeout:60000}, async () => {
  await mkdir(artifacts,{recursive:true});
  const browser=await chromium.launch({headless:true, ...(process.env.OSG_BROWSER_CHANNEL ? {channel:process.env.OSG_BROWSER_CHANNEL} : {})});
  const page=await browser.newPage({viewport:{width:1440,height:1080}});
  const errors=[],requests=[];page.on('pageerror',e=>errors.push(e.message));
  const catalogue=[11,12].map(version=>({id:`x-plane-${version}`,name:`X-Plane ${version}`,provider:'x-plane',installed:true,launchSupported:true,executable:`C:/Games/X-Plane ${version}/X-Plane.exe`,source:'steam',installations:[{executable:`C:/Games/X-Plane ${version}/X-Plane.exe`,installPath:`C:/Games/X-Plane ${version}`,source:'steam'},{executable:`D:/Backup/X-Plane ${version}/X-Plane.exe`,installPath:`D:/Backup/X-Plane ${version}`,source:'standalone'}],integrationSetup:{state:'setup-required',message:'Set up RREF UDP for this installation.',live:'waiting',verified:false}}));
  const games={catalogue,profiles:[],defaultProfiles:{},session:{phase:'idle'}};
  const integration={configured:false,running:false,revision:1};
  await page.route('**/api/gateway/**',async route=>{
    const r=route.request().postDataJSON();requests.push(r);const game=catalogue.find(g=>g.id===r.gameId);
    if(r.command==='game_installation' && game) game.executable=r.executable;
    if(r.command==='game_integration_setup' && game) game.integrationSetup={state:'ready',message:'RREF UDP configured; live qualification pending.',live:'waiting',verified:false,connection:{source:'rref',version:Number(game.id.slice(-2)),host:'127.0.0.1',port:49000,listenPort:0,updateHz:10,freshnessMs:1500}};
    if(r.command==='integration_validate') integration.validation={valid:true,diagnostics:[],catalogueRevision:1};
    if(r.command==='integration_save') {integration.config=structuredClone(r.integrationConfig);integration.configured=true;integration.revision++;}
    if(r.command==='game_profile_save') {games.profiles=[r.gameProfile];games.defaultProfiles[r.gameProfile.gameId]=r.gameProfile.id;}
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({ok:true,state:{running:false,needsReconnect:false,modules:[],ports:[],games,integration,dcsBios:{running:false}}})});
  });
  try {
    await page.goto(endpoint);
    const nav=page.getByRole('navigation',{name:'Main navigation'});
    await nav.getByRole('button',{name:'Games',exact:true}).click();
    await page.getByRole('button',{name:'Open X-Plane 12 details',exact:true}).click();
    const panel=page.locator('.setup-panel');
    await expect(page.getByLabel('Detected X-Plane installations')).toBeVisible();
    await page.getByLabel('Detected X-Plane installations').selectOption('D:/Backup/X-Plane 12/X-Plane.exe');
    await page.getByRole('button',{name:'Use this path',exact:true}).click();
    await panel.getByRole('button',{name:'Set up telemetry',exact:true}).click();
    await expect(panel.getByText('Ready for integration',{exact:true})).toBeVisible();
    await expect(panel.getByText('Waiting for game data',{exact:true})).toBeVisible();
    await expect(page.getByLabel('DCS Saved Games folder')).toHaveCount(0);
    await page.screenshot({path:resolve(artifacts,'xplane-ready-desktop.png'),fullPage:true});
    await nav.getByRole('button',{name:'Integrations',exact:true}).click();
    const editor=page.locator('section[aria-label="Simulator integrations"]');
    await editor.getByLabel(/^Provider/).selectOption('x-plane');
    await editor.getByLabel(/^Simulator version/).selectOption('12');
    await editor.getByRole('button',{name:'Use prepared connection settings'}).click();
    await editor.getByLabel('Simulator UDP receive port',{exact:true}).fill('49005');
    await editor.getByText('Import or export portable profile JSON',{exact:true}).click();
    await editor.getByLabel('Portable document JSON',{exact:true}).fill(await readFile(resolve('integration-profiles/x-plane-c172-panel.json'),'utf8'));
    await editor.getByRole('button',{name:'Preview import',exact:true}).click();
    await expect(editor.getByLabel('Simulator UDP receive port',{exact:true})).toHaveValue('49005');
    await expect(editor.getByRole('heading',{name:'Outputs',exact:true})).toBeVisible();
    await editor.getByRole('button',{name:'Validate draft',exact:true}).click();
    await editor.getByRole('button',{name:'Save validated profile',exact:true}).click();
    await expect(editor.getByRole('button',{name:'Start integration',exact:true})).toBeDisabled();
    await page.screenshot({path:resolve(artifacts,'xplane-integration-desktop.png'),fullPage:true});
    await nav.getByRole('button',{name:'Games',exact:true}).click();
    await page.getByRole('button',{name:'Open X-Plane 12 details',exact:true}).click();
    await page.getByRole('button',{name:'Use saved integration',exact:true}).click();
    await page.getByRole('button',{name:'Save profile',exact:true}).click();
    const saved=requests.find(r=>r.command==='game_profile_save');
    assert.equal(saved.gameProfile.gameId,'x-plane-12');assert.equal(saved.gameProfile.integration.connection.version,12);assert.equal(saved.gameProfile.integration.connection.port,49005);
    assert.equal(saved.gameProfile.integration.document.outputs.length,2);
    for(const width of [768,390]) {await page.setViewportSize({width,height:1024});await page.screenshot({path:resolve(artifacts,`xplane-ready-${width}px.png`),fullPage:true});assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));}
    assert.equal(requests.some(r=>r.command==='integration_start'||r.command==='game_launch'),false);assert.deepEqual(errors,[]);
  } catch(error) {console.log(await page.locator('main').innerText());throw error;} finally {await browser.close();}
});

test('frontend DCS and ACR setup actions, readiness, native verification and responsive layout', { timeout: 90_000 }, async () => {
  await mkdir(artifacts, { recursive: true });
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1080 } });
  const page = await context.newPage();
  const errors = [], requests = [];
  page.on('pageerror', error => errors.push(error.message));
  let acrVerify = false;
  const dcs = { id: 'dcs', name: 'DCS World', provider: 'dcs-bios', installed: true, launchSupported: true, executable: String.raw`C:\Games\DCS World\bin\DCS.exe`, source: 'Standalone', integrationSetup: { state: 'setup-required', message: 'Choose your DCS Saved Games folder.', candidates: [folder, alternate], version: '0.11.7', live: 'waiting' } };
  const acr = { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: true, launchSupported: true, executable: String.raw`C:\Games\Assetto Corsa Rally\acr.exe`, source: 'Steam', integrationSetup: { state: 'setup-required', message: 'Configure native telemetry to connect your hardware.', live: 'waiting', verified: false } };
  const games = { catalogue: [dcs, acr], profiles: [], defaultProfiles: {}, session: { phase: 'idle' } };
  const integration = { configured: false, running: false };
  const dcsBios = { configured: false, running: false };
  await context.route('**/api/gateway/**', async route => {
    const request = route.request().postDataJSON();
    requests.push(request);
    const game = request.gameId === 'dcs' ? dcs : acr;
    if (request.command === 'game_integration_setup') {
      if (game === dcs) dcs.integrationSetup = { ...dcs.integrationSetup, userFolder: request.userFolder, state: 'setting-up', message: 'Installing DCS-BIOS.', progress: 'Verifying downloaded DCS-BIOS files.' };
      else acr.integrationSetup = { state: 'needs-attention', message: 'Configured; native telemetry not yet verified.', connection: { source: 'shared-memory', freshnessMs: 1000 }, verified: false, live: 'waiting' };
    } else if (request.command === 'game_integration_cancel') {
      game.integrationSetup = { ...game.integrationSetup, state: 'setup-required', message: 'Setup cancelled; previous files restored.' };
      delete game.integrationSetup.progress;
    } else if (request.command === 'game_integration_recover') {
      game.integrationSetup = { ...game.integrationSetup, state: 'setup-required', recoveryNeeded: false, message: 'Previous setup restored. You can try again.' };
    } else if (request.command === 'game_integration_check' && game === acr && acrVerify) {
      acr.integrationSetup = { state: 'ready', message: 'Native telemetry is compatible with this game build.', version: '1.2.3', verified: true, connection: { source: 'shared-memory', freshnessMs: 1000 }, live: 'waiting', optionalMaxRpm: false, probeChannels: { rpm: { status: 'available' }, speed: { status: 'available' }, gear: { status: 'available' }, max_rpm: { status: 'unknown' } } };
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ok: true, state: { running: false, needsReconnect: false, modules: [], ports: [], games, integration, dcsBios } }) });
  });
  async function screenshot(name) { await page.screenshot({ path: resolve(artifacts, name), fullPage: true }); }
  async function fitsViewport() {
    const dimensions = await page.evaluate(() => ({ viewport: innerWidth, content: document.documentElement.scrollWidth }));
    assert.ok(dimensions.content <= dimensions.viewport + 1, `Horizontal overflow: ${JSON.stringify(dimensions)}`);
    const panel = page.locator('.setup-panel');
    const bounds = await panel.boundingBox();
    assert.ok(bounds && bounds.x >= 0 && bounds.x + bounds.width <= dimensions.viewport + 1, 'Setup panel fits viewport');
  }
  try {
    await page.goto(endpoint);
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Games', exact: true }).click();
    await page.getByRole('button', { name: 'Open DCS World details', exact: true }).click();
    const panel = page.locator('.setup-panel');
    const setup = panel.getByRole('button', { name: 'Set up integration', exact: true });
    await expect(setup).toBeDisabled();
    await page.getByLabel('Detected DCS Saved Games folders', { exact: true }).selectOption(alternate);
    await expect(setup).toBeEnabled();
    await page.getByLabel('DCS Saved Games folder', { exact: true }).fill('relative/DCS');
    await expect(setup).toBeDisabled();
    await expect(panel.getByText('Enter an absolute folder path.', { exact: true })).toBeVisible();
    await page.getByLabel('DCS Saved Games folder', { exact: true }).fill(folder);
    await setup.click();
    await expect(panel.getByText('Verifying downloaded DCS-BIOS files.', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Launch', exact: true })).toBeDisabled();
    await screenshot('dcs-progress-desktop.png');
    assert.equal(requests.find(request => request.command === 'game_integration_setup').userFolder, folder);
    await panel.getByRole('button', { name: 'Cancel setup', exact: true }).click();
    await expect(panel.getByText('Setup cancelled; previous files restored.', { exact: true })).toBeVisible();
    dcs.integrationSetup = { ...dcs.integrationSetup, state: 'needs-attention', recoveryNeeded: true, message: 'Interrupted setup requires recovery.' };
    await expect(panel.getByRole('button', { name: 'Recover integration setup', exact: true })).toBeVisible();
    await screenshot('dcs-recovery-desktop.png');
    await panel.getByRole('button', { name: 'Recover integration setup', exact: true }).click();
    await expect(panel.getByText('Previous setup restored. You can try again.', { exact: true })).toBeVisible();
    await panel.getByRole('button', { name: 'Set up integration', exact: true }).click();
    dcs.integrationSetup = { ...dcs.integrationSetup, state: 'ready', message: 'DCS-BIOS export and catalogue are configured.', cataloguePath: String.raw`C:\Users\Pilot\Saved Games\DCS\Scripts\DCS-BIOS\doc\json`, connection: { receivePort: 5010 }, live: 'waiting' };
    delete dcs.integrationSetup.progress;
    await expect(panel.getByText('Ready for integration', { exact: true })).toBeVisible();
    await expect(panel.getByText('Waiting for game data', { exact: true })).toBeVisible();
    await fitsViewport();
    await screenshot('dcs-ready-desktop.png');
    for (const width of [768, 390]) {
      await page.setViewportSize({ width, height: 1024 });
      await fitsViewport();
      await screenshot(`dcs-ready-${width}px.png`);
    }
    await page.setViewportSize({ width: 1440, height: 1080 });
    integration.running = true;
    await expect(panel.getByRole('button', { name: 'Repair integration', exact: true })).toBeDisabled();
    await expect(panel.getByText('Stop the active integration before setting up integration.', { exact: true })).toBeVisible();
    integration.running = false;
    await expect(panel.getByRole('button', { name: 'Repair integration', exact: true })).toBeEnabled();
    await page.getByRole('button', { name: 'All games', exact: true }).click();
    await page.getByRole('button', { name: 'Open Assetto Corsa Rally details', exact: true }).click();
    await expect(page.getByLabel('DCS Saved Games folder', { exact: true })).toHaveCount(0);
    await panel.getByRole('button', { name: 'Set up telemetry', exact: true }).click();
    const acrSetup = requests.find(request => request.command === 'game_integration_setup' && request.gameId === acr.id);
    assert.ok(acrSetup);
    assert.equal('userFolder' in acrSetup, false);
    assert.equal('processId' in acrSetup, false);
    await expect(panel.getByText('Telemetry configured · verification needed', { exact: true })).toBeVisible();
    await expect(panel.getByText('Native telemetry not verified', { exact: true })).toBeVisible();
    await screenshot('acr-configured-unverified-desktop.png');
    acrVerify = true;
    await panel.getByRole('button', { name: 'Recheck readiness', exact: true }).click();
    await expect(panel.getByText('Native telemetry verified', { exact: true })).toBeVisible();
    await expect(panel.getByText('Ready for integration', { exact: true })).toBeVisible();
    await expect(panel.getByText('Waiting for game data', { exact: true })).toBeVisible();
    await panel.getByText('Last native check', { exact: true }).click();
    await expect(panel.getByText('available', { exact: true })).toHaveCount(3);
    await expect(panel.getByText('unknown', { exact: true })).toBeVisible();
    await expect(panel.getByText('Telemetry channels', { exact: true })).toHaveCount(0);
    await expect(panel.getByText('Maximum RPM is unavailable in this game build. Other supported telemetry can still work.', { exact: true })).toBeVisible();
    acr.integrationSetup.live = 'receiving';
    acr.integrationSetup.channels = { rpm: { status: 'fresh', value: 4000 }, speed: { status: 'fresh', value: 31.2 }, gear: { status: 'fresh', value: 3 }, max_rpm: { status: 'unsupported' } };
    await expect(panel.getByText('Receiving game data', { exact: true })).toBeVisible();
    await panel.getByText('Telemetry channels', { exact: true }).click();
    await screenshot('acr-native-verified-desktop.png');
    for (const width of [768, 390]) {
      await page.setViewportSize({ width, height: 1024 });
      await fitsViewport();
      await screenshot(`acr-native-verified-${width}px.png`);
    }
    acr.integrationSetup = { ...acr.integrationSetup, connection: { source: 'synthetic' }, live: 'receiving', verified: true };
    await expect(panel.getByText('Native telemetry verified', { exact: true })).toHaveCount(0);
    await expect(panel.getByText('Receiving synthetic test data', { exact: true })).toBeVisible();
    await expect(panel.locator('.setup-readiness.ready')).toHaveCount(0);
    await expect(panel.getByText('Synthetic test telemetry configured', { exact: true })).toBeVisible();
    await expect(panel.getByText('Native telemetry is compatible with this game build.', { exact: true })).toHaveCount(0);
    await expect(panel.getByText('Telemetry channels', { exact: true })).toHaveCount(0);
    await expect(panel.getByText('Last native check', { exact: true })).toHaveCount(0);
    await screenshot('acr-synthetic-mobile.png');
    await page.getByRole('button', { name: 'All games', exact: true }).click();
    await expect(page.locator('.tile-verification.verified')).toHaveCount(0);
    await expect(page.locator('.game-tile.rally').getByText('Receiving synthetic test data', { exact: true })).toBeVisible();
    await page.setViewportSize({ width: 1440, height: 1080 });
    await screenshot('game-tiles-desktop.png');
    assert.deepEqual(errors, [], `Browser runtime errors: ${errors.join('\n')}`);
    console.log(`Frontend-only mocked setup acceptance passed. Screenshots: ${artifacts}`);
  } finally {
    await context.close();
    await browser.close();
  }
});

test('frontend clean closed-game ACR setup creates, validates, saves and captures a native profile with deferred PID', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ headless: true, ...(process.env.OSG_BROWSER_CHANNEL ? { channel: process.env.OSG_BROWSER_CHANNEL } : {}) });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1080 } });
  const page = await context.newPage();
  const errors = [], requests = [];
  page.on('pageerror', error => errors.push(error.message));
  const serial = '111111111111111111111111', gatewayId = 'stm32:' + serial, moduleId = '00112233445566778899aabbccddeeff';
  const port = { path: 'COM77', backend: 'stm32', manufacturer: 'OpenSimGear', friendlyName: 'OpenSimGear gateway', serialNumber: serial };
  const module = { id: moduleId, name: 'Rally LEDs', kind: 'panel', online: true, registered: true, active: true, enabled: false, published: false, outputOnly: true, buttonCount: 0, axisCount: 0, buttons: [], axes: [], boot: 9, serialNumber: 'LED-PANEL', settings: { label: 'Rally LEDs', debounce: 5, inverted: false } };
  const actions = { status: 'ready', boot: 9, catalogueRevision: 1, states: [{ id: 'rgb.set', values: ['000000'] }], actions: [{ id: 'rgb.set', endpoint: 'rgb', label: 'Rev light color', semantics: 'setter', readable: true, parameters: [{ id: 'color', label: 'Color', type: 'rgb' }] }] };
  const catalogue = { provider: 'assetto-corsa-rally', revision: 1, total: 1, commands: [], channels: [{ id: 'rev_ratio', label: 'Rev ratio', type: 'number', derived: true }] };
  const acr = { id: 'assetto-corsa-rally', name: 'Assetto Corsa Rally', provider: 'assetto-corsa-rally', installed: true, launchSupported: true, executable: String.raw`C:\Games\ACR\acr.exe`, integrationSetup: { state: 'setup-required', message: 'Native telemetry setup is required.', live: 'waiting', verified: false } };
  const games = { catalogue: [acr], profiles: [], defaultProfiles: {}, session: { phase: 'idle' } };
  const integration = { configured: false, running: false, revision: 1 };
  const state = { gatewayId, backend: 'stm32', connected: true, running: true, needsReconnect: false, layoutPending: false, ports: [port], modules: [module], moduleActions: { [moduleId]: actions }, games, integration, dcsBios: { running: false, configured: false } };
  await context.route('**/api/gateway/**', async route => {
    const request = route.request().postDataJSON();
    requests.push(request);
    if (request.command === 'game_integration_setup') acr.integrationSetup = { state: 'needs-attention', message: 'Configured; native telemetry not yet verified.', connection: { source: 'shared-memory', freshnessMs: 1800 }, live: 'waiting', verified: false };
    else if (request.command === 'integration_catalogue') integration.catalogue = catalogue;
    else if (request.command === 'integration_validate') {
      assert.equal(request.integrationConfig.connection.processId == null, true, 'Authored validation fixture receives deferred PID');
      integration.validation = { valid: true, diagnostics: [], revision: integration.revision, catalogueRevision: 1 };
    } else if (request.command === 'integration_save') {
      integration.config = structuredClone(request.integrationConfig);
      integration.configured = true;
      integration.revision++;
      delete integration.validation;
    } else if (request.command === 'game_profile_save') {
      const profile = structuredClone(request.gameProfile);
      games.profiles = [profile];
      games.defaultProfiles[acr.id] = profile.id;
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ok: true, state }) });
  });
  try {
    await page.goto(endpoint);
    const nav = page.getByRole('navigation', { name: 'Main navigation' });
    await nav.getByRole('button', { name: 'Games', exact: true }).click();
    await page.getByRole('button', { name: 'Open Assetto Corsa Rally details', exact: true }).click();
    await page.locator('.setup-panel').getByRole('button', { name: 'Set up telemetry', exact: true }).click();
    await expect(page.locator('.setup-panel').getByText('Telemetry configured · verification needed', { exact: true })).toBeVisible();
    await nav.getByRole('button', { name: 'Integrations', exact: true }).click();
    const editor = page.locator('section[aria-label="Simulator integrations"]');
    await editor.getByLabel(/^Provider/).selectOption('assetto-corsa-rally');
    await expect(editor.getByRole('button', { name: 'Use prepared connection settings', exact: true })).toBeEnabled();
    await expect(editor.getByLabel('Freshness timeout (ms)', { exact: true })).toHaveValue('1800');
    await editor.getByLabel('Profile name', { exact: true }).fill('Closed-game ACR lights');
    await editor.getByLabel('Windows game process ID', { exact: true }).fill('0');
    await editor.getByRole('button', { name: 'Use prepared connection settings', exact: true }).click();
    await expect(editor.getByLabel('Windows game process ID', { exact: true })).toHaveValue('');
    await expect(editor.getByText('Games selects its actual process when launched; manual Start needs a running PID.', { exact: true })).toBeVisible();
    await editor.getByLabel(/^Assigned device/).selectOption(gatewayId + '/' + moduleId);
    await editor.getByRole('button', { name: 'Load provider catalogue', exact: true }).click();
    await editor.getByRole('button', { name: 'Add rev lights', exact: true }).click();
    await expect(editor.getByRole('article', { name: 'Output binding rev-light-1', exact: true })).toBeVisible();
    await expect(editor.getByRole('button', { name: 'Validate draft', exact: true })).toBeEnabled();
    await editor.getByRole('button', { name: 'Validate draft', exact: true }).click();
    await editor.getByRole('button', { name: 'Save validated profile', exact: true }).click();
    await expect(editor.getByText('Profile saved. Validate the saved settings before starting.', { exact: true })).toBeVisible();
    assert.equal(integration.config.connection.source, 'shared-memory');
    assert.equal('processId' in integration.config.connection, false);
    await editor.getByRole('button', { name: 'Validate draft', exact: true }).click();
    await expect(editor.getByRole('button', { name: 'Start integration', exact: true })).toBeDisabled();
    await expect(editor.getByText('Launch this integration from Games, or select a running ACR process for a manual session.', { exact: true })).toBeVisible();
    assert.equal(requests.some(request => request.command === 'integration_start'), false, 'The UI never sends manual Start with a deferred PID');
    await page.screenshot({ path: resolve(artifacts, 'acr-closed-integration-saved-desktop.png'), fullPage: true });
    await nav.getByRole('button', { name: 'Games', exact: true }).click();
    await page.getByRole('button', { name: 'Open Assetto Corsa Rally details', exact: true }).click();
    await page.getByRole('button', { name: 'Use saved integration', exact: true }).click();
    await page.getByRole('button', { name: 'Save profile', exact: true }).click();
    const captured = requests.find(request => request.command === 'game_profile_save').gameProfile;
    assert.equal(captured.integration.document.name, 'Closed-game ACR lights');
    assert.equal(captured.integration.connection.source, 'shared-memory');
    assert.equal('processId' in captured.integration.connection, false);
    await expect(page.locator('.profile-editor').getByRole('button', { name: 'Activate profile', exact: true })).toBeEnabled();
    await expect(page.locator('.setup-panel').getByText('Telemetry configured · verification needed', { exact: true })).toBeVisible();
    await page.screenshot({ path: resolve(artifacts, 'acr-closed-game-profile-captured-desktop.png'), fullPage: true });
    assert.deepEqual(errors, []);
    console.log('Frontend-only mocked closed-game ACR profile Save/capture acceptance passed.');
  } catch (error) {
    await page.screenshot({ path: resolve(artifacts, 'acr-closed-flow-failure.png'), fullPage: true });
    console.log('Frontend fixture failure page:', await page.locator('main').innerText());
    console.log('Browser errors:', errors);
    throw error;
  } finally {
    await context.close();
    await browser.close();
  }
});
