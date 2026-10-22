import assert from 'node:assert/strict';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import test from 'node:test';
import { canonical, importBundle, importWidgetBundle, publishDocument } from '../../packages/companion-designer/src/index.ts';

// Uses the actual dev Cockpit WebView and normal UI handlers. No state injection.
test('reusable widgets import, edit, preview and persist in the native Cockpit', {
  skip: process.env.OSG_WIDGETS_NATIVE_UI !== '1', timeout: 120000,
}, async () => {
  const origin = process.env.OSG_WIDGETS_ORIGIN ?? 'http://127.0.0.1:1422';
  const targets = await (await fetch(`${process.env.OSG_WIDGETS_CDP ?? 'http://127.0.0.1:9232'}/json/list`, { signal: AbortSignal.timeout(5000) })).json();
  const target = targets.find(t => t.type === 'page' && new URL(t.url).origin === origin);
  assert.ok(target, 'Start the isolated native widget Cockpit first');
  const socket = new WebSocket(target.webSocketDebuggerUrl), pending = new Map(); let sequence = 0;
  await new Promise((yes, no) => { socket.addEventListener('open', yes, { once: true }); socket.addEventListener('error', no, { once: true }); });
  socket.addEventListener('message', event => { const msg = JSON.parse(event.data), item = pending.get(msg.id); if (!item) return; clearTimeout(item.timer); pending.delete(msg.id); msg.error ? item.reject(Error(msg.error.message)) : item.resolve(msg.result); });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++sequence, timer = setTimeout(() => { pending.delete(id); reject(Error(method + ' timed out')); }, 15000); pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => { const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text); return r.result.value; };
  const pause = ms => new Promise(r => setTimeout(r, ms));
  const until = async (expression, label) => { for (let i = 0; i < 100; i++) { const value = await evaluate(expression); if (value) return value; await pause(100); } throw Error(label); };
  const click = label => until(`(()=>{const b=[...document.querySelectorAll('button')].find(b=>(b.getAttribute('aria-label')??b.textContent.trim())===${JSON.stringify(label)});if(!b||b.disabled)return false;b.click();return true;})()`, 'Missing enabled button ' + label);
  const change = (selector, value) => evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});if(!e)throw Error('Missing field '+${JSON.stringify(selector)});e.value=${JSON.stringify(value)};e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));})()`);
  const run = Date.now(), directory = '.research/widgets-native-ui/' + run, checks = [], panelName = 'Shared systems ' + run;
  const bundlePath=process.env.OSG_WIDGETS_BUNDLE??'examples/companion/shared-systems.bundle.json';
  const source = JSON.parse(await readFile(bundlePath, 'utf8')),widgetCount=source.document.presentation.widgets.length;
  const layerCount = source.document.presentation.widgets.reduce((sum, widget) => sum + source.document.presentation.definitions.find(d => d.id === widget.config.definitionId && d.version === widget.config.definitionVersion).layers.length, 0);
  await mkdir(directory, { recursive: true });
  const screenshot = async name => { const r = await send('Page.captureScreenshot', { format: 'png' }); await writeFile(`${directory}/${name}.png`, Buffer.from(r.data, 'base64')); };
  const download = async (label, folder) => {
    const path = resolve(directory, folder); await mkdir(path, { recursive: true });
    await send('Browser.setDownloadBehavior', { behavior: 'allow', downloadPath: path });
    await click(label);
    for (let attempt = 0; attempt < 100; attempt++) {
      const names = (await readdir(path)).filter(name => name.endsWith('.json'));
      if (names.length === 1) { const file = resolve(path, names[0]); try { const text = await readFile(file, 'utf8'); JSON.parse(text); return { file, text }; } catch {} }
      await pause(100);
    }
    throw Error(label + ' did not download a complete JSON bundle');
  };
  const importFile = async (selector, file) => {
    const { root } = await send('DOM.getDocument');
    const { nodeId } = await send('DOM.querySelector', { nodeId: root.nodeId, selector });
    assert.ok(nodeId, 'Missing import control ' + selector);
    await send('DOM.setFileInputFiles', { nodeId, files: [resolve(file)] });
  };
  try {
    await send('Page.enable');
    await send('Page.setInterceptFileChooserDialog', { enabled: true });
    await send('Page.reload', { ignoreCache: true });
    await until(`!!window.__TAURI_INTERNALS__&&!!document.querySelector('nav[aria-label="Main navigation"]')`, 'Native Cockpit did not load');
    await click('Companions');
    await until(`!!document.querySelector('.panel-import:not(:disabled),.back-panels:not(:disabled)')`, 'Companions did not finish loading');
    if (await evaluate(`!!document.querySelector('.back-panels')`)) await click('Back to Panels');
    await until(`!!document.querySelector('input.panel-import:not(:disabled)')`, 'Panel library not ready');
    await importFile('input.panel-import', bundlePath);
    await until(`document.querySelectorAll('.component-widget').length===${widgetCount}`, 'Shared catalog panel did not import');
    const loadedFonts = await evaluate(`(async()=>{const faces=await Promise.all(['OSGPortableSans','OSGPortableMono'].map(face=>document.fonts.load('22px '+face)));await document.fonts.ready;return faces.map(group=>group.map(font=>({family:font.family,status:font.status})));})()`);
    for (const [index, name] of ['OSGPortableSans', 'OSGPortableMono'].entries()) assert.ok(loadedFonts[index].some(font => font.family === name && font.status === 'loaded'), 'The actual portable font face must load');
    assert.equal(await evaluate(`document.querySelectorAll('.widget-artwork text tspan').length>40`), true);
    await click('Edit panel name');
    await change('[aria-label="Panel name"]', panelName);
    await evaluate(`document.querySelector('[aria-label="Panel name"]').dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}))`);
    await until(`document.querySelector('.panel-title-button')?.textContent.trim()===${JSON.stringify(panelName)}`, 'Panel rename did not commit');
    assert.equal(await evaluate(`document.querySelectorAll('.widget-artwork').length`), widgetCount);
    assert.equal(await evaluate(`document.documentElement.scrollWidth>innerWidth`), false);
    assert.equal(await evaluate(`document.querySelectorAll('.component-widget [data-layer-id]').length`), layerCount);
    await screenshot('shared-catalog');
    checks.push(`The native designer imports all ${widgetCount} portable catalog widgets and renders their ordered layers.`);
    await click('Save');
    await until(`document.querySelector('.editor-title>p')?.textContent==='Edit the panel layout.'&&!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Save'&&!b.disabled)`, 'Panel save did not settle');
    await click('Preview');
    await until(`document.body.innerText.includes('Preview armed')`, 'Preview did not arm');
    const selector = '[data-definition-id="osg-illuminated-button"] .component-hit';
    const point = await evaluate(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});e.scrollIntoView({block:'center'});const r=e.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await send('Input.dispatchMouseEvent', { type: 'mousePressed', button: 'left', buttons: 1, clickCount: 1, ...point });
    await until(`document.querySelector('[data-definition-id="osg-illuminated-button"] [data-widget-state]')?.getAttribute('data-widget-state')==='pressed'`, 'Local press was not shown');
    await send('Input.dispatchMouseEvent', { type: 'mouseReleased', button: 'left', buttons: 0, clickCount: 1, ...point });
    await until(`document.body.innerText.includes('2 recorded transitions')`, 'Momentary press/release was not recorded');
    assert.equal(await evaluate(`document.querySelector('[data-definition-id="osg-illuminated-button"] [data-widget-state]').getAttribute('data-widget-state')`), 'unknown');
    checks.push('Actual pointer capture emits two momentary transitions and returns to unknown feedback without inventing confirmation.');
    await click('Stop preview');
    await evaluate(`document.querySelector('[aria-label^="Edit design Illuminated button"]').click()`);
    await until(`!!document.querySelector('dialog.widget-designer[open]')`, 'Widget editor did not open');
    // Repeated acceptance runs retain immutable revisions. Remove only this
    // test's previous READY annotations before adding a new one, keeping the
    // reusable recipe bounded and the main label readable on every run.
    const baseLayers = source.document.presentation.definitions.find(definition => definition.id === 'osg-illuminated-button' && definition.version === 1).layers.length;
    if (await evaluate(`document.querySelector('[aria-label="Widget name"]').value==='Illuminated button native acceptance'`)) {
      while (await evaluate(`document.querySelectorAll('.layers-panel li').length>${baseLayers}`)) {
        const count = await evaluate(`document.querySelectorAll('.layers-panel li').length`);
        await evaluate(`document.querySelector('.layers-panel li:last-child button').click()`);
        await until(`document.querySelector('[aria-label="Layer text"]')?.value==='READY'`, 'Only prior acceptance annotations may be normalized');
        await click('Delete layer');
        await until(`document.querySelectorAll('.layers-panel li').length===${count - 1}`, 'Prior acceptance annotation was not removed');
      }
    }
    const beforeLayers = await evaluate(`document.querySelectorAll('.layers-panel li').length`);
    await change('[aria-label="Widget name"]', 'Illuminated button native acceptance');
    await change('[aria-label="New layer type"]', 'text');
    // The type selector uses a Svelte value binding, which listens to change.
    await click('Add layer');
    await until(`document.querySelectorAll('.layers-panel li').length===${beforeLayers + 1}`, 'New layer did not appear');
    await change('[aria-label="Layer text"]', 'READY');
    await change('.layer-properties > .geometry:first-of-type label:nth-child(4) input', 18);
    await change('.layer-properties > .geometry:first-of-type label:nth-child(2) input', 8);
    await evaluate(`(()=>{const input=[...document.querySelectorAll('.layer-properties label')].find(label=>label.textContent.trim()==='Font size').querySelector('input');input.value='14';input.dispatchEvent(new Event('change',{bubbles:true}));})()`);
    await change('[aria-label="Widget preview state"]', 'active');
    await change('[aria-label="Layer text"]', 'ACTIVE');
    await until(`document.querySelector('.design-stage svg')?.textContent.includes('ACTIVE')`, 'State text override did not render');
    await click('Reset state overrides');
    await until(`document.querySelector('.design-stage svg')?.textContent.includes('READY')&&!document.querySelector('.design-stage svg')?.textContent.includes('ACTIVE')`, 'State override did not reset');
    await evaluate(`[...document.querySelectorAll('.layers-panel summary')].find(s=>s.textContent==='Hit area').click()`);
    await change('[aria-label="Hit area width"]', 90);
    await change('[aria-label="Hit area x"]', 5);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Hit area x"]').value`), '5');
    await screenshot('widget-editor');
    await click('Save to widget library');
    await until(`!document.querySelector('dialog.widget-designer')&&!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Illuminated button native acceptance')`, 'Widget revision was not saved');
    assert.equal(await evaluate(`document.querySelector('[data-definition-id="osg-illuminated-button"]').getAttribute('data-definition-version')`), '1');
    checks.push('Library editing saves an immutable new revision while the existing panel remains pinned to version 1.');
    await click('Illuminated button native acceptance');
    await until(`document.querySelectorAll('[data-definition-id="osg-illuminated-button"]').length===2`, 'Second independent instance did not appear');
    const revisions = await evaluate(`[...document.querySelectorAll('[data-definition-id="osg-illuminated-button"]')].map(e=>Number(e.getAttribute('data-definition-version')))`);
    assert.equal(revisions[0], 1); assert.ok(revisions[1] > 1);
    // The rich ESP32 profile requires independent, nonoverlapping touch areas.
    // Reuse the read-only frame's cell rather than covering the first control.
    for (const [field, value] of [[3, 144], [4, 86], [1, 612], [2, 194]]) await change(`.properties-overlay .bounds label:nth-child(${field}) input`, value);
    await click('Save');
    await until(`document.querySelector('.editor-title>p')?.textContent==='Edit the panel layout.'&&!![...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Save'&&!b.disabled)`, 'Updated panel did not save');
    await screenshot('reused-instance');
    await send('Page.reload', { ignoreCache: true });
    await until(`!!document.querySelector('nav[aria-label="Main navigation"]')`, 'Reload failed');
    await click('Companions');
    await until(`!!document.querySelector('.panel-import:not(:disabled),.back-panels:not(:disabled)')`, 'Reloaded Companions did not finish loading');
    if (await evaluate(`!!document.querySelector('.back-panels')`)) await click('Back to Panels');
    await until(`!![...document.querySelectorAll('.panel-tile')].find(e=>e.getAttribute('aria-label')===${JSON.stringify('Edit ' + panelName)})`, 'Saved panel not restored');
    const saved = await evaluate(`document.querySelector('[aria-label='+CSS.escape(${JSON.stringify('Edit ')}+${JSON.stringify(panelName)})+']').innerText`);
    assert.match(saved, /Shared systems/i);
    await click('Edit ' + panelName);
    await until(`document.querySelectorAll('[data-definition-id="osg-illuminated-button"]').length===2`, 'Saved instance revisions not restored');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('[data-definition-id="osg-illuminated-button"]')].map(e=>Number(e.getAttribute('data-definition-version')))`), revisions);
    assert.equal(await evaluate(`!!document.querySelector('[aria-label="Edit design Illuminated button native acceptance"]')`), true);
    assert.equal(await evaluate(`document.querySelector('[data-definition-id="osg-illuminated-button"][data-definition-version="${revisions[1]}"] .component-hit').style.width`), '90%');
    checks.push('The saved panel and widget library persist after native WebView reload.');
    await evaluate(`document.querySelector('[aria-label="Edit design Linear gauge"]').click()`);
    await until(`!!document.querySelector('dialog.widget-designer[open]')`, 'Gauge editor did not open');
    await evaluate(`[...document.querySelectorAll('.layers-panel li button')].find(button=>button.querySelector('strong')?.textContent==='Gauge').click()`);
    await until(`document.querySelector('[aria-label="Minimum property"]')?.value==='minimum'&&document.querySelector('[aria-label="Maximum property"]')?.value==='maximum'`, 'Exposed gauge ranges were not retained in the visual inspector');
    assert.equal(await evaluate(`document.querySelector('[aria-label="Gauge minimum"]').disabled&&document.querySelector('[aria-label="Gauge maximum"]').disabled`),true);
    await change('[aria-label="Minimum property"]','');
    assert.equal(await evaluate(`document.querySelector('[aria-label="Gauge minimum"]').disabled`),false);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Gauge minimum"]').value`),'0');
    await click('Cancel');
    checks.push('The visual gauge inspector preserves exposed ranges and can intentionally unlink a property without changing its resolved value.');
    const exported = await download('Export', 'panel-export');
    const portable = await importBundle(exported.text), published = await publishDocument(portable.document, portable.assets);
    assert.equal(portable.document.presentation.widgets.length, widgetCount + 1);
    assert.deepEqual(portable.document.presentation.widgets.filter(widget => widget.config.definitionId === 'osg-illuminated-button').map(widget => widget.config.definitionVersion), revisions);
    const nativePublications = await evaluate(`(async()=>{const reply=await window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command:'companion_status',clientId:'widget-export-acceptance'}});if(!reply.ok)throw Error(reply.error);return reply.state.companions.host.publications;})()`);
    assert.ok(nativePublications.some(panel => panel.publicationHash === published.publicationHash), 'The UI-exported panel must match a publication admitted by the actual Cockpit host');
    const widgetExport = await download('Export widgets', 'widget-export');
    const widgetLibrary = await importWidgetBundle(widgetExport.text);
    assert.ok(widgetLibrary.library.definitions.some(definition => definition.id === 'osg-illuminated-button' && definition.version === revisions[1] && definition.layers.some(layer => layer.kind === 'text' && layer.text === 'READY')));
    await importFile('[aria-label="Import widget library"]', widgetExport.file);
    await until(`document.body.innerText.includes('Widget library imported. Existing panels keep their pinned versions.')`, 'The exported widget library did not import through its normal UI');
    assert.deepEqual(await evaluate(`[...document.querySelectorAll('[data-definition-id="osg-illuminated-button"]')].map(e=>Number(e.getAttribute('data-definition-version')))`), revisions);
    checks.push('Actual UI downloads round-trip the edited library and exact host-admitted panel publication without changing pinned revisions.');
    // Panel imports intentionally allocate fresh installation-independent control
    // identities. Verify their content through another normal UI export.
    await click('Back to Panels');
    await importFile('input.panel-import', exported.file);
    await until(`document.querySelectorAll('.component-widget').length===${widgetCount + 1}`, 'The UI-exported panel did not reimport');
    const roundTrip = await download('Export', 'panel-reimport');
    const imported = await importBundle(roundTrip.text);
    assert.equal(canonical(imported.document.presentation.definitions), canonical(portable.document.presentation.definitions));
    const configs = document => document.presentation.widgets.map(widget => {
      const config = structuredClone(widget.config);
      const definition = document.presentation.definitions.find(definition => definition.id === config.definitionId && definition.version === config.definitionVersion);
      if (definition.interaction.kind === 'navigate') config.properties[definition.interaction.pageProperty] = document.ui.pages.findIndex(page => page.id === config.properties[definition.interaction.pageProperty]);
      return config;
    });
    assert.deepEqual(configs(imported.document), configs(portable.document));
    assert.notEqual(imported.document.ui.controls[0].id, portable.document.ui.controls[0].id);
    checks.push('The exported panel reimports through the actual UI with identical pinned definitions/properties and fresh independent control identities.');
    // Return to the saved panel; the round-trip draft was never saved.
    await click('Back to Panels');
    await until(`!![...document.querySelectorAll('button')].find(button=>button.textContent.trim()==='Discard and continue')`, 'Unsaved imported panel did not request discard');
    await click('Discard and continue');
    await until(`!!document.querySelector('.panel-import:not(:disabled)')`, 'Panel list did not return after discarding the import');
    if(process.env.OSG_WIDGETS_KEEP_ACCEPTANCE_PANEL!=='1'){
      await click('Delete '+panelName);
      await until(`!!document.querySelector('.delete-confirmation[open]')`,'Acceptance cleanup confirmation did not open');
      await evaluate(`document.querySelector('.delete-confirmation .destructive-button').click()`);
      await until(`![...document.querySelectorAll('.panel-tile-title')].some(title=>title.textContent===${JSON.stringify(panelName)})&&!!document.querySelector('.panel-import:not(:disabled)')`,'Acceptance panel cleanup did not finish');
      checks.push('The temporary unassigned acceptance panel is removed after persistence is verified, preserving bounded draft/cache capacity.');
    }
    await writeFile(`${directory}/evidence.json`, JSON.stringify({ origin, actualNativeCockpit: true, checks, saved, exportedPanel: exported.file, exportedWidgetLibrary: widgetExport.file, publicationHash: published.publicationHash, capabilityFingerprint: published.capabilityFingerprint }, null, 2));
    console.log(JSON.stringify({ directory, checks }));
  } catch (error) {
    await writeFile(`${directory}/failure.txt`, String(error));
    try { await screenshot('failure'); await writeFile(`${directory}/page.txt`, await evaluate('document.body.innerText')); } catch {}
    throw error;
  } finally { socket.close(); }
});
