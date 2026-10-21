import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-editor','./profile-diagram','./profile-canvas']);
const {profileAutoLayout,PROFILE_BLOCK_WIDTH:width,PROFILE_BLOCK_HEIGHT:height}=modules.get('./profile-editor');
const {diagramRoutes,createDiagramRouter,diagramPortAnchor,diagramPendingPath,diagramSmoothPath,diagramOrthogonalPath,diagramSegmentLine,diagramRouteQuality}=modules.get('./profile-diagram');
const {diagramAutomaticCorners,diagramSimpleRoute}=modules.get('./profile-diagram-lanes');
const edge=(from,to,id=`${from}-${to}`)=>({id,from:{nodeId:from,portId:'out'},to:{nodeId:to,portId:'in'}});
const placed=positions=>positions.map(({nodeId,...position})=>({id:nodeId,position}));
const plain=value=>JSON.parse(JSON.stringify(value));
function cubics(path){
  assert.doesNotMatch(path,/[LQAHV]/,'Smooth paths contain only cubic Beziers');
  const numbers=path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number),result=[];
  let start={x:numbers[0],y:numbers[1]};
  for(let i=2;i<numbers.length;i+=6){const curve={start,first:{x:numbers[i],y:numbers[i+1]},second:{x:numbers[i+2],y:numbers[i+3]},end:{x:numbers[i+4],y:numbers[i+5]}};result.push(curve);start=curve.end;}
  return result;
}
function sample({start:a,first:b,second:c,end:d},t){const s=1-t;return{x:s*s*s*a.x+3*s*s*t*b.x+3*s*t*t*c.x+t*t*t*d.x,y:s*s*s*a.y+3*s*s*t*b.y+3*s*t*t*c.y+t*t*t*d.y};}
function roundedSamples(path){
  const samples=[];let start;
  for(const [,command,parameters]of path.matchAll(/([MLA])([^MLA]*)/g)){
    const values=parameters.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi).map(Number),end={x:values.at(-2),y:values.at(-1)};
    if(command==='A'){
      const sweep=values[4],sign=sweep?1:-1,center={x:(start.x+end.x-sign*(end.y-start.y))/2,y:(start.y+end.y+sign*(end.x-start.x))/2},angle=Math.atan2(start.y-center.y,start.x-center.x);
      for(let step=0;step<=100;step++){const at=angle+sign*Math.PI/2*step/100;samples.push({x:center.x+values[0]*Math.cos(at),y:center.y+values[0]*Math.sin(at)});}
    }else if(start)for(let step=0;step<=10;step++)samples.push({x:start.x+(end.x-start.x)*step/10,y:start.y+(end.y-start.y)*step/10});
    start=end;
  }
  return samples;
}
function assertClearSamples(nodes,path){
  for(const point of roundedSamples(path))for(const {id,position:p,width:nodeWidth=100,height:nodeHeight=100}of nodes)assert.ok(!(point.x>p.x+1e-7&&point.x<p.x+nodeWidth-1e-7&&point.y>p.y+1e-7&&point.y<p.y+nodeHeight-1e-7),`Rounded arc crosses block ${id} at ${JSON.stringify(point)}`);
}
function assertNoSelfCrossings(points,label){
  const distinct=points.filter((point,index)=>!index||Math.hypot(point.x-points[index-1].x,point.y-points[index-1].y)>1e-7);
  for(let i=1;i<distinct.length;i++)for(let j=1;j<i-1;j++){
    const a=distinct[j-1],b=distinct[j],c=distinct[i-1],d=distinct[i],rx=b.x-a.x,ry=b.y-a.y,sx=d.x-c.x,sy=d.y-c.y,qx=c.x-a.x,qy=c.y-a.y,denominator=rx*sy-ry*sx;
    if(Math.abs(denominator)<1e-8){
      if(Math.abs(qx*ry-qy*rx)>1e-6)continue;
      const length=Math.hypot(rx,ry),from=(qx*rx+qy*ry)/length,to=from+(sx*rx+sy*ry)/length,overlap=Math.min(length,Math.max(from,to))-Math.max(0,Math.min(from,to));
      assert.ok(overlap<1e-6,`${label} retraces its own rendered path`);continue;
    }
    const t=(qx*sy-qy*sx)/denominator,u=(qx*ry-qy*rx)/denominator;
    assert.ok(!(t>=-1e-7&&t<=1+1e-7&&u>=-1e-7&&u<=1+1e-7),`${label} crosses or touches its own rendered path`);
  }
}
function verifyRoutes(nodes,bindings){
  const routes=diagramRoutes(nodes,bindings,width,height);
  assert.equal(routes.length,bindings.length,'Every saved route stays visible');
  for(const {binding,points,path}of routes){
    const from=nodes.find(node=>node.id===binding.from.nodeId),to=nodes.find(node=>node.id===binding.to.nodeId);
    assert.deepEqual(plain(points[0]),plain(from.outputAnchors?.[binding.from.portId]??{x:from.position.x+(from.width??width),y:from.position.y+(from.height??height)-20}));
    assert.deepEqual(plain(points.at(-1)),plain(to.inputAnchors?.[binding.to.portId]??{x:to.position.x,y:to.position.y+(to.height??height)-20}));
    assert.ok(!/NaN|Infinity/.test(path));
    const curves=cubics(path);
    for(const curve of curves)for(let step=1;step<80;step++){
      const point=sample(curve,step/80);
      for(const {id,position:p,width:nodeWidth=width,height:nodeHeight=height}of nodes)assert.ok(!(point.x>p.x+1e-8&&point.x<p.x+nodeWidth-1e-8&&point.y>p.y+1e-8&&point.y<p.y+nodeHeight-1e-8),`${binding.id} Bezier crosses block ${id}`);
    }
    for(let i=1;i<curves.length;i++){
      const previous=curves[i-1],next=curves[i];
      assert.ok(Math.abs(previous.end.x-previous.second.x-(next.first.x-next.start.x))<1e-7);
      assert.ok(Math.abs(previous.end.y-previous.second.y-(next.first.y-next.start.y))<1e-7,'Bezier joins have matching first derivatives');
    }
    for(let i=1;i<points.length;i++){
      const a=points[i-1],b=points[i];assert.ok(a.x===b.x||a.y===b.y,'Routes use orthogonal channels');
      for(const {id,position:p,width:nodeWidth=width,height:nodeHeight=height}of nodes){
        const crosses=a.x===b.x?a.x>p.x&&a.x<p.x+nodeWidth&&Math.max(a.y,b.y)>p.y&&Math.min(a.y,b.y)<p.y+nodeHeight:
          a.y>p.y&&a.y<p.y+nodeHeight&&Math.max(a.x,b.x)>p.x&&Math.min(a.x,b.x)<p.x+nodeWidth;
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


test('parallel signals and fan-in/fan-out share footer anchors and have deterministic separate paths',()=>{
  const bindings=[...Array.from({length:8},(_,i)=>edge('source','target',`parallel-${i}`)),edge('source','lower','branch'),edge('lower','target','merge')];
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}},{id:'lower',position:{x:450,y:350}}];
  const routes=verifyRoutes(nodes,bindings),parallel=routes.filter(route=>route.binding.id.startsWith('parallel'));
  assert.equal(new Set(parallel.map(route=>route.path)).size,8);
  assert.equal(new Set(parallel.map(route=>route.points[0].y)).size,1);
  assert.equal(new Set(parallel.map(route=>route.points.at(-1).y)).size,1);
  const reversed=new Map(diagramRoutes(nodes,[...bindings].reverse(),width,height).map(route=>[route.binding.id,route.path]));
  for(const route of routes)assert.equal(reversed.get(route.binding.id),route.path);
  verifyRoutes(nodes.map(node=>node.id==='lower'?{...node,position:{x:450,y:-100}}:node),bindings);
});


test('tight saved layouts preserve all separate paths without requiring auto-layout',async()=>{
  const doc=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/mixed.json',import.meta.url),'utf8'));
  const nodes=modules.get('./profile-canvas').canvasNodes(doc,[],[]);
  verifyRoutes(nodes,doc.bindings);
  const routes=diagramRoutes(nodes,doc.bindings,width,height,{style:'orthogonal',cornerRadius:100});
  assert.equal(routes.length,doc.bindings.length,'Large radii never close an existing narrow channel');
  for(const route of routes)assertClearSamples(nodes.map(node=>({...node,width:node.width??width,height:node.height??height})),route.path);
});


test('joystick axes and direct buttons use a single common input and output handle per block',()=>{
  const nodes=placed([{nodeId:'right',x:0,y:156},{nodeId:'left',x:0,y:468},{nodeId:'roll',x:432,y:0},{nodeId:'pitch',x:432,y:312},{nodeId:'collective',x:432,y:624},{nodeId:'controller',x:864,y:312}]);
  const bindings=[edge('right','roll'),edge('right','pitch'),edge('right','controller'),edge('left','collective'),edge('left','controller'),edge('roll','controller'),edge('pitch','controller'),edge('collective','controller')];
  const routes=verifyRoutes(nodes,bindings);
  for(const node of nodes){
    const outgoing=routes.filter(route=>route.binding.from.nodeId===node.id),incoming=routes.filter(route=>route.binding.to.nodeId===node.id);
    assert.ok(new Set(outgoing.map(route=>JSON.stringify(route.points[0]))).size<=1);
    assert.ok(new Set(incoming.map(route=>JSON.stringify(route.points.at(-1)))).size<=1);
  }
  const inputHeights=routes.filter(route=>route.binding.to.nodeId==='controller').map(route=>route.points.at(-1).y).sort((a,b)=>a-b);
  assert.equal(new Set(inputHeights).size,1);
});

test('saved and pending wires use fully smooth Beziers with horizontal port tangents',()=>{
  const output=diagramPortAnchor({x:20,y:40},'output',width,height),input=diagramPortAnchor({x:400,y:40},'input',width,height);
  assert.deepEqual(plain(output),{x:20+width,y:40+height-20});assert.deepEqual(plain(input),{x:400,y:40+height-20});
  const pending=diagramPendingPath(output,{x:700,y:400},'output');assert.ok(pending.startsWith(`M${output.x},${output.y} C`));assert.ok(!pending.includes(' L'));
  assert.match(diagramPendingPath(input,{x:10,y:0},'input'),/ C/);
  assert.equal(diagramSmoothPath([]),'');
  const curves=cubics(diagramSmoothPath([{x:0,y:0},{x:100,y:0},{x:100,y:100},{x:200,y:100}]));
  assert.equal(curves.length,2);assert.equal(curves[0].first.y,0);assert.equal(curves.at(-1).second.y,100);
  assert.ok(curves[0].first.x>curves[0].start.x);assert.ok(curves.at(-1).second.x<curves.at(-1).end.x);
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:600,y:320}}];
  const route=diagramRoutes(nodes,[edge('source','target')],width,height)[0],routedCurves=cubics(route.path);
  assert.equal(routedCurves.length,1,'An unobstructed connection uses one horizontal-tangent cubic');
  assert.equal(routedCurves[0].first.y,route.points[0].y);assert.equal(routedCurves.at(-1).second.y,route.points.at(-1).y);
});

