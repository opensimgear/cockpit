import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {mkdir} from 'node:fs/promises';
const require=createRequire(import.meta.url),{chromium}=require(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');

test('running Cockpit: content widget editor and shared States/Signals canvas controls',{timeout:180000},async()=>{
  const endpoint=(await(await fetch('http://127.0.0.1:9222/json/version')).json()).webSocketDebuggerUrl;
  const browser=await chromium.connectOverCDP(endpoint,{noDefaults:true});
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith('http://127.0.0.1:1420'));
  assert.ok(page,'Start the dev Cockpit first.');page.setDefaultTimeout(15000);
  const directory='.research/state-signal/canvas';await mkdir(directory,{recursive:true});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const preferences=await page.evaluate(()=>[localStorage.getItem('osg-canvas-navigation'),localStorage.getItem('osg-canvas-styling')]);
  const editor=page.getByRole('region',{name:'Generic widget designer',exact:true});
  const select=mode=>editor.locator('nav').getByRole('button',{name:mode,exact:true}).click();
  const open=async()=>{await page.getByRole('button',{name:'Companions',exact:true}).click();await page.getByRole('button',{name:'Edit design Toggle',exact:true}).click();await editor.waitFor();};
  const settings=async(wheelAction,arrowStyle,cornerRadius=24)=>page.evaluate(async({wheelAction,arrowStyle,cornerRadius})=>{
    (await import('/src/lib/canvas-navigation.ts')).setWheelAction(wheelAction);
    const styling=await import('/src/lib/canvas-styling.ts');styling.setArrowStyle(arrowStyle);styling.setCornerRadius(cornerRadius);
  },{wheelAction,arrowStyle,cornerRadius});
  const state=()=>editor.locator('.plane').evaluate(element=>{const m=new DOMMatrix(getComputedStyle(element).transform);return{x:m.e,y:m.f,zoom:m.a};});
  const fitted=()=>editor.locator('.viewport').evaluate(element=>{const v=element.getBoundingClientRect();return [...element.querySelectorAll('article')].every(card=>{const c=card.getBoundingClientRect();return c.left>=v.left+20&&c.top>=v.top+20&&c.right<=v.right-20&&c.bottom<=v.bottom-20;});});
  const wheel=async(ctrlKey=false)=>{await editor.locator('.viewport').dispatchEvent('wheel',{deltaY:80,deltaX:20,deltaMode:0,ctrlKey,bubbles:true,cancelable:true});};
  try{
    await page.evaluate(async()=>{const entry=performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname==='/src/lib/companion-startup.ts');if(!entry)throw Error('Startup module was not loaded');await(await import(entry.name)).restoreCompanionPanels();});await page.reload();await open();assert.equal(await page.locator('dialog[open]').count(),0);
    assert.equal(await page.getByRole('region',{name:'Widgets',exact:true}).count(),0,'Library is replaced by the editor');
    assert.equal(await editor.evaluate(el=>getComputedStyle(el).position),'static');
    for(const mode of ['States','Signals']){
      await select(mode);await editor.locator('[data-graph-node]').first().waitFor();await editor.getByRole('button',{name:'Fit all',exact:true}).click();assert.equal(await fitted(),true,mode+' initial fit');
      for(const name of ['Fit all','Auto-layout','Zoom in','Zoom out'])assert.equal(await editor.getByRole('button',{name,exact:true}).locator('svg').count(),1);
      const dotStyle=await editor.locator('.port-dot').first().evaluate(el=>{const s=getComputedStyle(el);return{width:s.width,height:s.height,border:s.borderTopWidth,radius:s.borderRadius,pointer:s.pointerEvents};});assert.ok(parseFloat(dotStyle.border)>0&&parseFloat(dotStyle.border)<=1,'One-pixel border snaps to the display scale');delete dotStyle.border;assert.deepEqual(dotStyle,{width:'8px',height:'8px',radius:'50%',pointer:'none'});
      const before=await state();await editor.getByRole('button',{name:'Zoom in',exact:true}).click();assert.ok((await state()).zoom>before.zoom);await editor.getByRole('button',{name:'Zoom out',exact:true}).click();assert.ok(Math.abs((await state()).zoom-before.zoom)<.001);
      await settings('pan','smooth');const smooth=await editor.locator('.wires>path').evaluateAll(paths=>paths.map(p=>p.getAttribute('d')));assert.ok(smooth.some(d=>d.includes('C')));
      let start=await state();await wheel();let next=await state();assert.equal(next.zoom,start.zoom);assert.ok(Math.abs(next.x-start.x+20)<1&&Math.abs(next.y-start.y+80)<1);
      await wheel(true);assert.ok((await state()).zoom<next.zoom);
      await settings('zoom','orthogonal',0);const orthogonal=await editor.locator('.wires>path').evaluateAll(paths=>paths.map(p=>p.getAttribute('d')));assert.notDeepEqual(orthogonal,smooth);assert.ok(orthogonal.every(d=>!d.includes('C')&&!d.includes('A')));
      start=await state();await wheel();next=await state();assert.ok(next.zoom<start.zoom);await wheel(true);assert.equal((await state()).zoom,next.zoom);
      await editor.getByRole('button',{name:'Fit all',exact:true}).click();
      const viewport=editor.locator('.viewport');await viewport.scrollIntoViewIfNeeded();const box=await viewport.boundingBox();start=await state();await page.mouse.move(box.x+8,box.y+8);await page.mouse.down();await page.mouse.move(box.x+68,box.y+48,{steps:5});await page.mouse.up();next=await state();assert.ok(Math.abs(next.x-start.x-60)<2&&Math.abs(next.y-start.y-40)<2,'Background drag pans');
      await viewport.focus();await page.keyboard.press('0');assert.equal(await fitted(),true);await page.keyboard.press('+');assert.ok((await state()).zoom>next.zoom);
      await editor.getByRole('button',{name:'Auto-layout',exact:true}).click();await page.waitForFunction(()=>document.querySelector('[aria-label="Auto-layout"]')?.getAttribute('aria-busy')==='false');assert.equal(await fitted(),true,mode+' auto-layout fits');assert.equal(await editor.locator('.notice').count(),0);
      const positions=await editor.locator('[data-graph-node]').evaluateAll(cards=>Object.fromEntries(cards.map(c=>[c.dataset.graphNode,{x:c.offsetLeft,y:c.offsetTop,w:c.offsetWidth,h:c.offsetHeight}])));
      const entries=Object.values(positions);for(let a=0;a<entries.length;a++)for(let b=a+1;b<entries.length;b++){const x=entries[a],y=entries[b];assert.ok(x.x+x.w<=y.x||y.x+y.w<=x.x||x.y+x.h<=y.y||y.y+y.h<=x.y,'Auto-layout must separate cards');}
      await select('Design');await select(mode);assert.deepEqual(await editor.locator('[data-graph-node]').evaluateAll(cards=>Object.fromEntries(cards.map(c=>[c.dataset.graphNode,{x:c.offsetLeft,y:c.offsetTop,w:c.offsetWidth,h:c.offsetHeight}]))),positions,'Layout persists independently of source');
      await editor.getByRole('button',{name:'Fit all',exact:true}).click();const header=editor.locator('[data-graph-node] header').first();await header.scrollIntoViewIfNeeded();const card=await header.boundingBox(),id=await header.locator('..').getAttribute('data-graph-node'),old=positions[id],zoom=(await state()).zoom;
      await page.mouse.move(card.x+card.width/2,card.y+card.height/2);await page.mouse.down();await page.mouse.move(card.x+card.width/2+25,card.y+card.height/2+15,{steps:5});await page.mouse.up();const moved=await header.locator('..').evaluate(c=>({x:c.offsetLeft,y:c.offsetTop}));assert.ok(Math.abs(moved.x-old.x-25/zoom)<2&&Math.abs(moved.y-old.y-15/zoom)<2,'Header drag uses world coordinates');
      await editor.getByRole('button',{name:'Fit all',exact:true}).click();const output=editor.locator('.port.output').first();await output.hover();assert.equal(await output.locator('.port-dot').evaluate(el=>getComputedStyle(el).backgroundColor),await output.locator('.port-dot').evaluate(el=>getComputedStyle(el).borderTopColor));await page.screenshot({path:`${directory}/${mode.toLowerCase()}.png`});
    }
    // Content navigation returns to the calling panel without discarding its draft.
    await editor.getByRole('button',{name:'Back',exact:true}).click();await page.getByRole('region',{name:'Panels',exact:true}).getByRole('button',{name:'New',exact:true}).click();
    const panelName='Canvas return check';await page.getByRole('textbox',{name:'Panel name',exact:true}).fill(panelName);await page.getByRole('textbox',{name:'Panel name',exact:true}).press('Tab');
    await page.getByRole('complementary',{name:'Panel configuration'}).getByRole('button',{name:'New',exact:true}).click();await editor.waitFor();await editor.getByRole('button',{name:'Back',exact:true}).click();assert.equal(await page.getByRole('textbox',{name:'Panel name',exact:true}).inputValue(),panelName);
    await page.evaluate(()=>{window.confirm=()=>true;});await page.getByRole('button',{name:'Back',exact:true}).click();await page.getByRole('button',{name:'Edit design Toggle',exact:true}).click();
    await editor.getByRole('textbox',{name:'Widget name',exact:true}).fill('Unsaved capsule');await editor.getByRole('textbox',{name:'Widget name',exact:true}).press('Tab');
    await page.evaluate(()=>{window.confirm=()=>false;});await editor.getByRole('button',{name:'Back',exact:true}).click();assert.equal(await editor.isVisible(),true);
    await page.evaluate(()=>{window.confirm=()=>true;});await editor.getByRole('button',{name:'Back',exact:true}).click();assert.equal(await editor.count(),0);
    assert.deepEqual(errors,[]);
  }catch(error){console.error(error);throw error;}finally{
    try{await page.evaluate(async values=>{for(const [i,key]of ['osg-canvas-navigation','osg-canvas-styling'].entries())values[i]===null?localStorage.removeItem(key):localStorage.setItem(key,values[i]);},preferences);
    await page.evaluate(async()=>{const entry=performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname==='/src/lib/companion-startup.ts');if(!entry)throw Error('Startup module was not loaded');await(await import(entry.name)).restoreCompanionPanels();});await page.reload();await open();await select('States');await page.evaluate(async()=>{const entry=performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname==='/src/lib/companion-startup.ts');if(!entry)throw Error('Startup module was not loaded');await(await import(entry.name)).restoreCompanionPanels();});}finally{await browser.close();}
  }
});
