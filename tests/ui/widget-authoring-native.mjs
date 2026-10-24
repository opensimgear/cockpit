import assert from 'node:assert/strict';
import test from 'node:test';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
const {chromium}=createRequire(import.meta.url)(process.env.OSG_PLAYWRIGHT_MODULE??'C:/Users/ivang/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright');

test('running Cockpit: contextual States and Signals authoring persists complete source',{timeout:240000},async()=>{
  const endpoint=process.env.OSG_GENERIC_CDP??(await(await fetch('http://127.0.0.1:9222/json/version',{signal:AbortSignal.timeout(15000)})).json()).webSocketDebuggerUrl;
  const browser=await chromium.connectOverCDP(endpoint,{noDefaults:true,timeout:15000});
  const page=browser.contexts().flatMap(c=>c.pages()).find(p=>p.url().startsWith(process.env.OSG_GENERIC_ORIGIN??'http://127.0.0.1:1420'));
  assert.ok(page,'Start the actual native dev Cockpit before running this acceptance test.');page.setDefaultTimeout(12000);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const request=async payload=>{const reply=await page.evaluate(request=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{...request,clientId:'widget-authoring-native'}}),payload);assert.equal(reply.ok,true,reply.error??payload.command);return reply.state;};
  const id='authoring-native',title='Authoring acceptance';
  const fixture={id,title,size:{width:64,height:32},parameters:[],behavior:{version:1,signals:[{id:'flag',source:'local',type:'boolean',initial:false},{id:'available',source:'input',type:'boolean',initial:true}],events:[{id:'activate'}],regions:[{id:'machine',initial:'ready',states:['ready','active'],transitions:[]}]},inputs:[],outputs:[],visualDomains:[],hitRegions:[],scene:[{id:'face',kind:'rectangle',x:0,y:0,width:64,height:32,fill:'#335566'}],design:{nodes:[],owners:[],appearances:[],effects:[]}};
  const editor=page.locator('section.generic-designer'),nav=editor.locator('nav[aria-label="Designer workspaces"]');
  const tab=name=>nav.getByRole('button',{name,exact:true}).click();
  const inspector=()=>editor.locator('.graph-editor aside');
  const behavior=()=>inspector().locator('.behavior-inspector');
  const signal=()=>inspector().locator('.signal-inspector');
  const card=id=>editor.locator('[data-graph-node]').filter({has:page.locator(`header[aria-label=${JSON.stringify('Move '+id)}]`)});
  const choose=async id=>{const header=card(id).locator('header');await header.focus();await header.press('Enter');};
  const change=async(locator,value)=>{if(await locator.evaluate(node=>node.tagName==='SELECT'))await locator.selectOption(String(value));else{await locator.fill(String(value));await locator.press('Tab');}};
  const field=(root,label)=>root.getByLabel(label,{exact:true}).or(root.locator('label').filter({hasText:new RegExp('^'+label.replace(/[.*+?^${}()|[\]\\]/g,'\\$&'))}).locator('input,select,textarea')).first();
  const add=async name=>{const menu=editor.locator('.graph-editor .toolbar details.add');if(await menu.getAttribute('open')===null)await menu.locator('summary').click();await menu.getByRole('button',{name,exact:true}).click();};
  const clean=async()=>{assert.equal(await editor.locator('.diagnostic').count(),0,await editor.locator('.diagnostic').allTextContents());assert.equal(await inspector().locator('[role="alert"]').count(),0,await inspector().locator('[role="alert"]').allTextContents());};
  const directory='.research/widget-authoring-native';await mkdir(directory,{recursive:true});
  try{
    await request({command:'companion_status'});
    const library=(await request({command:'widget_library_get'})).widgetLibrary;
    await request({command:'widget_save',bundle:{definitions:[fixture]},expectedRevision:library.revision});
    await page.evaluate(()=>{history.replaceState(null,'','/');window.confirm=()=>true;});await page.reload();
    await page.getByRole('button',{name:'Companions',exact:true}).click();await page.getByRole('button',{name:'Edit design '+title,exact:true}).click();await editor.waitFor();
    assert.equal(await nav.getByRole('button',{name:'Data',exact:true}).count(),0);assert.equal(await nav.getByRole('button',{name:'Behavior',exact:true}).count(),0);

    await tab('Signals');await add('Local value');await change(field(signal(),'ID'),'level');await change(field(signal(),'Type'),'number');await change(field(signal(),'Maximum'),'20');await change(field(signal(),'Initial'),'4');
    await add('Local value');await change(field(signal(),'ID'),'mode');await change(field(signal(),'Type'),'enum');await change(field(signal(),'Values (comma separated)'),'off, on, standby');await change(field(signal(),'Initial'),'standby');
    await add('External input');await change(field(signal(),'ID'),'quality');await change(field(signal(),'Type'),'enum');await change(field(signal(),'Values (comma separated)'),'fresh, unknown, stale, disconnected');await change(field(signal(),'Initial'),'unknown');
    await add('Feedback');await change(field(signal(),'ID'),'confirmed');await change(field(signal(),'Type'),'number');await change(field(signal(),'Maximum'),'20');await change(field(signal(),'Fresh for (ms)'),'1500');await change(field(signal(),'Quality signal'),'quality');await field(signal(),'Require game confirmation').check();
    await add('To profile');await change(field(signal(),'ID'),'command');await change(field(signal(),'Label'),'Requested level');await change(field(signal(),'Signal'),'level');assert.equal(await field(signal(),'Type').inputValue(),'number');assert.equal(await field(signal(),'Neutral').inputValue(),'4');
    await add('From profile');await change(field(signal(),'ID'),'feedback');await change(field(signal(),'Label'),'Confirmed level');await change(field(signal(),'Signal'),'confirmed');
    await add('Output binding');await change(field(signal(),'ID'),'pulse');await change(field(signal(),'Neutral'),'true');
    await add('Rendering samples');await change(field(signal(),'Signal'),'level');await change(field(signal(),'Samples'),'5');await change(field(signal(),'Maximum error (px)'),'5');await change(field(signal(),'Travel (px)'),'20');await change(field(signal(),'Smoothing (ms)'),'120');await clean();
    await choose('level');assert.equal(await field(signal(),'Initial').inputValue(),'4');await choose('Requested level');assert.equal(await field(signal(),'ID').inputValue(),'command');await choose('level rendering');assert.equal(await field(signal(),'Samples').inputValue(),'5');

    await tab('States');await add('Event');await change(field(behavior(),'ID'),'accept');await field(behavior(),'available').check();
    await add('Timer');await change(field(behavior(),'ID'),'delay');await change(field(behavior(),'Event'),'accept');await change(field(behavior(),'Delay (ms)'),'250');await field(behavior(),'Repeat').check();
    await add('Timer');await change(field(behavior(),'ID'),'expiry');await change(field(behavior(),'Event'),'accept');await change(field(behavior(),'Delay (ms)'),'700');
    await add('Touch area');await change(field(behavior(),'ID'),'surface');await change(field(behavior(),'width'),'32');await change(field(behavior(),'height'),'24');await change(field(behavior(),'x'),'4');await change(field(behavior(),'y'),'3');for(const name of ['press','release','activate','cancel'])await change(field(behavior(),name),'accept');assert.equal(await behavior().getByRole('img',{name:'Hit region on widget artwork'}).count(),1);
    await change(field(behavior(),'cancel'),'');await editor.locator('[data-connection-source="hit:surface|cancel"]').focus();await page.keyboard.press('Enter');await editor.locator('[data-connection-target="event:accept"]').focus();await page.keyboard.press('Enter');await choose('surface');assert.equal(await field(behavior(),'cancel').inputValue(),'accept');await page.screenshot({path:directory+'/touch.png'});
    await add('Transition');await change(field(behavior(),'ID'),'accept-input');await change(field(behavior(),'Event'),'accept');await change(field(behavior(),'From'),'*');await change(field(behavior(),'To'),'');await change(field(behavior(),'Priority'),'7');await field(behavior(),'Guard').check();await change(field(behavior(),'Condition operator'),'eq');await change(field(behavior(),'Left operator'),'ref');await change(field(behavior(),'Left reference'),'available');await change(field(behavior(),'Right value type'),'boolean');await change(field(behavior(),'Right value'),'true');
    for(const type of ['set','emit','cue','start-timer','cancel-timer'])await behavior().getByRole('button',{name:type,exact:true}).click();
    const actions=behavior().locator('fieldset');assert.equal(await actions.count(),5);
    const action=type=>actions.filter({has:page.locator('legend',{hasText:new RegExp(' · '+type+'$')})});
    await change(field(action('set'),'Signal'),'flag');await change(field(action('set'),'Value value'),'true');await change(field(action('emit'),'Output'),'pulse');await change(field(action('emit'),'Value value'),'false');await change(field(action('cue'),'Cue'),'accept-sound');await change(field(action('start-timer'),'Timer'),'delay');await change(field(action('cancel-timer'),'Timer'),'expiry');
    assert.match(await behavior().innerText(),/same event snapshot/);await clean();
    await choose('accept');await change(field(behavior(),'ID'),'accepted');await choose('delay');await change(field(behavior(),'ID'),'delay-renamed');await clean();
    await editor.getByRole('button',{name:'Edit transition accept-input',exact:true}).first().focus();await page.keyboard.press('Enter');assert.equal(await field(behavior(),'ID').inputValue(),'accept-input');await behavior().scrollIntoViewIfNeeded();await page.screenshot({path:directory+'/states.png'});

    await editor.locator('.buildbar').getByRole('button',{name:'Compile',exact:true}).click();await editor.getByRole('button',{name:'Save',exact:true}).waitFor();
    await page.waitForFunction(()=>!document.querySelector('.generic-designer .save-action button')?.disabled,undefined,{timeout:90000});await editor.getByRole('button',{name:'Save',exact:true}).click();await editor.waitFor({state:'hidden'});
    const saved=(await request({command:'widget_get',id})).widgetLibrary.definitions[0];
    assert.equal(saved.behavior.signals.find(s=>s.id==='level').type,'number');assert.equal(saved.behavior.signals.find(s=>s.id==='level').initial,4);assert.deepEqual(saved.behavior.signals.find(s=>s.id==='mode').values,['off','on','standby']);assert.equal(saved.behavior.signals.find(s=>s.id==='mode').initial,'standby');assert.equal(saved.behavior.signals.find(s=>s.id==='confirmed').freshnessMs,1500);assert.equal(saved.behavior.signals.find(s=>s.id==='confirmed').qualitySignal,'quality');assert.equal(saved.behavior.signals.find(s=>s.id==='confirmed').requireGameConfirmed,true);
    assert.equal(saved.inputs.find(p=>p.id==='command').signal,'level');assert.equal(saved.outputs.find(p=>p.id==='feedback').signal,'confirmed');assert.equal(saved.behavior.outputs.find(p=>p.id==='command').neutral,4);assert.equal(saved.visualDomains.find(d=>d.signal==='level').samples,5);assert.equal(saved.visualDomains.find(d=>d.signal==='level').smoothingMs,120);
    assert.deepEqual(saved.behavior.events.find(e=>e.id==='accepted').updates,['available']);assert.equal(saved.behavior.timers.find(t=>t.id==='delay-renamed').event,'accepted');assert.equal(saved.behavior.timers.find(t=>t.id==='delay-renamed').repeat,true);assert.deepEqual(saved.hitRegions[0],{id:'surface',x:4,y:3,width:32,height:24,events:{press:'accepted',release:'accepted',activate:'accepted',cancel:'accepted'}});
    const transition=saved.behavior.regions[0].transitions.find(t=>t.id==='accept-input');assert.equal(transition.from,'*');assert.equal(transition.to,undefined);assert.equal(transition.event,'accepted');assert.equal(transition.priority,7);assert.deepEqual(transition.guard,{op:'eq',left:{ref:'available'},right:{literal:true}});assert.deepEqual(transition.actions,[{type:'set',signal:'flag',value:{literal:true}},{type:'emit',port:'pulse',value:{literal:false}},{type:'cue',cue:'accept-sound'},{type:'start-timer',timer:'delay-renamed'},{type:'cancel-timer',timer:'expiry'}]);
    await page.reload();await page.getByRole('button',{name:'Companions',exact:true}).click();await page.getByRole('button',{name:'Edit design '+title,exact:true}).click();await tab('States');await choose('delay-renamed');assert.equal(await field(behavior(),'Delay (ms)').inputValue(),'250');await tab('Signals');await choose('level');assert.equal(await field(signal(),'Maximum').inputValue(),'20');await page.screenshot({path:directory+'/signals.png'});
    // State-owned signal initials and their published neutral must stay aligned.
    await editor.getByRole('button',{name:'Back',exact:true}).click();
    await page.evaluate(()=>localStorage.setItem('osg-widget-layout-1:toggle:States:requested-state',JSON.stringify({off:{x:483.7905701754386,y:118.67434210526315},on:{x:80,y:48}})));
    await page.getByRole('button',{name:'Edit design Toggle',exact:true}).click();await tab('States');
    assert.ok(await editor.locator('.wires>path').count(),'New event and touch cards must not hide wires when only old state positions were saved.');
    const boxes=await editor.locator('[data-graph-node]').evaluateAll(cards=>cards.map(c=>({id:c.dataset.graphNode,x:c.offsetLeft,y:c.offsetTop,w:c.offsetWidth,h:c.offsetHeight})));
    for(let i=0;i<boxes.length;i++)for(let j=i+1;j<boxes.length;j++){const a=boxes[i],b=boxes[j];assert.ok(a.x+a.w<=b.x||b.x+b.w<=a.x||a.y+a.h<=b.y||b.y+b.h<=a.y,`Saved-layout cards overlap: ${a.id} / ${b.id}`);}
    const compileValid=async()=>{await clean();await editor.locator('.buildbar').getByRole('button',{name:'Compile',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.generic-designer .save-action button')?.disabled,undefined,{timeout:90000});};
    const ownerInitial=await field(inspector(),'Initial state').inputValue(),alternate=ownerInitial==='off'?'on':'off';
    await change(field(inspector(),'Value · '+ownerInitial),String(ownerInitial!=='on'));await compileValid();await editor.getByRole('button',{name:'Undo',exact:true}).click();
    await change(field(inspector(),'Initial state'),alternate);await compileValid();await editor.getByRole('button',{name:'Undo',exact:true}).click();
    await choose(ownerInitial);await inspector().getByRole('button',{name:'Delete',exact:true}).click();assert.equal(await field(inspector(),'Initial state').inputValue(),alternate);await compileValid();await editor.getByRole('button',{name:'Undo',exact:true}).click();assert.equal(await field(inspector(),'Initial state').inputValue(),ownerInitial);await editor.getByRole('button',{name:'Back',exact:true}).click();
    assert.deepEqual(errors,[]);await writeFile(directory+'/result.json',JSON.stringify({passed:true,id,checks:['workspace tabs','typed signals and feedback quality','profile ports','rendering samples','events and safe rename','timer authoring and safe rename','touch geometry and phase wiring','Any-state stay transition and keyboard wire inspection','guard expressions','all action types','native save and reopen','state owner value, initial and deletion keep output neutral valid']},null,2));
  }catch(error){console.error('Captured page errors:',errors);await page.screenshot({path:directory+'/failure.png'}).catch(()=>{});throw error;}finally{await browser.close();}
});
