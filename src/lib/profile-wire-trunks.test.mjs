import test from 'node:test';
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-wire-trunks']);
const {buildDiagramWireTrunks:build}=modules.get('./profile-wire-trunks');
const {diagramRoutes}=modules.get('./profile-diagram');
const {bundleDiagramRoutes}=modules.get('./profile-wire-bundles')??(await loadModules(['./profile-wire-bundles'])).get('./profile-wire-bundles');
const plain=value=>JSON.parse(JSON.stringify(value));
const node=(id,x,y,width=100,height=100)=>({id,position:{x,y},width,height});
const wire=(id,from,to)=>({id,from:{nodeId:from,portId:id},to:{nodeId:to,portId:id}});
function fixture(style='smooth',bindings=[wire('a','source','one'),wire('b','source','two'),wire('c','source','three')],nodes=[node('source',0,200),node('one',500,0),node('two',500,200),node('three',500,400)]){
  const routes=diagramRoutes(nodes,bindings,100,100,{style});
  return {nodes,routes,bundles:bundleDiagramRoutes(routes,true)};
}
function checkEndpoints(before,after){for(const bundle of after){const original=before.find(value=>value.id===bundle.id);assert.deepEqual(plain(bundle.route.points[0]),plain(original.route.points[0]));assert.deepEqual(plain(bundle.route.points.at(-1)),plain(original.route.points.at(-1)));assert.equal(bundle.members,original.members);}}
function trunkMembers(result){return result.trunks.map(trunk=>trunk.members.map(route=>route.binding.id));}

for(const style of ['smooth','orthogonal'])test(`${style} fan-out shares a horizontal stem and a tapped vertical bus`,()=>{
  const f=fixture(style,[wire('a','source','one'),wire('b','source','two'),wire('c','source','three')],[node('source',0,400),node('one',500,0),node('two',500,150),node('three',500,300)]);
  const result=build(f.bundles,f.nodes,100,100,style,24);
  checkEndpoints(f.bundles,result.bundles);
  assert.ok(result.trunks.some(trunk=>trunk.id.endsWith(':stem')&&trunk.members.length===3));
  assert.ok(result.trunks.some(trunk=>trunk.id.includes(':spine:')&&trunk.members.length>=2),'A common vertical bus continues until a branch is needed');
  assert.ok(result.trunks.every(trunk=>trunk.direction==='output'));
  assert.ok(result.bundles.every(bundle=>bundle.route.path!==f.bundles.find(original=>original.id===bundle.id).route.path));
  if(style==='smooth')assert.ok(result.bundles.every(bundle=>!/[LAQ]/.test(bundle.route.path)));
  for(const trunk of result.trunks.filter(trunk=>trunk.id.endsWith(':stem'))){const suffix=trunk.route.path.replace(/^M[^ ]+ /,'');for(const bundle of result.bundles)assert.ok(bundle.route.path.includes(suffix),'Every route draws the exact common stem');}
});

for(const style of ['smooth','orthogonal'])test(`${style} fan-in uses a reversed bus and retains input anchors`,()=>{
  const nodes=[node('sink',500,400),node('one',0,0),node('two',0,150),node('three',0,300)],bindings=[wire('a','one','sink'),wire('b','two','sink'),wire('c','three','sink')],f=fixture(style,bindings,nodes);
  const result=build(f.bundles,nodes,100,100,style,24);
  checkEndpoints(f.bundles,result.bundles);
  assert.ok(result.trunks.some(trunk=>trunk.direction==='input'&&trunk.id.includes(':spine:')));
  assert.ok(result.trunks.find(trunk=>trunk.id.endsWith(':stem')).members.length===3);
  assert.ok(result.trunks.every(trunk=>trunk.route.points.at(-1).x>=trunk.route.points[0].x||trunk.id.includes(':spine:')||trunk.id.includes(':turn:')));
});

test('source and destination bus membership coexist on a route without deleting protected stations',()=>{
  const nodes=[node('source',0,400),node('source-two',0,0),node('sink',600,400),node('sink-two',600,0)];
  const bindings=[wire('a','source','sink'),wire('b','source','sink-two'),wire('c','source-two','sink')],f=fixture('smooth',bindings,nodes);
  const result=build(f.bundles,nodes,100,100,'smooth',24);
  const both=result.bundles.find(bundle=>bundle.id==='a');
  assert.ok(result.trunks.some(trunk=>trunk.direction==='output'&&trunk.members.some(route=>route.binding.id==='a')));
  assert.ok(result.trunks.some(trunk=>trunk.direction==='input'&&trunk.members.some(route=>route.binding.id==='a')));
  checkEndpoints(f.bundles,result.bundles);
  const stem=result.trunks.find(trunk=>trunk.direction==='output'&&trunk.id.endsWith(':stem'));
  assert.ok(both.route.path.includes(stem.route.path.replace(/^M[^ ]+ /,'')));
});