test('segmented arrows round perpendicular lanes with the configured circular radius',()=>{
  const points=[{x:0,y:0},{x:100,y:0},{x:100,y:100},{x:200,y:100}];
  assert.equal(diagramOrthogonalPath(points),'M0,0 L76,0 A24,24 0 0 1 100,24 L100,76 A24,24 0 0 0 124,100 L200,100');
  assert.equal(diagramOrthogonalPath(points,8),'M0,0 L92,0 A8,8 0 0 1 100,8 L100,92 A8,8 0 0 0 108,100 L200,100');
  assert.equal(diagramOrthogonalPath(points,0),'M0,0 L100,0 L100,100 L200,100');
  assert.deepEqual(plain(diagramSegmentLine(points,1,24)),{start:{x:100,y:24},end:{x:100,y:76}});
  assert.deepEqual(plain(diagramSegmentLine(points,0,24)),{start:{x:0,y:0},end:{x:76,y:0}});
  assert.equal(diagramSegmentLine(points,1,1000),null,'Adjacent corner radii consume at most the available segment');
  assert.match(diagramOrthogonalPath(points,1000),/ A50,50 /);assert.doesNotMatch(diagramOrthogonalPath(points,-3),/ A/);
  assert.equal(diagramSegmentLine(points,3),null);assert.equal(diagramSegmentLine([{x:0,y:0},{x:0,y:0}],0),null);
  assert.match(diagramPendingPath(points[0],points.at(-1),'output','orthogonal',7),/ A7,7 /);
  assert.doesNotMatch(diagramPendingPath(points[0],points.at(-1),'output','orthogonal',0),/ A/);
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:600,y:320}}];
  const routes=diagramRoutes(nodes,[edge('source','target')],width,height,{style:'orthogonal',cornerRadius:9});
  assert.match(routes[0].path,/ A9,9 /);assert.deepEqual(plain(routes[0].points[0]),{x:width,y:height-20});
  assert.deepEqual(plain(routes[0].points.at(-1)),{x:600,y:320+height-20});
});

