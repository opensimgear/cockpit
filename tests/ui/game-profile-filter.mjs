// Isolated page using the running cockpit's real components; no native state changes.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
const origin=process.env.OSG_GAMES_UI_URL??'http://127.0.0.1:1420';

test('Games filters by matching profile presence and Clear restores all games',{timeout:60_000},async()=>{
  const browser=await chromium.launch({headless:true,...(process.env.OSG_BROWSER_CHANNEL?{channel:process.env.OSG_BROWSER_CHANNEL}:{})});
  try{
    const page=await browser.newPage({viewport:{width:1440,height:1000}}),errors=[];
    page.on('pageerror',error=>errors.push(error.message));
    const compiled=await(await fetch(origin+'/src/lib/GamesPanel.svelte')).text();
    const runtime=compiled.match(/from ["']([^"']*\/svelte\.js[^"']*)["']/)[1];
    const props={busy:false,status:{catalogue:[
      {id:'racer',name:'Racing game',source:'steam',installed:true,launchSupported:false},
      {id:'local',name:'Local game',source:'local',installed:true,launchSupported:false},
    ]},profiles:[{id:'fixture',name:'Racing profile',isDefault:false,gameIds:['racer']},{id:'default',name:'Default',isDefault:true,gameIds:[]}]};
    const path='/__game_profile_filter_test';
    await page.route(origin+path,route=>route.fulfill({contentType:'text/html',body:`<html><body><div class="cockpit"><main style="height:100dvh;padding:24px;overflow:auto"><div id="test"></div></main></div><script type="module">import {mount} from '${runtime}';import Games from '/src/lib/GamesPanel.svelte';import '/src/app.css';mount(Games,{target:document.querySelector('#test'),props:{...${JSON.stringify(props)},request(){throw Error('Filtering must not change native state.');}}});</script></body></html>`}));
    await page.goto(origin+path);
    const tiles=page.locator('.game-tile');await expect(tiles).toHaveCount(2);
    await page.getByRole('button',{name:'Filter games',exact:true}).click();
    const filters=page.getByRole('region',{name:'Game filters'}),group=filters.getByRole('group',{name:'Has profile',exact:true});
    await group.getByRole('checkbox',{name:'Yes',exact:true}).check();
    await expect(tiles).toHaveCount(1);await expect(tiles).toHaveAttribute('data-game-id','racer');
    await group.getByRole('checkbox',{name:'No',exact:true}).check();await expect(tiles).toHaveCount(2);
    await group.getByRole('checkbox',{name:'Yes',exact:true}).uncheck();
    await expect(tiles).toHaveCount(1);await expect(tiles).toHaveAttribute('data-game-id','local');
    await filters.getByRole('button',{name:'Clear',exact:true}).click();
    await expect(tiles).toHaveCount(2);await expect(filters).toHaveCount(0);
    assert.deepEqual(errors,[]);
  }finally{await browser.close();}
});
