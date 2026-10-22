import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';

const {refineDiagramLanes,separateDiagramParallels,spaceDiagramTracks,diagramRouteQuality,diagramSimpleRoute}=(await loadModules(['./profile-diagram-lanes'])).get('./profile-diagram-lanes');
const plain=value=>JSON.parse(JSON.stringify(value));
const parallel=(id,sourceColumn,targetColumn)=>({binding:{id},manual:false,points:[
  {x:1200,y:1015},{x:sourceColumn,y:1015},{x:sourceColumn,y:1057},
  {x:targetColumn,y:1057},{x:targetColumn,y:1015},{x:2768,y:1015},
]});

test('parallel physical ports separate long shared trunks into the available side of a narrow corridor',()=>{
  const originals=[parallel('route-21',1280,2700),parallel('route-47',1292,2688),parallel('route-48',1304,2676),
    {binding:{id:'crossing'},manual:true,points:[{x:2000,y:1060},{x:2000,y:1110}]}];
  const before=diagramRouteQuality(originals),fixed=plain(originals.at(-1));
  let checks=0;
  const result=refineDiagramLanes(originals.map(route=>({...route})),(route,points)=>{
    checks++;
    // The expanded blocks above the trunk occupy the full strip through
    // y=1057. There is room below; endpoint lead columns must stay ordered.
    return diagramSimpleRoute(points)&&points[2].y>=1057&&points[2].y<=1100&&points[2].y===points[3].y
      &&points[1].x<=points[3].x&&points[1].x>1200&&points[3].x<2768;
  },12,()=>true);
  assert.equal(new Set(result.slice(0,3).map(route=>route.points[2].y)).size,3);
  assert.ok(diagramRouteQuality(result).overlap<before.overlap/4);
  assert.deepEqual(plain(result.at(-1)),fixed,'authored crossing wire remains unchanged');
  for(let index=0;index<3;index++){
    assert.deepEqual(plain(result[index].points[0]),{x:1200,y:1015});
    assert.deepEqual(plain(result[index].points.at(-1)),{x:2768,y:1015});
    assert.ok(diagramSimpleRoute(result[index].points));
  }
  assert.ok(checks<=48*3+16*4,'refinement remains bounded');
});

test('lane refinement is deterministic and leaves authored parallel paths untouched',()=>{
  const make=()=>[parallel('route-21',1280,2700),{...parallel('authored',1292,2688),manual:true}];
  const clear=(_route,points)=>diagramSimpleRoute(points)&&points[2].y>=1057;
  const original=make(),first=refineDiagramLanes(make(),clear),second=refineDiagramLanes(make(),clear);
  assert.deepEqual(plain(first),plain(second));
  assert.deepEqual(plain(first[1]),original[1]);
});

test('final separation keeps parallel trunks at least one lane apart, including tracks across port height',()=>{
  const route=(id,column,row)=>({binding:{id},manual:false,points:[{x:1200,y:2393},{x:column,y:2393},{x:column,y:row},{x:2700,y:row},{x:2700,y:2393},{x:2768,y:2393}]});
  const original=[route('route-21',1280,2389),route('route-47',1292,2390),route('route-48',1304,2391.7)];
  let checks=0;
  const result=separateDiagramParallels(original.map(value=>({...value})),(_route,points)=>{
    checks++;
    return diagramSimpleRoute(points)&&points[2].y>=2389;
  });
  assert.deepEqual(plain(result[0]),original[0],'one automatic trunk supplies a stable base');
  const rows=result.map(value=>value.points[2].y);
  for(let i=0;i<rows.length;i++)for(let j=i+1;j<rows.length;j++)assert.ok(Math.abs(rows[i]-rows[j])>=12-1e-6);
  assert.ok(rows.some(row=>row>2393),'a simple approach can cross port height to use the free corridor');
  for(const value of result){
    assert.ok(diagramSimpleRoute(value.points));
    assert.deepEqual(plain(value.points[0]),{x:1200,y:2393});
    assert.deepEqual(plain(value.points.at(-1)),{x:2768,y:2393});
  }
  assert.ok(checks<=256);
});

test('final separation respects fixed paths, unrelated endpoints and full-candidate clearance',()=>{
  const fixed={...parallel('manual',1280,2700),manual:true},moving=parallel('moving',1292,2688),unrelated=parallel('unrelated',1304,2676);
  unrelated.points=unrelated.points.map((point,index)=>index===0?{...point,y:1000}:point);
  const blocked=separateDiagramParallels([fixed,moving,unrelated],()=>false);
  assert.equal(blocked[0],fixed);assert.equal(blocked[1],moving);assert.equal(blocked[2],unrelated);
  const result=separateDiagramParallels([fixed,moving,unrelated],(_route,points)=>diagramSimpleRoute(points)&&points[2].y>=1057);
  assert.equal(result[0],fixed);assert.equal(result[2],unrelated);
  assert.ok(result[1].points[2].y>=1069);
});