test('large circular corners clear blocks automatically and safely cap manual rounding without moving lanes',()=>{
  const nodes=[{id:'source',position:{x:0,y:100}},{id:'obstacle',position:{x:200,y:200}},{id:'target',position:{x:500,y:300}}],binding=edge('source','target');
  for(const cornerRadius of [0,24,64,100]){
    const route=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',cornerRadius})[0];
    assert.equal(route.cornerRadius,cornerRadius,'Automatic lanes leave enough space for the chosen radius');
    assertClearSamples(nodes,route.path);
  }
  const points=[{x:100,y:180},{x:320,y:180},{x:320,y:380},{x:500,y:380}],wireRoutes=[{bindingId:binding.id,points}],before=JSON.stringify(wireRoutes);
  const route=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',cornerRadius:100,wireRoutes})[0];
  assert.equal(route.manual,true);assert.equal(route.customBlocked,false);assert.deepEqual(plain(route.points),points);
  assert.ok(route.cornerRadius>68&&route.cornerRadius<69,'A tight manual corner uses the largest safe circular radius');
  assertClearSamples(nodes,route.path);assert.equal(JSON.stringify(wireRoutes),before);
  const span=diagramSegmentLine(route.points,1,route.cornerRadius);assert.ok(span);assert.equal(span.start.y,180+route.cornerRadius);assert.equal(span.end.y,380-route.cornerRadius);
  assertClearSamples(nodes.map(node=>({...node,position:{x:-node.position.x-100,y:node.position.y}})),diagramRoutes(nodes.map(node=>({...node,position:{x:-node.position.x-100,y:node.position.y}})),[binding],100,100,{style:'orthogonal',cornerRadius:100})[0].path);
});

