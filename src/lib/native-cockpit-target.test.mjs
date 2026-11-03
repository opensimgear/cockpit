import test from 'node:test';
import assert from 'node:assert/strict';
import {nativeMainTarget} from '../../scripts/reactive/cdp.mjs';

test('actual native harness selects main while a restricted activation WebView is still open',()=>{
  const activation={id:'activation',url:'http://127.0.0.1:1420/?profile-activation-window=1'};
  const main={id:'main',url:'http://127.0.0.1:1420/'};
  for(const pages of [[activation,main],[main,activation]])assert.equal(nativeMainTarget(pages),main);
  assert.equal(nativeMainTarget([activation]),undefined);
});

test('native entry selection covers dev and packaged URLs without claiming other pages',()=>{
  for(const url of ['http://localhost:1420/index.html','http://tauri.localhost/']){
    const main={url};assert.equal(nativeMainTarget([{url:'about:blank'},{url:'http://remote.test/?localhost:1420'},main]),main);
  }
  assert.equal(nativeMainTarget([{url:'http://127.0.0.1:5173/'},{url:'http://localhost:1420/settings.html'}]),undefined);
});
