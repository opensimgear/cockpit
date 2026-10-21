import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-diagram-simplify','./profile-diagram-lanes']);
const {simplifyDiagramRoute}=modules.get('./profile-diagram-simplify');
const {diagramSimpleRoute}=modules.get('./profile-diagram-lanes');
const path=coordinates=>coordinates.map(([x,y])=>({x,y}));
function validator(original,boxes=[],extra=()=>true){
  return points=>{
    if(!diagramSimpleRoute(points))return false;
    const first=points[0],last=points.at(-1),before=points.at(-2);
    if(first.x!==original[0].x||first.y!==original[0].y||last.x!==original.at(-1).x||last.y!==original.at(-1).y)return false;
    if(points[1].y!==first.y||points[1].x<=first.x||before.y!==last.y||before.x>=last.x)return false;
    if(points.slice(1).some((b,index)=>{
      const a=points[index];
      return boxes.some(box=>a.y===b.y?a.y>box.top&&a.y<box.bottom&&Math.max(a.x,b.x)>box.left&&Math.min(a.x,b.x)<box.right:a.x>box.left&&a.x<box.right&&Math.max(a.y,b.y)>box.top&&Math.min(a.y,b.y)<box.bottom);
    }))return false;
    return extra(points);
  };
}

test('an unobstructed same-height detour becomes one leg without mutating its guide',()=>{
  const original=path([[0,0],[20,0],[20,-80],[280,-80],[280,0],[300,0]]),snapshot=structuredClone(original);
  for(const point of original)Object.freeze(point);Object.freeze(original);
  assert.deepEqual(JSON.parse(JSON.stringify(simplifyDiagramRoute(original,validator(original)))),path([[0,0],[300,0]]));
  assert.deepEqual(original,snapshot);
});

test('a stepped route between different heights keeps only H-V-H and valid endpoint tangents',()=>{
  const original=path([[0,0],[20,0],[20,40],[70,40],[70,80],[280,80],[280,120],[300,120]]);
  const result=simplifyDiagramRoute(original,validator(original));
  assert.equal(result.length-1,3);assert.ok(validator(original)(result));
  assert.equal(result.reduce((sum,point,index)=>index?sum+Math.abs(point.x-result[index-1].x)+Math.abs(point.y-result[index-1].y):sum,0),420);
});

test('a centered obstacle keeps the five necessary legs while extra detour bends disappear',()=>{
  const original=path([[0,0],[30,0],[30,-70],[70,-70],[70,-90],[210,-90],[210,-70],[270,-70],[270,0],[300,0]]);
  const clear=validator(original,[{left:120,right:180,top:-30,bottom:30}]),result=simplifyDiagramRoute(original,clear);
  assert.equal(result.length-1,5);assert.ok(clear(result));
});

test('a route already at its minimum keeps the original instance when every shortcut is blocked',()=>{
  const original=path([[0,0],[30,0],[30,-70],[270,-70],[270,0],[300,0]]);
  assert.equal(simplifyDiagramRoute(original,validator(original,[{left:120,right:180,top:-30,bottom:30}])),original);
});

test('full candidate restrictions are respected even when a direct path is obstacle-free',()=>{
  const original=path([[0,0],[20,0],[20,-80],[70,-80],[70,-100],[150,-100],[150,-80],[280,-80],[280,0],[300,0]]);
  const throughRequiredLane=points=>points.some((a,index)=>{const b=points[index+1];return b&&a.y===-80&&b.y===-80&&Math.min(a.x,b.x)<=150&&Math.max(a.x,b.x)>=150;});
  const clear=validator(original,[],throughRequiredLane),result=simplifyDiagramRoute(original,clear);
  assert.equal(result.length-1,5);assert.ok(clear(result));assert.ok(throughRequiredLane(result));
});

test('a bounded pass does not repeatedly query a validator that rejects every shortcut',()=>{
  const original=path(Array.from({length:128},(_,index)=>[Math.floor(index/2)*10,index%2?10:0]));
  let checks=0;
  assert.equal(simplifyDiagramRoute(original,()=>{checks++;return false;}),original);
  assert.ok(checks<=256,`Expected bounded full-route validation, received ${checks}`);
});
