import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');

test('running Cockpit: Preview automatically compiles, displays centered progress, cancels and reuses a valid build',{timeout:120000},async()=>{
  const endpoint=(await(await fetch((process.env.OSG_GENERIC_CDP??'http://127.0.0.1:9222')+'/json/version')).json()).webSocketDebuggerUrl;
  const browser=await chromium.connectOverCDP(endpoint,{noDefaults:true});
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_GENERIC_ORIGIN??'http://127.0.0.1:1420'));
  assert.ok(page,'Start the actual dev Cockpit before running this qualification.');
  page.setDefaultTimeout(15000);
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  try{
    await page.getByRole('button',{name:'Companions',exact:true}).click();
    await page.getByRole('button',{name:'Edit design Toggle',exact:true}).click();
    const editor=page.locator('section.generic-designer'),nav=editor.locator('nav[aria-label="Designer workspaces"]');
    const preview=()=>nav.getByRole('button',{name:'Preview',exact:true}).click();
    assert.equal(await nav.getByRole('button',{name:'Try',exact:true}).count(),0);
    // Startup may already compile the pilot. Exercise the real compiler with a cold cache.
    const originalWidth=await editor.getByRole('spinbutton',{name:'Compiled width'}).inputValue();
    await page.evaluate(async()=>{
      const module=performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname==='/packages/widget-compiler/src/compiler.ts');
      if(!module)throw Error('The widget compiler module was not loaded');
      (await import(module.name)).clearWidgetCompileCache();
    });
    await preview();
    const loading=editor.locator('.preview-loading');await loading.waitFor();
    const geometry=await loading.evaluate(node=>{
      const outer=node.getBoundingClientRect(),inner=node.firstElementChild.getBoundingClientRect();
      return {x:Math.abs(outer.x+outer.width/2-inner.x-inner.width/2),y:Math.abs(outer.y+outer.height/2-inner.y-inner.height/2)};
    });
    assert.ok(geometry.x<2&&geometry.y<2,'Loading bar must be centered in the preview workspace');
    assert.equal(await loading.getByRole('progressbar',{name:'Widget compilation'}).count(),1);
    await mkdir('.research/widget-preview',{recursive:true});
    await page.screenshot({path:'.research/widget-preview/loading.png'});
    await loading.getByRole('button',{name:'Cancel',exact:true}).click();
    await loading.waitFor({state:'hidden'});
    assert.equal(await editor.locator('.try-canvas canvas').count(),0);
    await editor.locator('.empty').getByRole('button',{name:'Retry',exact:true}).click();
    await editor.locator('.try-canvas canvas').waitFor({timeout:60000});
    assert.equal(await editor.locator('.buildbar [role="status"]').innerText(),'Compilation complete');
    const width=await editor.getByRole('spinbutton',{name:'Compiled width'}).inputValue();
    assert.equal(await editor.locator('.try-canvas canvas').evaluate(c=>c.width),Number(width));
    await nav.getByRole('button',{name:'Design',exact:true}).click();await preview();
    assert.equal(await loading.count(),0,'Opening an unchanged Preview must reuse its build');
    await editor.locator('.try-canvas canvas').waitFor();
    await nav.getByRole('button',{name:'Design',exact:true}).click();
    await editor.getByRole('spinbutton',{name:'Compiled width'}).fill('120');
    await nav.getByRole('button',{name:'Design',exact:true}).click();await preview();
    await loading.waitFor();await editor.locator('.try-canvas canvas').waitFor({timeout:60000});
    assert.equal(await editor.locator('.try-canvas canvas').evaluate(c=>c.width),120);
    await nav.getByRole('button',{name:'Design',exact:true}).click();
    await editor.getByRole('spinbutton',{name:'Compiled width'}).fill(originalWidth);
    await nav.getByRole('button',{name:'Design',exact:true}).click();await preview();
    await editor.locator('.try-canvas canvas').waitFor({timeout:60000});
    await page.screenshot({path:'.research/widget-preview/ready.png'});
    assert.deepEqual(errors,[]);
  }finally{await browser.close();}
});