test('a blocked backward branch retains its westbound tangent through the multi-curve fallback',()=>{
  const nodes=[node('source',600,500),node('one',0,0),node('two',0,200),node('blocker',250,-100,150,400)];
  const points=[[[700,580],[740,580],[740,-140],[-40,-140],[-40,80],[0,80]],[[700,580],[760,580],[760,380],[-40,380],[-40,280],[0,280]]];
  const routes=points.map((value,index)=>({binding:wire(index?'b':'a','source',index?'two':'one'),points:value.map(([x,y])=>({x,y})),path:`original-${index}`,cornerRadius:24,manual:false,customBlocked:false}));
  const bundles=bundleDiagramRoutes(routes,true),result=build(bundles,nodes,100,100,'smooth',24);
  assert.ok(result.trunks.some(trunk=>trunk.id.endsWith(':stem')));
  const route=result.bundles.find(bundle=>bundle.id==='a').route,values=route.path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number);
  assert.ok((values.length-2)/6>5,'The card blocks a single central cubic, retaining a multi-curve detour');
  let endpoint={x:values[0],y:values[1]},previous=null;
  for(let index=2;index<values.length;index+=6){
    const first={x:values[index],y:values[index+1]},second={x:values[index+2],y:values[index+3]},end={x:values[index+4],y:values[index+5]};
    if(previous){const incoming={x:endpoint.x-previous.x,y:endpoint.y-previous.y},outgoing={x:first.x-endpoint.x,y:first.y-endpoint.y};assert.ok(incoming.x*outgoing.x+incoming.y*outgoing.y>0,'No cusp or reversal at a composed curve join');assert.ok(Math.abs(incoming.x*outgoing.y-incoming.y*outgoing.x)<1e-6,'The bus and branch share their actual local tangent');}
    previous=second;endpoint=end;
  }
  checkEndpoints(bundles,result.bundles);
});

test('an intervening card prevents a shared bus from cutting through it',()=>{
  const f=fixture('smooth',[wire('a','source','one'),wire('b','source','two')],[node('source',0,200),node('blocker',116,180,160,130),node('one',500,0),node('two',500,400)]);
  const result=build(f.bundles,f.nodes,100,100,'smooth',24);
  checkEndpoints(f.bundles,result.bundles);
  assert.equal(result.trunks.length,0,'No visible shared corridor exists before the adjacent card');
  assert.ok(result.bundles.every((bundle,index)=>bundle===f.bundles[index]));
});

test('named port rows, coincident nodes and opposite directions do not merge',()=>{
  const f=fixture();
  const bundles=f.bundles.slice(0,2).map((bundle,index)=>({...bundle,route:{...bundle.route,points:bundle.route.points.map((point,p)=>p===0?{...point,y:point.y+index*28}:point)}}));
  assert.equal(build(bundles,f.nodes,100,100,'smooth',24).trunks.length,0);
  const different=f.bundles.slice(0,2).map((bundle,index)=>({...bundle,route:{...bundle.route,binding:{...bundle.route.binding,from:{...bundle.route.binding.from,nodeId:index?'other':'source'}}}}));
  assert.equal(build(different,f.nodes,100,100,'smooth',24).trunks.length,0);
});

test('selected and authored signals retain their exact objects and paths',()=>{
  const f=fixture('smooth',[wire('a','source','one'),wire('b','source','two'),wire('c','source','three')]);
  const authored={...f.bundles[0],route:{...f.bundles[0].route,manual:true}};authored.members=[authored.route];
  const bundles=[authored,...f.bundles.slice(1)],result=build(bundles,f.nodes,100,100,'smooth',24,false,'b');
  assert.equal(result.trunks.length,0);
  assert.equal(result.bundles[0],authored);
  assert.equal(result.bundles[1],bundles[1]);
});

test('derivation is deterministic and leaves all saved geometry immutable',()=>{
  const f=fixture(),before=plain(f);
  const freeze=value=>{if(value&&typeof value==='object'){Object.values(value).forEach(freeze);Object.freeze(value);}return value;};freeze(f);
  const first=build(f.bundles,f.nodes,100,100,'smooth',24),second=build(f.bundles,f.nodes,100,100,'smooth',24);
  assert.deepEqual(plain(first),plain(second));assert.deepEqual(plain(f),before);
  assert.deepEqual(plain(trunkMembers(first)),plain(trunkMembers(second)));
});

test('a hundred branches use bounded local corridor checks during dragging',()=>{
  const nodes=[node('source',0,20_000),...Array.from({length:100},(_,index)=>node(`sink-${index}`,600,index*140))];
  const bindings=Array.from({length:100},(_,index)=>wire(`wire-${index}`,'source',`sink-${index}`));
  const f=fixture('smooth',bindings,nodes),start=performance.now(),result=build(f.bundles,nodes,100,100,'smooth',24,true);
  assert.ok(performance.now()-start<1800,'No per-route visibility grid or ELK pass is built');
  assert.ok(result.trunks.some(trunk=>trunk.members.length===100));
  assert.ok(result.trunks.length<210);checkEndpoints(f.bundles,result.bundles);
});
