import assert from 'node:assert/strict';
import test from 'node:test';
import {loadModules} from './test-module-loader.mjs';

const {routeOverview,createOverviewRouter}= (await loadModules(['./profile-overview-routing'])).get('./profile-overview-routing');
const {diagramRoutes}= (await loadModules(['./profile-diagram'])).get('./profile-diagram');
const binding=(id,from,to)=>({id,from:{nodeId:from,portId:''},to:{nodeId:to,portId:id}});
const nodes=[
  {id:'resource-1',position:{x:43.33331298828125,y:239.40625},width:288,height:188},
  {id:'virtual-1',position:{x:397.33331298828125,y:237.40625},width:288,height:188,inputAnchors:{'completed-route':{x:397.33331298828125,y:405.40625}}},
  {id:'resource-2',position:{x:900,y:500},width:288,height:188},
  {id:'virtual-2',position:{x:1400,y:500},width:288,height:188},
];
const difficult=binding('completed-route','resource-1','virtual-1');
const plain=value=>JSON.parse(JSON.stringify(value));

test('snapshot-sized physical-to-virtual gap routes at fallback clearance for both arrow styles',()=>{
  for(const style of ['smooth','orthogonal']){
    const routes=routeOverview(nodes,[difficult],288,188,{style,clearance:36});
    assert.equal(routes.length,1,style);
    assert.equal(routes[0].binding.id,difficult.id);
    assert.ok(routes[0].points.length>=2,style);
    assert.ok(routes[0].path,style);
    assert.deepEqual(plain(diagramRoutes(nodes,[difficult],288,188,{style,clearance:36})),[],'The fixture requires fallback.');
  }
});

test('fallback retains already-routed paths, orders output by inputs, and routes IDs once',()=>{
  const easy=binding('easy-route','resource-2','virtual-2');
  const input=[difficult,easy,easy];
  const expected=diagramRoutes(nodes,[easy],288,188,{style:'smooth',clearance:36})[0];
  const routes=routeOverview(nodes,input,288,188,{style:'smooth',clearance:36});
  assert.deepEqual(plain(routes.map(route=>route.binding.id)),['completed-route','easy-route']);
  assert.equal(new Set(routes.map(route=>route.binding.id)).size,routes.length);
  assert.deepEqual(plain(routes[1]),plain(expected));
  assert.ok(routes.every(route=>route.points.every(point=>Number.isFinite(point.x)&&Number.isFinite(point.y))));
});

test('missing endpoints and empty input remain omitted without inventing routes',()=>{
  const missing=binding('missing','absent','virtual-1');
  assert.deepEqual(plain(routeOverview(nodes,[missing],288,188,{style:'orthogonal'})),[]);
  assert.deepEqual(plain(routeOverview(nodes,[],288,188,{style:'orthogonal'})),[]);
});

test('overview router reuses immutable geometry and invalidates on nodes, bindings and style changes',()=>{
  const router=createOverviewRouter(),documentBinding=binding('cached','resource-2','virtual-2');
  const first=router(nodes,[documentBinding],288,188,{style:'smooth',clearance:36});
  assert.strictEqual(router(nodes,[documentBinding],288,188,{style:'smooth',clearance:36}),first);
  const nextBinding={...documentBinding,from:{...documentBinding.from}};
  const rebound=router(nodes,[nextBinding],288,188,{style:'smooth',clearance:36});
  assert.notStrictEqual(rebound,first);assert.strictEqual(rebound[0].binding,nextBinding);
  assert.strictEqual(router(nodes,[nextBinding],288,188,{style:'smooth',clearance:36}),rebound);
  const moved=nodes.map(node=>node.id==='resource-2'?{...node,position:{...node.position,x:node.position.x+5}}:node);
  assert.notStrictEqual(router(moved,[nextBinding],288,188,{style:'smooth',clearance:36}),rebound);
  assert.notStrictEqual(router(moved,[nextBinding],288,188,{style:'orthogonal',clearance:36}),router(moved,[nextBinding],288,188,{style:'smooth',clearance:36}));
});

test('overview router bounds its least-recently-used history',()=>{
  const router=createOverviewRouter(1),first=router(nodes,[difficult],288,188,{style:'smooth',clearance:36});
  router(nodes,[binding('other','resource-2','virtual-2')],288,188,{style:'smooth',clearance:36});
  assert.notStrictEqual(router(nodes,[difficult],288,188,{style:'smooth',clearance:36}),first);
});