test('named transform ports retain distinct row anchors while compact obstacles use their true bounds',()=>{
  const {canvasNodeSize,canvasNodeAnchor}=modules.get('./profile-canvas');
  const transform={id:'gate',kind:'processing',position:{x:440,y:80},processing:{inputs:[{id:'value'},{id:'enabled'}],outputs:[{id:'result'}]}};
  const gate={...transform,...canvasNodeSize(transform),inputAnchors:Object.fromEntries(transform.processing.inputs.map(port=>[port.id,canvasNodeAnchor(transform,'input',port.id)])),outputAnchors:{result:canvasNodeAnchor(transform,'output','result')}};
  const bindings=[{id:'value',from:{nodeId:'source',portId:'out'},to:{nodeId:'gate',portId:'value'}},{id:'enabled',from:{nodeId:'source',portId:'out'},to:{nodeId:'gate',portId:'enabled'}},{id:'result',from:{nodeId:'gate',portId:'result'},to:{nodeId:'sink',portId:'in'}}];
  const routes=verifyRoutes([{id:'source',position:{x:0,y:0}},gate,{id:'sink',position:{x:900,y:0}}],bindings);
  assert.notEqual(routes[0].points.at(-1).y,routes[1].points.at(-1).y);assert.deepEqual(plain(routes[2].points[0]),{x:664,y:172});
  const unobstructed=verifyRoutes([{id:'source',position:{x:0,y:100}},{id:'small-transform',position:{x:430,y:100},width:224,height:114},{id:'sink',position:{x:860,y:100}}],[edge('source','sink')]);
  assert.ok(unobstructed[0].points.every(point=>point.y===268),'A route may pass beneath the compact transform without an obsolete full-card obstacle');
});

test('Feature Lab geometry preserves every route and block clearance in both styles',async()=>{
  // Geometry only, captured from the actual DCS demo. No assignments, device
  // identities, contracts or runtime state are needed to reproduce its routing.
  const {nodes,bindings}=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/feature-lab-geometry.json',import.meta.url),'utf8'));
  assert.equal(nodes.length,42);assert.equal(bindings.length,73);
  const before=JSON.stringify({nodes,bindings});
  const started=performance.now();verifyRoutes(nodes,bindings);
  const routes=diagramRoutes(nodes,bindings,width,height,{style:'orthogonal',cornerRadius:64});
  assert.equal(routes.length,bindings.length);
  for(const route of routes)assertClearSamples(nodes,route.path);
  assert.ok(performance.now()-started<2000,'The complete dense graph must route without a multi-second search');
  assert.equal(JSON.stringify({nodes,bindings}),before);
});

test('a moved block burying ports does not search every lane in the dense graph',async()=>{
  const {nodes,bindings,wireRoutes}=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/feature-lab-moved-geometry.json',import.meta.url),'utf8'));
  const started=performance.now();
  for(const style of ['smooth','orthogonal']){
    const routes=diagramRoutes(nodes,bindings,width,height,{style,wireRoutes});
    assert.equal(routes.length,71);
    assert.deepEqual(bindings.filter(binding=>!routes.some(route=>route.binding.id===binding.id)).map(binding=>binding.id),['route-2','route-80'],'Only the two buried ports remain unrouted');
    if(style==='orthogonal')for(const route of routes)assertClearSamples(nodes,route.path);
  }
  assert.ok(performance.now()-started<1500,'An impossible endpoint must not trigger repeated whole-grid searches');
});

test('endpoint rejection preserves a valid lane exactly on an obstacle boundary',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:500,y:300}},{id:'obstacle',position:{x:400,y:340},width:60,height:100}],bindings=[edge('source','target')];
  for(const style of ['smooth','orthogonal']){
    const clear=diagramRoutes(nodes,bindings,100,100,{style});assert.equal(clear.length,1);
    const blocked=nodes.map(node=>node.id==='obstacle'?{...node,position:{x:401,y:340}}:node);
    assert.equal(diagramRoutes(blocked,bindings,100,100,{style}).length,0,'Moving past the boundary buries the arrival point');
  }
});

test('one hundred parallel inputs remain distinct and clear without an expanding grid search',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:320}}];
  const bindings=Array.from({length:100},(_,index)=>edge('source','target',`parallel-${String(index).padStart(3,'0')}`));
  const started=performance.now();
  const smooth=verifyRoutes(nodes,bindings),rounded=diagramRoutes(nodes,bindings,width,height,{style:'orthogonal',cornerRadius:40});
  for(const routes of [smooth,rounded]){assert.equal(routes.length,100);assert.equal(new Set(routes.map(route=>route.path)).size,100);}
  for(const route of rounded)assertClearSamples(nodes.map(node=>({...node,width,height})),route.path);
  assert.ok(performance.now()-started<2500,'Dense input fan-in must not trigger a full growing-grid search per wire');
});

