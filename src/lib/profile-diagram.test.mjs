import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-editor','./profile-diagram']);
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
    assert.deepEqual(plain(points[0]),{x:from.position.x+width,y:from.position.y+height/2});
    assert.deepEqual(plain(points.at(-1)),{x:to.position.x,y:to.position.y+height/2});
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
