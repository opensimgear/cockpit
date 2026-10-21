import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';

const modules=await loadModules(['./profile-editor','./profile-diagram','./profile-canvas']);
const {profileAutoLayout,PROFILE_BLOCK_WIDTH:width,PROFILE_BLOCK_HEIGHT:height}=modules.get('./profile-editor');
const {diagramRoutes,diagramPortAnchor,diagramPendingPath,diagramSmoothPath,diagramOrthogonalPath,diagramSegmentLine}=modules.get('./profile-diagram');
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
  assert.equal(curves.length,3);assert.equal(curves[0].first.y,0);assert.equal(curves.at(-1).second.y,100);
  assert.ok(curves[0].first.x>curves[0].start.x);assert.ok(curves.at(-1).second.x<curves.at(-1).end.x);
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'target',position:{x:600,y:320}}];
  const route=diagramRoutes(nodes,[edge('source','target')],width,height)[0];assert.equal(cubics(route.path).length,1,'A clear connection uses a single flowing curve');
  const curve=cubics(route.path)[0];assert.notEqual(sample(curve,.25).y,curve.start.y);assert.notEqual(sample(curve,.75).y,curve.end.y);
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