test('a canvas router reuses geometry through unrelated document updates while returning current bindings',()=>{
  const router=createDiagramRouter(),nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:320}}],bindings=[edge('source','target')];
  const initial=router(nodes,bindings,width,height);
  assert.strictEqual(router(nodes,bindings,width,height),initial,'Selection and viewport updates reuse the route array');
  const nextBindings=bindings.map(binding=>({...binding,from:{...binding.from},to:{...binding.to},enabled:false}));
  const updated=router(nodes.map(node=>({...node,name:'Updated inventory name',position:{...node.position}})),nextBindings,width,height);
  assert.strictEqual(updated[0].binding,nextBindings[0],'Cached geometry never returns stale binding metadata');
  assert.strictEqual(updated[0].points,initial[0].points);
  assert.strictEqual(router(nodes,nextBindings,width,height),updated);
  const secondCanvas=createDiagramRouter();assert.notStrictEqual(secondCanvas(nodes,bindings,width,height)[0].points,initial[0].points,'Caches are canvas-local');
});

test('router caching observes dimensions, anchors, endpoints, styles, radii and authored bends',()=>{
  const router=createDiagramRouter(16),nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:320}}],bindings=[edge('source','target')];
  const first=router(nodes,bindings,width,height),assertChanged=(current)=>assert.notStrictEqual(current[0].points,first[0].points);
  assertChanged(router(nodes.map(node=>node.id==='source'?{...node,position:{x:0,y:20}}:node),bindings,width,height));
  assertChanged(router(nodes.map(node=>node.id==='source'?{...node,width:240}:node),bindings,width,height));
  const anchored=router(nodes.map(node=>node.id==='source'?{...node,outputAnchors:{out:{x:width,y:100}}}:node),bindings,width,height);assertChanged(anchored);assert.equal(anchored[0].points[0].y,100);
  assertChanged(router(nodes,[{...bindings[0],from:{nodeId:'source',portId:'other'}}],width,height));
  assertChanged(router(nodes,bindings,width+10,height));assertChanged(router(nodes,bindings,width,height+10));
  const segmented=router(nodes,bindings,width,height,{style:'orthogonal',cornerRadius:8});assertChanged(segmented);assert.match(segmented[0].path,/ A8,8 /);
  const radius=router(nodes,bindings,width,height,{style:'orthogonal',cornerRadius:40});assert.notEqual(radius[0].path,segmented[0].path);
  const points=[{x:width,y:height-20},{x:500,y:height-20},{x:500,y:320+height-20},{x:900,y:320+height-20}];
  const manual=router(nodes,bindings,width,height,{style:'orthogonal',wireRoutes:[{bindingId:bindings[0].id,points}]});assert.equal(manual[0].manual,true);assert.deepEqual(plain(manual[0].points),points);
  assert.strictEqual(router(nodes,bindings,width,height),first,'An undo can reuse the original geometry');
  const bounded=createDiagramRouter(1),old=bounded(nodes,bindings,width,height);bounded(nodes,bindings,width,height,{style:'orthogonal'});assert.notStrictEqual(bounded(nodes,bindings,width,height)[0].points,old[0].points,'Eviction bounds retained geometry');
});

test('ordered parallel fans use centered separate lanes without crossings',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}}],bindings=Array.from({length:8},(_,index)=>edge('source','target',`ordered-${index}`));
  for(const style of ['smooth','orthogonal']){
    const routes=diagramRoutes(nodes,bindings,width,height,{style}),quality=diagramRouteQuality(routes);
    assert.equal(quality.crossings,0,'Fan-out and fan-in keep the same lane order');
    const lanes=routes.map(route=>route.points.slice(1).map((point,index)=>({point,length:Math.abs(point.x-route.points[index].x)})).sort((a,b)=>b.length-a.length)[0].point.y);
    assert.equal(new Set(lanes).size,bindings.length);
    assert.ok(Math.min(...lanes)<height-20&&Math.max(...lanes)>height-20,'The fan is centered around its common physical port');
    assert.ok(quality.overlap<2000,'Only short leads share a port; the long signal corridors are separate');
  }
});

test('coordinated Feature Lab routing reduces crossings and shared tracks against the captured baseline',async()=>{
  const {nodes,bindings}=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/feature-lab-geometry.json',import.meta.url),'utf8'));
  // Same exact geometry and pairwise segment metric: before this routing pass,
  // the deterministic plan contained 342 crossings and 60516px of shared tracks.
  const routes=diagramRoutes(nodes,bindings,width,height),quality=diagramRouteQuality(routes);
  assert.equal(routes.length,73);
  assert.ok(quality.crossings<300,`Expected fewer crossings than342, received${quality.crossings}`);
  assert.ok(quality.overlap<45000,`Expected shorter shared tracks than60516px, received${quality.overlap}`);
});

