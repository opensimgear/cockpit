import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-canvas-render']);
const {canvasRouteNodes,canvasPreviewRoutes}=modules.get('./profile-canvas-render');
const {diagramSmoothPath,diagramOrthogonalPath}=modules.get('./profile-diagram');
const plain=value=>JSON.parse(JSON.stringify(value));
const node=(id,x,y)=>({id,position:{x,y},width:100,height:100});
const binding=(id,from,to)=>({id,from:{nodeId:from,portId:'out'},to:{nodeId:to,portId:'in'}});
const route=(id,from,to,points)=>({binding:binding(id,from,to),points,path:diagramOrthogonalPath(points,24),cornerRadius:24,manual:true,customBlocked:false});
const setup=()=>({nodes:[node('a',0,0),node('b',400,200),node('c',0,500),node('d',400,500)],routes:[route('ab','a','b',[{x:100,y:80},{x:250,y:80},{x:250,y:280},{x:400,y:280}]),route('cd','c','d',[{x:100,y:580},{x:400,y:580}])]});
function frozen(value){if(value&&typeof value==='object'){Object.freeze(value);for(const child of Object.values(value))frozen(child);}return value;}
function assertOrthogonal(points){for(let i=1;i<points.length;i++)assert.ok((points[i].x===points[i-1].x)!==(points[i].y===points[i-1].y),'Every segment is nonempty and axis aligned');}

test('routing geometry contains only card bounds and named transform anchors at exact border centers',()=>{
  const cards=[{id:'resource',name:'Inputs',kind:'resource',category:'Controller',position:{x:10,y:20},observation:{large:'live data'}},{id:'gate',name:'Gate',kind:'processing',category:'Transform',position:{x:400,y:70},processing:{inputs:[{id:'value'},{id:'gate'}],outputs:[{id:'result'}]}}];
  const geometry=canvasRouteNodes(frozen(cards));
  assert.deepEqual(plain(geometry),[{id:'resource',position:{x:10,y:20},width:288,height:188},{id:'gate',position:{x:400,y:70},width:224,height:142,inputAnchors:{value:{x:400,y:162},gate:{x:400,y:190}},outputAnchors:{result:{x:624,y:162}}}]);
  assert.notEqual(geometry[0].position,cards[0].position);
});

test('unchanged geometry reuses the whole route array and changing one card retains unrelated wire objects',()=>{
  const {nodes,routes}=setup();frozen(nodes);frozen(routes);const before=plain(routes);
  assert.equal(canvasPreviewRoutes(routes,nodes,100,100,'orthogonal'),routes);
  const moved=nodes.map(row=>row.id==='a'?{...row,position:{x:24,y:35}}:row);
  const preview=canvasPreviewRoutes(routes,moved,100,100,'orthogonal');
  assert.notEqual(preview,routes);assert.notEqual(preview[0],routes[0]);assert.equal(preview[1],routes[1]);assert.equal(preview[0].binding,routes[0].binding);
  assert.deepEqual(plain(preview[0].points[0]),{x:124,y:115});assert.deepEqual(plain(preview[0].points.at(-1)),{x:400,y:280});assert.deepEqual(plain(routes),before);assertOrthogonal(preview[0].points);
});

test('named input and output anchors override card footers independently for every incident port',()=>{
  const source={...node('a',0,0),outputAnchors:{out:{x:224,y:92},other:{x:224,y:120}}},target={...node('b',500,40),inputAnchors:{in:{x:500,y:132}}};
  const routes=[route('ab','a','b',[{x:224,y:92},{x:350,y:92},{x:350,y:132},{x:500,y:132}])];
  const moved=[{...source,outputAnchors:{out:{x:254,y:132},other:{x:254,y:160}}},{...target,inputAnchors:{in:{x:480,y:162}}}];
  const preview=canvasPreviewRoutes(frozen(routes),moved,288,188,'smooth');
  assert.deepEqual(plain(preview[0].points[0]),{x:254,y:132});assert.deepEqual(plain(preview[0].points.at(-1)),{x:480,y:162});
  assert.equal(preview[0].path,diagramSmoothPath(preview[0].points,preview[0].points));
  assert.match(preview[0].path,/ C/);assert.doesNotMatch(preview[0].path,/ [LA]/);
});

test('moving both cards by the group delta carries all existing bends without changing authored data',()=>{
  const {nodes,routes}=setup(),before=plain(routes),dx=-60,dy=90;
  const moved=nodes.map(row=>['a','b'].includes(row.id)?{...row,position:{x:row.position.x+dx,y:row.position.y+dy}}:row);
  const preview=canvasPreviewRoutes(frozen(routes),moved,100,100,'orthogonal',80);
  assert.deepEqual(plain(preview[0].points),routes[0].points.map(point=>({x:point.x+dx,y:point.y+dy})));
  assert.equal(preview[0].path,diagramOrthogonalPath(preview[0].points,24));assert.match(preview[0].path,/ A24,24 /);
  assert.equal(preview[0].manual,true);assert.equal(preview[0].customBlocked,false);assert.deepEqual(plain(routes),before);
});

