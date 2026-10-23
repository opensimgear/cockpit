import test from 'node:test';
import assert from 'node:assert/strict';
import {panelResolutionWarning} from './panel-resolution.ts';

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