test('incremental routing repairs moved obstacles and preserves unrelated route geometry',()=>{
  const nodes=[{id:'upper-source',position:{x:0,y:0}},{id:'upper-target',position:{x:1000,y:0}},{id:'lower-source',position:{x:0,y:900}},{id:'lower-target',position:{x:1000,y:900}},{id:'obstacle',position:{x:500,y:400},width:100,height:100}];
  const bindings=[edge('upper-source','upper-target'),edge('lower-source','lower-target')];
  for(const style of ['smooth','orthogonal']){
    const router=createDiagramRouter(),initial=router(nodes,bindings,width,height,{style});
    const moved=nodes.map(node=>node.id==='obstacle'?{...node,position:{x:500,y:120}}:node),updated=router(moved,bindings,width,height,{style});
    assert.equal(updated.length,2);assert.notEqual(updated[0].path,initial[0].path,'A wire intersected by the moved obstacle is repaired');
    assert.strictEqual(updated[1].points,initial[1].points,'An unrelated chain retains the exact same geometry');
    assert.equal(updated[1].path,initial[1].path);
    assert.strictEqual(router(nodes,bindings,width,height,{style}),initial,'Undo returns the original complete route snapshot');
    for(const route of updated)for(let index=1;index<route.points.length;index++){
      const a=route.points[index-1],b=route.points[index];
      assert.ok(!(a.x===b.x?a.x>500&&a.x<600&&Math.max(a.y,b.y)>120&&Math.min(a.y,b.y)<220:a.y>120&&a.y<220&&Math.max(a.x,b.x)>500&&Math.min(a.x,b.x)<600));
    }
  }
});

test('smooth manual routes interpolate explicit reroute handles after endpoint moves',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:320}}],binding=edge('source','target');
  const points=[{x:width,y:height-20},{x:500,y:height-20},{x:500,y:320+height-20},{x:900,y:320+height-20}],handle={x:500,y:300},wireRoutes=[{bindingId:binding.id,points,handles:[handle]}];
  for(const placed of [nodes,nodes.map(node=>node.id==='source'?{...node,position:{x:0,y:40}}:node)]){
    const route=diagramRoutes(placed,[binding],width,height,{style:'smooth',wireRoutes})[0];
    assert.equal(route.manual,true);assert.ok(cubics(route.path).some(curve=>curve.end.x===handle.x&&curve.end.y===handle.y),'The explicit handle remains an interpolated cubic knot');
  }
});

test('incremental obstacle repair restores an authored lane once it becomes clear',()=>{
  const binding=edge('source','target'),nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}},{id:'obstacle',position:{x:500,y:120},width:100,height:100}];
  const points=[{x:width,y:height-20},{x:900,y:height-20}],wireRoutes=[{bindingId:binding.id,points}];
  const router=createDiagramRouter(),blocked=router(nodes,[binding],width,height,{wireRoutes});
  assert.equal(blocked[0].customBlocked,true);assert.equal(blocked[0].manual,false);
  const clear=router(nodes.map(node=>node.id==='obstacle'?{...node,position:{x:500,y:400}}:node),[binding],width,height,{wireRoutes});
  assert.equal(clear[0].customBlocked,false);assert.equal(clear[0].manual,true);
  assert.deepEqual(plain(clear[0].points),points,'Temporary obstacle avoidance never replaces the authored route');
});

test('incremental smooth repair checks the rendered curve beyond its orthogonal guides',()=>{
  const binding=edge('source','target'),nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:1000,y:600}},{id:'obstacle',position:{x:2000,y:2000},width:20,height:20}],router=createDiagramRouter();
  const initial=router(nodes,[binding],width,height),route=initial[0];
  const distance=point=>Math.min(...route.points.slice(1).map((end,index)=>{
    const start=route.points[index],x=Math.max(Math.min(start.x,end.x),Math.min(Math.max(start.x,end.x),point.x)),y=Math.max(Math.min(start.y,end.y),Math.min(Math.max(start.y,end.y),point.y));
    return Math.hypot(point.x-x,point.y-y);
  }));
  const point=cubics(route.path).flatMap(curve=>[.25,.5,.75].map(t=>sample(curve,t))).sort((a,b)=>distance(b)-distance(a))[0];
  assert.ok(distance(point)>60,'The broad spline has room to curve away from its channel guides');
  const position={x:point.x-10,y:point.y-10},moved=nodes.map(node=>node.id==='obstacle'?{...node,position}:node);
  const updated=router(moved,[binding],width,height);
  assert.equal(updated.length,1);assert.notEqual(updated[0].path,initial[0].path,'An obstacle crossing the rendered spline triggers repair');
  for(const curve of cubics(updated[0].path))for(let step=0;step<=100;step++){
    const at=sample(curve,step/100);
    assert.ok(!(at.x>position.x&&at.x<position.x+20&&at.y>position.y&&at.y<position.y+20));
  }
});