test('a group drag translates the committed cubic fit exactly instead of fitting its guide again',()=>{
  const nodes=[node('a',0,0),node('b',400,200)],points=[{x:100,y:80},{x:250,y:80},{x:250,y:280},{x:400,y:280}];
  const paths=[
    ['M100,80 C166.5,80 309.25,280 400,280','M155,70 C221.5,70 364.25,270 455,270'],
    ['M100,80 C120,80 120,120 160,120 C200,120 200,280 400,280','M155,70 C175,70 175,110 215,110 C255,110 255,270 455,270']
  ];
  for(const [path,expected]of paths){
    const routes=frozen([{...route('ab','a','b',points),path,manual:false}]),before=plain(routes);
    const moved=nodes.map(row=>({...row,position:{x:row.position.x+55,y:row.position.y-10}}));
    const preview=canvasPreviewRoutes(routes,moved,100,100,'smooth');
    assert.equal(preview[0].path,expected,'Both direct and detoured fits keep their exact shape and cubic count');
    assert.deepEqual(plain(preview[0].points),points.map(point=>({x:point.x+55,y:point.y-10})));
    assert.deepEqual(plain(routes),before);
  }
});

test('a single endpoint drag reattaches a committed one-cubic fit without adding bends',()=>{
  const nodes=[node('a',0,0),node('b',400,200)],points=[{x:100,y:80},{x:250,y:80},{x:250,y:280},{x:400,y:280}];
  const routes=frozen([{...route('ab','a','b',points),path:'M100,80 C166.5,80 309.25,280 400,280',manual:false}]);
  const source=canvasPreviewRoutes(routes,nodes.map(row=>row.id==='a'?{...row,position:{x:25,y:35}}:row),100,100,'smooth');
  assert.equal(source[0].path,'M125,115 C191.5,115 309.25,280 400,280');
  const target=canvasPreviewRoutes(routes,nodes.map(row=>row.id==='b'?{...row,position:{x:360,y:220}}:row),100,100,'smooth');
  assert.equal(target[0].path,'M100,80 C166.5,80 269.25,300 360,300');
});

test('dragging either endpoint keeps assigned parallel smooth lanes distinct',()=>{
  const nodes=[node('a',0,0),node('b',500,0)];
  const routes=frozen([-60,-20,20,60].map((offset,index)=>{
    const points=[{x:100,y:80},{x:120,y:80},{x:120,y:80+offset},{x:480,y:80+offset},{x:480,y:80},{x:500,y:80}];
    return {...route(`ab-${index}`,'a','b',points),manual:false,path:diagramSmoothPath(points)};
  }));
  for(const id of ['a','b']){
    const moved=nodes.map(row=>row.id===id?{...row,position:{x:row.position.x+10,y:row.position.y+15}}:row);
    const preview=canvasPreviewRoutes(routes,moved,100,100,'smooth');
    assert.equal(new Set(preview.map(wire=>wire.path)).size,routes.length,'Distinct lanes must not collapse into a shared endpoint curve during drag');
    for(const wire of preview){assertOrthogonal(wire.points);assert.doesNotMatch(wire.path,/NaN|Infinity/);}
  }
});

test('backward and same-height edges retain outward leads and rounded, finite nonempty segments',()=>{
  const routes=[route('ab','a','b',[{x:100,y:80},{x:400,y:80}])];
  for(const targetY of [0,80,-80]){
    const preview=canvasPreviewRoutes(routes,[node('a',400,0),node('b',0,targetY)],100,100,'orthogonal',8),points=preview[0].points;
    assert.deepEqual(plain(points[0]),{x:500,y:80});assert.deepEqual(plain(points.at(-1)),{x:0,y:targetY+80});
    assert.ok(points[1].x>500);assert.ok(points.at(-2).x<0);assertOrthogonal(points);
    assert.equal(preview[0].path,diagramOrthogonalPath(points,8));assert.doesNotMatch(preview[0].path,/NaN|Infinity/);
  }
});

test('a dense 73-route drag regenerates only incident paths and performs no full router call',async()=>{
  const nodes=Array.from({length:74},(_,index)=>node(`node-${index}`,index?500:0,index*24));
  const routes=Array.from({length:73},(_,index)=>({...route(`wire-${index}`,'node-0',`node-${index+1}`,[{x:100,y:80},{x:300,y:80},{x:300,y:(index+1)*24+80},{x:500,y:(index+1)*24+80}]),manual:false}));
  const moved=nodes.map(row=>row.id==='node-37'?{...row,position:{x:530,y:row.position.y+17}}:row),snapshot=plain(routes);
  const preview=canvasPreviewRoutes(frozen(routes),moved,100,100,'smooth');
  assert.equal(preview.filter((wire,index)=>wire!==routes[index]).length,1);assert.notEqual(preview[36].path,routes[36].path);assert.deepEqual(plain(routes),snapshot);
  const source=await readFile(new URL('./profile-canvas-render.ts',import.meta.url),'utf8');
  assert.doesNotMatch(source,/\bdiagramRoutes\s*\(/,'Drag preview cannot invoke collision search');
});

test('temporarily absent endpoints retain their existing paths without mutating the routing cache',()=>{
  const {nodes,routes}=setup();assert.equal(canvasPreviewRoutes(frozen(routes),nodes.filter(row=>row.id!=='a'),100,100,'smooth'),routes);
});
