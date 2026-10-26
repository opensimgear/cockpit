import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./game-background-crop']),crop=modules.get('./game-background-crop');
const within=(frame,w,h)=>{assert.ok(frame.x>=0&&frame.y>=0);assert.ok(frame.x+frame.width<=1+1e-9&&frame.y+frame.height<=1+1e-9);assert.ok(frame.width>0&&frame.height>0);assert.ok(Math.abs(frame.width*w/(frame.height*h)-16/9)<1e-9);};
test('portrait, landscape and square crop frames preserve tile shape inside every image edge',()=>{
  for(const [w,h] of [[1920,1080],[600,1600],[1600,600],[800,800],[460,215],[920,430],[384,179],[1,8192],[8192,1]]){
    const initial=crop.maximumCrop(w,h);within(initial,w,h);assert.ok(initial.width===1||Math.abs(initial.height-1)<1e-9);
    for(const zoom of [1,2,8,100,-1])for(const dx of [-100,0,.3,100])for(const dy of [-100,0,.3,100]){
      const frame=crop.moveCrop(crop.zoomCrop(initial,zoom,w,h),dx,dy,w,h);within(frame,w,h);
      within(crop.resizeCrop(frame,dx,dy,w,h),w,h);
      for(const corner of ['nw','ne','sw','se'])within(crop.resizeCropCorner(frame,dx,dy,w,h,corner),w,h);
    }
  }
});

test('Fit image emits exact bounded edges for Steam header dimensions accepted by native crop validation',()=>{
  for(const [w,h] of [[460,215],[920,430],[384,179],[640,320],[1280,640],[9,5]]){
    const fit=crop.maximumCrop(w,h);within(fit,w,h);assert.equal(fit.y,0);assert.equal(fit.height,1);
    for(const frame of [fit,crop.positionedCrop(w,h,50,50),crop.boundedCrop({...fit,y:-Number.EPSILON,height:1+Number.EPSILON},w,h)]){
      within(frame,w,h);assert.equal(frame.y,0);assert.equal(frame.height,1);
    }
  }
});
test('each dragged corner keeps its opposite corner anchored',()=>{
  const frame=crop.zoomCrop(crop.maximumCrop(1600,900),2,1600,900);
  for(const corner of ['nw','ne','sw','se']){
    const changed=crop.resizeCropCorner(frame,.05,.05,1600,900,corner);
    const anchor=box=>[corner.endsWith('e')?box.x:box.x+box.width,corner.startsWith('s')?box.y:box.y+box.height];
    assert.deepEqual(anchor(changed),anchor(frame));within(changed,1600,900);
  }
});
test('zoom retains the selected center until image bounds require a clamp and reset fits the whole available tile',()=>{
  const initial=crop.maximumCrop(1920,1080),zoomed=crop.zoomCrop(initial,2,1920,1080);assert.equal(zoomed.width,.5);assert.equal(zoomed.x,.25);assert.equal(zoomed.y,.25);
  const edge=crop.moveCrop(zoomed,10,-10,1920,1080);assert.equal(edge.x,.5);assert.equal(edge.y,0);
  assert.equal(crop.zoomCrop(edge,1,1920,1080).width,1);
  const legacy=crop.positionedCrop(800,1200,0,100);assert.equal(legacy.y+legacy.height,1);
  assert.throws(()=>crop.maximumCrop(0,10));assert.deepEqual(JSON.parse(JSON.stringify(crop.boundedCrop({x:NaN,y:0,width:1,height:1},800,800))),JSON.parse(JSON.stringify(crop.maximumCrop(800,800))));
  assert.equal(crop.cropImageStyle(), '');assert.match(crop.cropImageStyle(zoomed),/height:auto/);assert.match(crop.cropImageStyle(zoomed),/width:200%/);
});
