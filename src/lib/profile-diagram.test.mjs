import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-editor','./profile-diagram','./profile-canvas']);
const {profileAutoLayout,PROFILE_BLOCK_WIDTH:width,PROFILE_BLOCK_HEIGHT:height}=modules.get('./profile-editor');
const {diagramRoutes}=modules.get('./profile-diagram');
const edge=(from,to,id=`${from}-${to}`)=>({id,from:{nodeId:from,portId:'out'},to:{nodeId:to,portId:'in'}});
const placed=positions=>positions.map(({nodeId,...position})=>({id:nodeId,position}));
const plain=value=>JSON.parse(JSON.stringify(value));
function verifyRoutes(nodes,bindings){
  const routes=diagramRoutes(nodes,bindings,width,height);
  assert.equal(routes.length,bindings.length,'Every saved route stays visible');
  for(const {binding,points,path}of routes){
    const from=nodes.find(node=>node.id===binding.from.nodeId),to=nodes.find(node=>node.id===binding.to.nodeId);
    assert.equal(points[0].x,from.position.x+width);assert.ok(points[0].y>=from.position.y+24&&points[0].y<=from.position.y+height-24);
    assert.equal(points.at(-1).x,to.position.x);assert.ok(points.at(-1).y>=to.position.y+24&&points.at(-1).y<=to.position.y+height-24);
    assert.ok(!/NaN|Infinity/.test(path));
    for(let i=1;i<points.length;i++){
      const a=points[i-1],b=points[i];assert.ok(a.x===b.x||a.y===b.y,'Routes use orthogonal channels');
      for(const {id,position:p}of nodes){
        const crosses=a.x===b.x?a.x>p.x&&a.x<p.x+width&&Math.max(a.y,b.y)>p.y&&Math.min(a.y,b.y)<p.y+height:
          a.y>p.y&&a.y<p.y+height&&Math.max(a.x,b.x)>p.x&&Math.min(a.x,b.x)<p.x+width;
        assert.equal(crosses,false,`${binding.id} crosses block ${id}`);
      }
    }
  }
  for(let i=0;i<routes.length;i++)for(let j=i+1;j<routes.length;j++){
    for(let a=1;a<routes[i].points.length;a++)for(let b=1;b<routes[j].points.length;b++){
      const p=routes[i].points[a-1],q=routes[i].points[a],r=routes[j].points[b-1],s=routes[j].points[b];
      const vertical=p.x===q.x&&r.x===s.x&&Math.abs(p.x-r.x)<1e-6;
      const horizontal=p.y===q.y&&r.y===s.y&&Math.abs(p.y-r.y)<1e-6;
      const overlap=vertical?Math.min(Math.max(p.y,q.y),Math.max(r.y,s.y))-Math.max(Math.min(p.y,q.y),Math.min(r.y,s.y)):
        horizontal?Math.min(Math.max(p.x,q.x),Math.max(r.x,s.x))-Math.max(Math.min(p.x,q.x),Math.min(r.x,s.x)):0;
      assert.ok(overlap<1e-6,`${routes[i].binding.id} and ${routes[j].binding.id} share a connection segment`);
    }
  }
  return routes;
}

test('auto-layout follows signal dependencies regardless of resource array order',()=>{
  const ids=['sink','last','source','first'],bindings=[edge('source','first'),edge('first','last'),edge('last','sink')];
  const positions=profileAutoLayout(ids,bindings),byId=new Map(positions.map(p=>[p.nodeId,p]));
  for(const binding of bindings)assert.ok(byId.get(binding.to.nodeId).x>byId.get(binding.from.nodeId).x+width);
  assert.equal(new Set(positions.map(p=>p.y)).size,1);
  assert.deepEqual(plain(profileAutoLayout(ids,bindings)),plain(positions));
  verifyRoutes(placed(positions),bindings);
});

test('branches, merges and skipped ranks never route through intermediate blocks',()=>{
  const ids=['a','b','c','d','e','sink','isolated'],bindings=[edge('a','b'),edge('a','c'),edge('b','d'),edge('c','e'),edge('d','sink'),edge('e','sink'),edge('a','sink')];
  const positions=profileAutoLayout(ids,bindings),nodes=placed(positions);
  for(const a of nodes)for(const b of nodes)if(a.id!==b.id)assert.ok(Math.abs(a.position.x-b.position.x)>=width||Math.abs(a.position.y-b.position.y)>=height);
  verifyRoutes(nodes,bindings);
  assert.ok(nodes.find(n=>n.id==='isolated').position.y>Math.max(...nodes.filter(n=>n.id!=='isolated').map(n=>n.position.y+height)));
});

