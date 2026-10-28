import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-overview-merges','./profile-overview-routing','./profile-route-preview']);
const {mergeOverviewRoutes,createOverviewMerger}=modules.get('./profile-overview-merges');
const {canvasPreviewRoutes}=modules.get('./profile-route-preview');
const {routeOverview}=modules.get('./profile-overview-routing');
const plain=value=>JSON.parse(JSON.stringify(value));
const node=(id,x,y)=>({id,position:{x,y},width:100,height:80});
const binding=(id,from,to='target',portId='input')=>({id,from:{nodeId:from,portId:'output'},to:{nodeId:to,portId}});
function setup(count=2,backward=false){
  const nodes=[...Array.from({length:count},(_,i)=>node(`source${i}`,backward?850:0,i*180)),node('target',600,180)];
  const bindings=Array.from({length:count},(_,i)=>binding(`edge${i}`,`source${i}`));
  return {nodes,bindings};
}
function merge(nodes,bindings,style='smooth',preview=false){
  const routes=routeOverview(nodes,bindings,100,80,{style,clearance:24});
  assert.equal(routes.length,bindings.length,'fixture must contain every original route');
  return {original:routes,...mergeOverviewRoutes(routes,nodes,100,80,style,24,preview)};
}
function missesCards(points,nodes){
  for(let i=1;i<points.length;i++){
    const a=points[i-1],b=points[i];
    for(const node of nodes){
      const {x,y}=node.position;
      if(a.x===b.x)assert.ok(a.x<=x||a.x>=x+node.width||Math.max(a.y,b.y)<=y||Math.min(a.y,b.y)>=y+node.height,`${node.id}: vertical intersection`);
      else assert.ok(a.y<=y||a.y>=y+node.height||Math.max(a.x,b.x)<=x||Math.min(a.x,b.x)>=x+node.width,`${node.id}: horizontal intersection`);
    }
  }
}
function curveMissesCards(path,nodes){
  const values=path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number);
  let a={x:values[0],y:values[1]};
  for(let i=2;i+5<values.length;i+=6){
    const [bx,by,cx,cy,dx,dy]=values.slice(i,i+6);
    for(let sample=0;sample<=64;sample++){
      const t=sample/64,s=1-t,x=s*s*s*a.x+3*s*s*t*bx+3*s*t*t*cx+t*t*t*dx,y=s*s*s*a.y+3*s*s*t*by+3*s*t*t*cy+t*t*t*dy;
      for(const node of nodes)assert.ok(x<=node.position.x||x>=node.position.x+node.width||y<=node.position.y||y>=node.position.y+node.height,`${node.id}: curve intersection`);
    }
    a={x:dx,y:dy};
  }
}
for(const style of ['smooth','orthogonal'])for(const count of [2,3])for(const backward of [false,true]){
  test(`${style}: ${count} ${backward?'backward':'forward'} incoming arrows share an early final stem`,()=>{
    const {nodes,bindings}=setup(count,backward),before=plain({nodes,bindings});
    const result=merge(nodes,bindings,style);
    assert.equal(result.merges.length,1);
    const trunk=result.merges[0];
    assert.ok(trunk.endpoint.x-trunk.junction.x===60);
    assert.equal(trunk.members.length,count);
    result.routes.forEach((route,index)=>{
      assert.strictEqual(route.binding,result.original[index].binding);
      assert.deepEqual(plain(route.points.at(-1)),plain(trunk.junction));
      missesCards(route.points,nodes);
      if(style==='smooth')curveMissesCards(route.path,nodes);
    });
    missesCards(trunk.points,nodes);
    assert.deepEqual(plain({nodes,bindings}),before);
  });
}
test('a single arrow and empty routes retain their original geometry',()=>{
  const {nodes,bindings}=setup(1),{original,routes,merges}=merge(nodes,bindings);
  assert.equal(merges.length,0);assert.strictEqual(routes[0],original[0]);
  assert.deepEqual(plain(mergeOverviewRoutes([],nodes,100,80,'smooth',24)),{routes:[],merges:[]});
});
test('different visible input anchors and destination nodes remain separate',()=>{
  const {nodes,bindings}=setup();
  nodes[2].inputAnchors={input:{x:600,y:240},second:{x:600,y:210}};
  bindings[1].to.portId='second';
  assert.equal(merge(nodes,bindings).merges.length,0);
  nodes.push(node('other',600,500));bindings[1].to.nodeId='other';
  assert.equal(merge(nodes,bindings).merges.length,0);
});
test('an intervening card routes branches around it and shortens only a constrained stem',()=>{
  const {nodes,bindings}=setup(3);nodes.push(node('obstacle',440,180));
  const result=merge(nodes,bindings,'orthogonal');
  assert.equal(result.merges.length,1);
  assert.ok(result.merges[0].endpoint.x-result.merges[0].junction.x<60);
  for(const route of result.routes)missesCards(route.points,nodes);
  missesCards(result.merges[0].points,nodes);
});
test('cheap preview preserves bindings and never mutates its existing guides',()=>{
  const {nodes,bindings}=setup(3),original=routeOverview(nodes,bindings,100,80,{style:'orthogonal'}),before=plain(original);
  const result=mergeOverviewRoutes(original,nodes,100,80,'orthogonal',24,true);
  assert.equal(result.merges.length,1);
  result.routes.forEach((route,index)=>{assert.strictEqual(route.binding,original[index].binding);missesCards(route.points,nodes);});
  assert.deepEqual(plain(original),before);
});
test('smooth preview either keeps its safe original route or safely reattaches to one stem',()=>{
  const {nodes,bindings}=setup(3),original=routeOverview(nodes,bindings,100,80,{style:'smooth'});
  const result=mergeOverviewRoutes(original,nodes,100,80,'smooth',24,true);
  for(const route of result.routes)curveMissesCards(route.path,nodes);
  if(result.merges.length)for(const route of result.routes)assert.deepEqual(plain(route.points.at(-1)),plain(result.merges[0].junction));
});
test('per-canvas merger reuses committed geometry and invalidates changed geometry',()=>{
  const {nodes,bindings}=setup(),routes=routeOverview(nodes,bindings,100,80,{style:'smooth'}),merger=createOverviewMerger();
  const first=merger(routes,nodes,100,80,'smooth',24);
  assert.strictEqual(merger(routes,nodes,100,80,'smooth',24),first);
  assert.strictEqual(merger(routes,plain(nodes),100,80,'smooth',24),first,'equal replacement nodes reuse geometry');
  const preview=merger(routes,nodes,100,80,'smooth',24,true);
  assert.strictEqual(preview,first);
  assert.strictEqual(merger(routes,nodes,100,80,'smooth',24),first,'preview cannot evict committed geometry');
  for(const args of [
    [routes,nodes,100,80,'orthogonal',24],
    [routes,nodes,100,80,'smooth',12],
    [routes,nodes,101,80,'smooth',24],
    [routes,nodes.map(n=>({...n,position:{...n.position,y:n.position.y+1}})),100,80,'smooth',24],
    [routes.map(r=>({...r,path:r.path+' '})),nodes,100,80,'smooth',24],
  ])assert.notStrictEqual(merger(...args),first);
});
test('equal replacement routes reuse path objects but refresh all binding and member identities',()=>{
  const {nodes,bindings}=setup(),routes=routeOverview(nodes,bindings,100,80,{style:'smooth'}),merger=createOverviewMerger();
  const first=merger(routes,nodes,100,80,'smooth',24),replacement=routes.map(route=>({...route,binding:{...route.binding,name:'current binding'}}));
  const second=merger(replacement,nodes,100,80,'smooth',24);
  assert.notStrictEqual(second,first);
  second.routes.forEach((route,index)=>{
    assert.strictEqual(route.binding,replacement[index].binding);
    assert.strictEqual(route.points,first.routes[index].points,'cached routing reused');
  });
  second.merges[0].members.forEach((route,index)=>assert.strictEqual(route,replacement[index]));
  assert.strictEqual(merger(replacement,nodes,100,80,'smooth',24),second);
  const single=createOverviewMerger(),one=single(routes.slice(0,1),nodes,100,80,'smooth',24);
  const refreshed=single(replacement.slice(0,1),nodes,100,80,'smooth',24);
  assert.strictEqual(refreshed.routes[0],replacement[0]);assert.notStrictEqual(one,refreshed);
});

