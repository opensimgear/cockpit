import test from 'node:test';
import assert from 'node:assert/strict';
import {panelResolutionWarning,panelDisplayReport} from './panel-resolution.ts';

test('display reports capture the current page and only bounded measured geometry',()=>{
  assert.deepEqual(panelDisplayReport('hash','main',800,480,2),{publicationHash:'hash',pageId:'main',cssWidth:800,cssHeight:480,pixelRatio:2});
  assert.deepEqual(panelDisplayReport(undefined,undefined,800,480,2),{cssWidth:800,cssHeight:480,pixelRatio:2});
  for(const args of [[undefined,'main',800,480,1],['hash',undefined,800,480,1],['hash','main',0,480,1],['hash','main',800,Infinity,1],['hash','main',NaN,480,1],['hash','main',16385,480,1],['hash','main',800,16385,1],['hash','main',800,480,.1],['hash','main',800,480,9],['hash','main',16384,480,8],['hash','main',800,16384,8]])assert.equal(panelDisplayReport(...args),null);
});

test('warns on either undersized dimension and includes physical target dimensions',()=>{
  for(const panel of [{width:800,height:480},{width:1600,height:480},{width:800,height:960}]){
    const message=panelResolutionWarning(panel,800,480,2);
    assert.match(message,/too small/);assert.ok(message.includes(`${panel.width} × ${panel.height}`));
    assert.match(message,/1600 × 960 px/);assert.match(message,/recompile/);
  }
});
test('matching, larger, unmeasured and invalid displays do not warn',()=>{
  for(const args of [
    [{width:800,height:480},800,480,1], [{width:1600,height:960},800,480,2],
    [{width:1600,height:960},799.999,479.999,2], [{width:800,height:480},800.001,480.001,1],
    [undefined,800,480,2], [{width:800,height:480},0,480,1],
    [{width:800,height:480},800,NaN,1], [{width:800,height:480},800,480,Infinity],
  ])assert.equal(panelResolutionWarning(...args),'');
});
test('window resizing, page changes and display scaling recompute the warning',()=>{
  const small={width:800,height:480},large={width:1600,height:960};
  assert.equal(panelResolutionWarning(small,800,480,1),'');
  assert.match(panelResolutionWarning(small,1000,600,1),/1000 × 600/);
  assert.match(panelResolutionWarning(small,800,480,1.5),/1200 × 720/);
  assert.equal(panelResolutionWarning(large,800,480,1.5),'');
});