test('manual obstacles, backwards connections and feedback loops retain clear routes',()=>{
  const nodes=[{id:'a',position:{x:0,y:100}},{id:'obstacle',position:{x:430,y:100}},{id:'sink',position:{x:860,y:100}},{id:'upper',position:{x:430,y:-220}}];
  const routes=verifyRoutes(nodes,[edge('a','sink'),edge('sink','a'),edge('a','a')]);
  assert.ok(routes[0].points.some(p=>p.y<100||p.y>100+height),'Skip connections must go around the obstacle');
  const bindings=[edge('a','b'),edge('b','a'),edge('b','sink')];
  verifyRoutes(placed(profileAutoLayout(['sink','a','b'],bindings)),bindings);
  assert.deepEqual(plain(diagramRoutes(nodes,[edge('missing','sink')],width,height)),[]);
});

test('complete profile layout preserves every binding and clears all blocks',async()=>{
  const doc=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/complete-rig.json',import.meta.url),'utf8'));
  const ids=[...doc.resources,...doc.processingNodes,...doc.virtualOutputs,...(doc.desktopOutputs??[])].map(node=>node.id);
  const before=JSON.stringify(doc),positions=profileAutoLayout(ids,doc.bindings);
  const start=performance.now();verifyRoutes(placed(positions),doc.bindings);
  assert.ok(performance.now()-start<3000,'Routing a complete profile should remain interactive');
  assert.equal(JSON.stringify(doc),before);
});


test('parallel signals and fan-in/fan-out get distinct attachments and deterministic lanes',()=>{
  const bindings=[...Array.from({length:8},(_,i)=>edge('source','target',`parallel-${i}`)),edge('source','lower','branch'),edge('lower','target','merge')];
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}},{id:'lower',position:{x:450,y:350}}];
  const routes=verifyRoutes(nodes,bindings),parallel=routes.filter(route=>route.binding.id.startsWith('parallel'));
  assert.equal(new Set(parallel.map(route=>route.path)).size,8);
  assert.equal(new Set(parallel.map(route=>route.points[0].y)).size,8);
  assert.equal(new Set(parallel.map(route=>route.points.at(-1).y)).size,8);
  const reversed=new Map(diagramRoutes(nodes,[...bindings].reverse(),width,height).map(route=>[route.binding.id,route.path]));
  for(const route of routes)assert.equal(reversed.get(route.binding.id),route.path);
  verifyRoutes(nodes.map(node=>node.id==='lower'?{...node,position:{x:450,y:-100}}:node),bindings);
});


test('tight saved layouts preserve all separate paths without requiring auto-layout',async()=>{
  const doc=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8'));
  const nodes=modules.get('./profile-canvas').canvasNodes(doc,[],[]);
  verifyRoutes(nodes,doc.bindings);
});


test('joystick axes and direct buttons do not look merged at a shared controller',()=>{
  const nodes=placed([{nodeId:'right',x:0,y:156},{nodeId:'left',x:0,y:468},{nodeId:'roll',x:432,y:0},{nodeId:'pitch',x:432,y:312},{nodeId:'collective',x:432,y:624},{nodeId:'controller',x:864,y:312}]);
  const bindings=[edge('right','roll'),edge('right','pitch'),edge('right','controller'),edge('left','collective'),edge('left','controller'),edge('roll','controller'),edge('pitch','controller'),edge('collective','controller')];
  const routes=verifyRoutes(nodes,bindings),verticalOwners=new Map();
  for(const route of routes)for(let i=1;i<route.points.length;i++){
    const a=route.points[i-1],b=route.points[i];if(a.x!==b.x)continue;
    assert.ok(!verticalOwners.has(a.x)||verticalOwners.get(a.x)===route.binding.id,'Separate connections need separate bend columns');
    verticalOwners.set(a.x,route.binding.id);
  }
  const inputHeights=routes.filter(route=>route.binding.to.nodeId==='controller').map(route=>route.points.at(-1).y).sort((a,b)=>a-b);
  for(let i=1;i<inputHeights.length;i++)assert.ok(inputHeights[i]-inputHeights[i-1]>=20);
});