test('adding parallel signals replans their physical fan and leaves every wire selectable',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:432,y:0}},{id:'contributor',position:{x:0,y:316}},{id:'unrelated-source',position:{x:0,y:800}},{id:'unrelated-target',position:{x:900,y:800}}];
  const first=edge('source','target','first'),second={...edge('source','target','second'),from:{nodeId:'source',portId:'another-output'},to:{nodeId:'target',portId:'another-input'}},contributor=edge('contributor','target','contributor'),unrelated=edge('unrelated-source','unrelated-target');
  const distance=(point,a,b)=>{const dx=b.x-a.x,dy=b.y-a.y,scale=Math.max(0,Math.min(1,((point.x-a.x)*dx+(point.y-a.y)*dy)/(dx*dx+dy*dy||1)));return Math.hypot(point.x-a.x-dx*scale,point.y-a.y-dy*scale);};
  for(const style of ['smooth','orthogonal']){
    const router=createDiagramRouter(),initial=router(nodes,[first,unrelated],width,height,{style});
    router(nodes,[first,contributor,unrelated],width,height,{style});
    const updated=router(nodes,[first,contributor,second,unrelated],width,height,{style});
    assert.notStrictEqual(updated[0].points,initial[0].points,'Adding a signal rebalances every peer at the shared physical resource port');
    assert.strictEqual(updated[3].points,initial[1].points,'An unrelated chain keeps its exact geometry');
    const samples=updated.slice(0,3).map(route=>style==='smooth'?cubics(route.path).flatMap(curve=>Array.from({length:81},(_,index)=>sample(curve,index/80))):roundedSamples(route.path));
    for(let index=0;index<3;index++){
      const otherSegments=samples.flatMap((other,peer)=>peer===index?[]:other.slice(1).map((end,i)=>[other[i],end]));
      assert.ok(samples[index].some(point=>Math.min(...otherSegments.map(([start,end])=>distance(point,start,end)))>9.25),'Each arrow has a centerline outside every neighboring 18px pointer hit area');
    }
  }
});

test('reconnecting a signal invalidates fans at both previous and new physical attachments',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'first-target',position:{x:900,y:0}},{id:'second-target',position:{x:900,y:350}},{id:'other-source',position:{x:0,y:800}},{id:'other-target',position:{x:900,y:800}}];
  const bindings=[edge('source','first-target','first'),edge('source','second-target','second'),edge('other-source','other-target','other')],router=createDiagramRouter();
  const initial=router(nodes,bindings,width,height),next=bindings.map(binding=>binding.id==='second'?{...binding,to:{nodeId:'first-target',portId:'another-resource-input'}}:binding),updated=router(nodes,next,width,height);
  assert.notStrictEqual(updated[0].points,initial[0].points,'An existing peer participates in the changed source and target fan');
  assert.strictEqual(updated[2].points,initial[2].points);
  assert.deepEqual(plain(updated[0].points[0]),plain(updated[1].points[0]));
  assert.deepEqual(plain(updated[0].points.at(-1)),plain(updated[1].points.at(-1)));
  assert.notEqual(updated[0].path,updated[1].path);
});

test('automatic cleanup erases retracing and self-crossing loops without changing authored geometry',()=>{
  const retrace=[{x:0,y:0},{x:20,y:0},{x:20,y:60},{x:20,y:20},{x:80,y:20}],before=JSON.stringify(retrace);
  assert.deepEqual(plain(diagramAutomaticCorners(retrace)),[{x:0,y:0},{x:20,y:0},{x:20,y:20},{x:80,y:20}]);
  const loop=[{x:0,y:0},{x:80,y:0},{x:80,y:80},{x:20,y:80},{x:20,y:-20},{x:120,y:-20}];
  assert.deepEqual(plain(diagramAutomaticCorners(loop)),[{x:0,y:0},{x:20,y:0},{x:20,y:-20},{x:120,y:-20}]);
  assert.equal(diagramSimpleRoute(retrace),false);assert.equal(diagramSimpleRoute(loop),false);
  assert.equal(diagramSimpleRoute(diagramAutomaticCorners(loop)),true);assert.equal(JSON.stringify(retrace),before);
  const manual=cubics(diagramSmoothPath(retrace,retrace)),turn=manual.findIndex(curve=>curve.end.x===20&&curve.end.y===60);
  assert.ok(turn>=0);assert.deepEqual(plain(manual[turn].second),{x:20,y:60});assert.deepEqual(plain(manual[turn+1].first),{x:20,y:60},'An authored reversal has a stationary cusp instead of an arbitrary rightward loop');
});

