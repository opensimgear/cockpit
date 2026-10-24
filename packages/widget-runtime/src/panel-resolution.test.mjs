import test from 'node:test';
import assert from 'node:assert/strict';
import {panelResolutionWarning} from './panel-resolution.ts';
test('physical destination pixels account for DPR, rounding and both authored dimensions',()=>{
  assert.match(panelResolutionWarning({width:800,height:480},800,480,2),/1600 × 960/);
  for(const panel of [{width:800,height:960},{width:1600,height:480}])assert.match(panelResolutionWarning(panel,800,480,2),/too small/);
  for(const args of [[{width:1600,height:960},800,480,2],[{width:800,height:480},800.001,480.001,1],[undefined,800,480,1],[{width:800,height:480},NaN,480,1],[{width:800,height:480},800,0,1],[{width:800,height:480},800,480,Infinity]])assert.equal(panelResolutionWarning(...args),'');
});