for(const style of ['smooth','orthogonal'])for(const backward of [false,true])for(const movedId of ['source0','target']){
  test(style+': repeated '+(backward?'backward ':'forward ')+movedId+' drag retains committed merge membership and tangents',()=>{
    const {nodes,bindings}=setup(3,backward),routes=routeOverview(nodes,bindings,100,80,{style}),merger=createOverviewMerger();
    const first=merger(routes,nodes,100,80,style,24),snapshot=plain(first);
    assert.equal(first.merges.length,1);
    for(const [dx,dy] of [[5,10],[20,35],[-10,-20],[50,60],[0,0]]){
      const moved=nodes.map(n=>n.id===movedId?{...n,position:{x:n.position.x+dx,y:n.position.y+dy}}:n);
      const input=canvasPreviewRoutes(routes,moved,100,80,style);
      const next=merger(input,moved,100,80,style,24,true),stem=next.merges[0];
      assert.equal(next.merges.length,1);
      assert.equal(stem.id,first.merges[0].id);
      assert.deepEqual(stem.members.map(r=>r.binding.id),first.merges[0].members.map(r=>r.binding.id));
      assert.equal(stem.endpoint.x-stem.junction.x,60);
      assert.deepEqual(plain(stem.endpoint),plain(input[0].points.at(-1)));
      for(let index=0;index<next.routes.length;index++){
        const route=next.routes[index];
        assert.deepEqual(plain(route.points[0]),plain(input[index].points[0]));
        assert.deepEqual(plain(route.points.at(-1)),plain(stem.junction));
        assert.doesNotMatch(route.path,/NaN|Infinity/);
        for(let i=1;i<route.points.length;i++)assert.ok((route.points[i].x===route.points[i-1].x)!==(route.points[i].y===route.points[i-1].y));
        if(style==='smooth'){
          const values=route.path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number);
          assert.equal(values.at(-3),values.at(-1),'Horizontal arrival tangent');
          assert.ok(values.at(-4)<values.at(-2),'Arrival proceeds toward the junction');
          assert.equal((route.path.match(/C/g)||[]).length,(first.routes[index].path.match(/C/g)||[]).length,'Retain committed cubic topology');
        }
      }
      if(!dx&&!dy)assert.strictEqual(next,first,'No motion reuses committed geometry');
    }
    assert.deepEqual(plain(first),snapshot,'Preview must leave committed cache intact');
    assert.strictEqual(merger(routes,nodes,100,80,style,24),first);
  });
}
for(const style of ['smooth','orthogonal'])test(style+': group drag translates merged shape exactly',()=>{
  const {nodes,bindings}=setup(3),routes=routeOverview(nodes,bindings,100,80,{style}),merger=createOverviewMerger();
  const first=merger(routes,nodes,100,80,style,24),dx=35,dy=-20;
  const moved=nodes.map(n=>({...n,position:{x:n.position.x+dx,y:n.position.y+dy}}));
  const next=merger(canvasPreviewRoutes(routes,moved,100,80,style),moved,100,80,style,24,true);
  next.routes.forEach((route,index)=>{
    assert.deepEqual(plain(route.points),plain(first.routes[index].points).map(p=>({x:p.x+dx,y:p.y+dy})));
    const coordinates=path=>path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number);
    if(style==='smooth')assert.deepEqual(coordinates(route.path),coordinates(first.routes[index].path).map((n,i)=>n+(i%2?dy:dx)));
  });
  assert.deepEqual(plain(next.merges[0].points),plain(first.merges[0].points).map(p=>({x:p.x+dx,y:p.y+dy})));
});