test('Feature Lab fan approaches never retrace themselves or double back near the auxiliary input',async()=>{
  const {nodes,bindings}=JSON.parse(await readFile(new URL('../../tests/fixtures/profiles/feature-lab-geometry.json',import.meta.url),'utf8'));
  for(const style of ['smooth','orthogonal']){
    const routes=diagramRoutes(nodes,bindings,width,height,{style});assert.equal(routes.length,73);
    for(const route of routes){
      assert.ok(diagramSimpleRoute(route.points),`${route.binding.id} contains an automatic retrace or self-crossing`);
      const points=style==='smooth'?cubics(route.path).flatMap(curve=>Array.from({length:33},(_,index)=>sample(curve,index/32))):roundedSamples(route.path);
      assertNoSelfCrossings(points,`${style} ${route.binding.id}`);
      if(route.binding.to.nodeId!=='virtual-aux')continue;
      const end=route.points.at(-1),lastOutside=points.findLastIndex(point=>point.x<end.x-300),terminal=points.slice(lastOutside+1);
      for(let index=1;index<terminal.length;index++){
        assert.ok(terminal[index].x>=terminal[index-1].x-1e-6,`${style} ${route.binding.id} reverses horizontally in its merge`);
        assert.ok(Math.abs(terminal[index].y-end.y)<=Math.abs(terminal[index-1].y-end.y)+1e-6,`${style} ${route.binding.id} moves away from the input height before merging`);
      }
    }
  }
});

test('nearby sources fan into a common input without forced overshoot in either direction',()=>{
  const nodes=[{id:'upper',position:{x:0,y:0}},{id:'lower',position:{x:0,y:350}},{id:'target',position:{x:500,y:180}}],bindings=[edge('upper','target','upper-a'),edge('upper','target','upper-b'),edge('lower','target','lower-a'),edge('lower','target','lower-b')];
  for(const style of ['smooth','orthogonal'])for(const placed of [nodes,nodes.map(node=>({...node,position:{x:-node.position.x-(node.width??width),y:node.position.y}}))]){
    const routes=diagramRoutes(placed,bindings,width,height,{style});assert.equal(routes.length,4);
    for(const route of routes){
      assert.ok(diagramSimpleRoute(route.points));
      const points=style==='smooth'?cubics(route.path).flatMap(curve=>Array.from({length:33},(_,index)=>sample(curve,index/32))):roundedSamples(route.path);
      assertNoSelfCrossings(points,`${style} ${route.binding.id}`);
    }
  }
});

test('unobstructed smooth routes use one cubic and cornered routes use the minimum links',()=>{
  const binding=edge('source','target');
  for(const targetY of [0,300]){
    const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:600,y:targetY}}];
    const smooth=diagramRoutes(nodes,[binding],100,100,{style:'smooth'})[0];
    assert.equal(cubics(smooth.path).length,1,'A clear connection needs one cubic, including a direct horizontal link');
    for(const cornerRadius of [0,24,64]){
      const orthogonal=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',cornerRadius})[0];
      assert.equal(orthogonal.points.length-1,targetY?3:1,'Horizontal port tangents require only H-V-H when endpoint heights differ');
      assertClearSamples(nodes,orthogonal.path);
    }
  }
});

test('one centered obstacle adds only the necessary smooth station or orthogonal detour',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}},{id:'obstacle',position:{x:430,y:0}}],binding=edge('source','target');
  const smooth=diagramRoutes(nodes,[binding],100,100,{style:'smooth'})[0],curves=cubics(smooth.path);
  assert.equal(curves.length,2,'One intermediate station bypasses the obstacle; fitting every guide corner would create four cubics');
  for(const curve of curves)for(let step=0;step<=100;step++){
    const point=sample(curve,step/100);assert.ok(!(point.x>430&&point.x<530&&point.y>0&&point.y<100));
  }
  assertNoSelfCrossings(curves.flatMap(curve=>Array.from({length:33},(_,index)=>sample(curve,index/32))),'Two-piece obstacle detour');
  for(const cornerRadius of [0,24,64]){
    const orthogonal=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',cornerRadius})[0];
    assert.equal(orthogonal.points.length-1,5,'Equal-height ports need exactly H-V-H-V-H around the centered obstacle');assertClearSamples(nodes,orthogonal.path);
  }
});

test('parallel equal-height ports retain exactly the extra cubic needed for separation',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:900,y:0}}],bindings=[edge('source','target','first'),edge('source','target','second')];
  const routes=diagramRoutes(nodes,bindings,100,100,{style:'smooth'});
  assert.equal(new Set(routes.map(route=>route.path)).size,2);
  for(const route of routes){assert.equal(cubics(route.path).length,2,'A second cubic retains a distinct intermediate lane with common horizontal port tangents');assertNoSelfCrossings(cubics(route.path).flatMap(curve=>Array.from({length:33},(_,index)=>sample(curve,index/32))),route.binding.id);}
});