test('optional grouping separates shared node-pair trunks with different named input anchors',()=>{
  const route=(id,column,targetY)=>({binding:{id},manual:false,points:[{x:336,y:884},{x:column,y:884},{x:column,y:100},{x:28,y:100},{x:28,y:targetY},{x:48,y:targetY}]});
  const original=[route('route-6',358,168),route('route-7',356,140)];
  const result=separateDiagramParallels(original.map(value=>({...value})),(_route,points)=>diagramSimpleRoute(points)&&points[1].x>336&&points.at(-2).x<48,12,()=> 'source\0gate');
  assert.ok(Math.abs(result[0].points[1].x-result[1].points[1].x)>=12);
  for(let index=0;index<original.length;index++){
    assert.deepEqual(plain(result[index].points[0]),original[index].points[0]);
    assert.deepEqual(plain(result[index].points.at(-1)),original[index].points.at(-1));
    assert.ok(diagramSimpleRoute(result[index].points));
  }
});

test('congestion detection compares all peer legs when their longest legs lie at opposite ends',()=>{
  const original=[
    {binding:{id:'route-6'},manual:false,points:[{x:336,y:884},{x:358,y:884},{x:358,y:517},{x:28,y:517},{x:28,y:168},{x:48,y:168}]},
    {binding:{id:'route-7'},manual:false,points:[{x:336,y:884},{x:356,y:884},{x:356,y:520},{x:22,y:520},{x:22,y:140},{x:48,y:140}]},
  ];
  const result=separateDiagramParallels(original.map(route=>({...route})),(_route,points)=>diagramSimpleRoute(points)&&points[1].x>336&&points.at(-2).x<48,12,()=> 'resource-1\0transform-3');
  assert.deepEqual(plain(result[0]),original[0]);
  assert.ok(Math.abs(result[0].points[3].x-result[1].points[3].x)>=12,'the longest congested target-side leg separates from the peer target leg');
  for(let index=0;index<result.length;index++){
    assert.ok(diagramSimpleRoute(result[index].points));
    assert.deepEqual(plain(result[index].points[0]),original[index].points[0]);
    assert.deepEqual(plain(result[index].points.at(-1)),original[index].points.at(-1));
  }
});

test('a separation candidate cannot duplicate a peer path after collinear cleanup',()=>{
  const fixed=Array.from({length:3},(_,index)=>({...parallel(`fixed-${index}`,1280,2700),manual:true}));
  const destination={...parallel('destination',1280,2700),manual:true};
  destination.points=destination.points.map((point,index)=>index===2||index===3?{...point,y:1069}:point);
  destination.points.splice(3,0,{x:2000,y:1069});
  const moving=parallel('moving',1280,2700);
  const result=separateDiagramParallels([...fixed,destination,moving],(_route,points)=>diagramSimpleRoute(points)&&points[2].y===1069);
  assert.equal(result.at(-1),moving,'if every clear candidate duplicates a peer, retain the previous geometry');
  assert.equal(result[3],destination);
});

const squared=(id,x,y=0,endY=200)=>({binding:{id},manual:false,points:[{x:0,y},{x,y},{x,y:endY},{x:400,y:endY}]});
test('squared tracks separate nearly touching routes without moving anchors or adding crossings',()=>{
  const routes=[squared('one',200),squared('two',206,20,220)];
  const original=plain(routes),result=spaceDiagramTracks(routes.map(route=>({...route})),()=>true);
  assert.ok(Math.abs(result[0].points[1].x-result[1].points[1].x)>=24);
  assert.ok(diagramRouteQuality(result).crossings<=diagramRouteQuality(original).crossings);
  for(let i=0;i<routes.length;i++){assert.deepEqual(plain(result[i].points[0]),original[i].points[0]);assert.deepEqual(plain(result[i].points.at(-1)),original[i].points.at(-1));assert.ok(diagramSimpleRoute(result[i].points));}
});
test('disjoint vertical segments align on a common column and spacing stays deterministic',()=>{
  const make=()=>[squared('one',201,0,80),squared('two',209,150,230)];
  const result=spaceDiagramTracks(make(),()=>true);
  assert.equal(result[0].points[1].x,result[1].points[1].x);
  assert.equal(result[0].points[1].x%24,0);
  assert.deepEqual(plain(result),plain(spaceDiagramTracks(make(),()=>true)));
});
test('track spacing respects obstacles and authored paths and has bounded validation work',()=>{
  const manual={...squared('manual',206,20,220),manual:true},moving=squared('moving',200);
  let checks=0;
  const blocked=spaceDiagramTracks([moving,manual],()=>{checks++;return false;});
  assert.deepEqual(plain(blocked),plain([moving,manual]));
  const result=spaceDiagramTracks([moving,manual],(_route,points)=>{checks++;return points[1].x>=200;});
  assert.deepEqual(plain(result[1]),plain(manual));assert.ok(result[0].points[1].x>=200);
  assert.ok(Math.abs(result[0].points[1].x-manual.points[1].x)>=24);assert.ok(checks<=64*7*2);
  assert.deepEqual(plain(spaceDiagramTracks([],()=>true)),[]);
});

test('crowded converging squared routes favour clear intersections over long merged trunks',()=>{
  const routes=[squared('upper',200,0,300),squared('lower',200,100,300),squared('return',200,200,100)];
  const before=diagramRouteQuality(routes),original=plain(routes);
  const after=spaceDiagramTracks(routes.map(route=>({...route})),()=>true);
  assert.ok(diagramRouteQuality(after).overlap<before.overlap/2);
  for(let i=0;i<after.length;i++){assert.ok(diagramSimpleRoute(after[i].points));assert.deepEqual(plain(after[i].points[0]),original[i].points[0]);assert.deepEqual(plain(after[i].points.at(-1)),original[i].points.at(-1));}
});
