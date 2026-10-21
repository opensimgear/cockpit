import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const modules=await loadModules(['./profile-wire-layout','./profile-diagram','./profile-vehicle-tabs']);
const {profileMoveWireSegment:move,profileAnchorWireRoute:anchor,profileSetWireStyle:setStyle,profileSetWireRoute:setRoute}=modules.get('./profile-wire-layout');
const {profileInsertWireHandle:insertHandle,profileMoveWireHandle:moveHandle,profileRemoveWireHandle:removeHandle,profileWireHandleOnRoute:handleOnRoute,profileReshapeWireSegment:reshapeSegment}=modules.get('./profile-wire-layout');
const {diagramRoutes,diagramPendingPath}=modules.get('./profile-diagram');
const {profileDocumentError,parseProfileDocument,exportProfileDocument}=modules.get('./profiles');
const graph=modules.get('./profile-graph');
const plain=value=>JSON.parse(JSON.stringify(value));
const points=[{x:100,y:80},{x:250,y:80},{x:250,y:380},{x:500,y:380}];
const binding={id:'route-1',from:{nodeId:'source',portId:'out'},to:{nodeId:'sink',portId:'in'}};
function draft(){
  const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
  return {document:{schemaVersion:2,id:'11111111-1111-4111-8111-111111111111',name:'Wire layout',revision:1,eligibility:{gameIds:[]},resources:[{id:'source',name:'Source',kind:'device',required:false,ports:[{id:'out',name:'Value',direction:'output',contract}]},{id:'sink',name:'Sink',kind:'device',required:false,ports:[{id:'in',name:'Value',direction:'input',contract},{id:'other',name:'Other',direction:'input',contract}]}],bindings:[plain(binding)],processingNodes:[],virtualOutputs:[],visibility:[],overrides:[],companionAssignments:[],editor:{positions:[],viewport:{x:0,y:0,zoom:1}}},assignments:[]};
}
function orthogonal(path){for(let i=1;i<path.length;i++)assert.ok((path[i].x===path[i-1].x)!==(path[i].y===path[i-1].y));}
test('segment drags constrain the moved axis and preserve both physical endpoints',()=>{
  const before=plain(points),vertical=move(points,1,{x:320,y:900});
  assert.deepEqual(plain(vertical),[{x:100,y:80},{x:320,y:80},{x:320,y:380},{x:500,y:380}]);
  for(const [index,y] of [[0,30],[2,430]]){
    const result=move(points,index,{x:-900,y});orthogonal(result);
    assert.deepEqual(plain(result[0]),points[0]);assert.deepEqual(plain(result.at(-1)),points.at(-1));
    assert.ok(result.some((point,i)=>i&&point.y===y&&result[i-1].y===y));
    assert.equal(result[0].y,result[1].y);assert.equal(result.at(-2).y,result.at(-1).y);
  }
  const straight=[{x:0,y:0},{x:200,y:0}],dogleg=move(straight,0,{x:99,y:100});
  orthogonal(dogleg);assert.equal(dogleg.length,6);assert.deepEqual(plain(dogleg[0]),straight[0]);assert.deepEqual(plain(dogleg.at(-1)),straight.at(-1));
  assert.deepEqual(points,before);assert.throws(()=>move(points,3,{x:1,y:1}),/existing/);assert.throws(()=>move(points,1,{x:Infinity,y:0}),/bounds/);
});
test('manual lanes re-anchor to moved blocks and styles use the same safe geometry',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'sink',position:{x:500,y:300}}],wireRoutes=[{bindingId:binding.id,points}];
  const smooth=diagramRoutes(nodes,[binding],100,100,{wireRoutes})[0];
  assert.equal(smooth.manual,true);assert.equal(smooth.customBlocked,false);assert.match(smooth.path,/ C/);assert.doesNotMatch(smooth.path,/[LQA]/);
  const angular=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',wireRoutes})[0];
  assert.deepEqual(plain(angular.points),points);assert.doesNotMatch(angular.path,/[QC]/);assert.match(angular.path,/ L/);assert.match(angular.path,/ A24,24 /);
  const sharp=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',cornerRadius:0,wireRoutes})[0];assert.deepEqual(plain(sharp.points),points);assert.doesNotMatch(sharp.path,/[AQC]/);
  const moved=diagramRoutes([{...nodes[0],position:{x:20,y:40}},{...nodes[1],position:{x:550,y:340}}],[binding],100,100,{style:'orthogonal',wireRoutes})[0];
  assert.equal(moved.manual,true);assert.deepEqual(plain(moved.points),[{x:120,y:120},{x:250,y:120},{x:250,y:420},{x:550,y:420}]);
  assert.match(diagramPendingPath(points[0],points.at(-1),'output','orthogonal'),/ A24,24 /);
  assert.deepEqual(plain(anchor(points,{x:280,y:80},{x:500,y:380})[0]),{x:280,y:80});
});
test('moving an obstacle into a manually chosen lane falls back visibly without losing the authored path',()=>{
  const nodes=[{id:'source',position:{x:0,y:0}},{id:'sink',position:{x:500,y:300}},{id:'obstacle',position:{x:200,y:150}}];
  const wireRoutes=[{bindingId:binding.id,points}],before=plain(wireRoutes),route=diagramRoutes(nodes,[binding],100,100,{style:'orthogonal',wireRoutes})[0];
  assert.equal(route.manual,false);assert.equal(route.customBlocked,true);assert.notDeepEqual(plain(route.points),points);
  for(let i=1;i<route.points.length;i++){const a=route.points[i-1],b=route.points[i];assert.ok(!(a.x===b.x&&a.x>200&&a.x<300&&Math.max(a.y,b.y)>150&&Math.min(a.y,b.y)<250));}
  assert.deepEqual(wireRoutes,before);
});
test('wire layout edits are immutable, validated and cleaned up with routes or endpoints',()=>{
  const original=draft(),changed=setRoute(setStyle(original,'orthogonal'),binding.id,points);
  assert.equal(changed.document.editor.arrowStyle,'orthogonal');assert.equal(profileDocumentError(changed.document),'');assert.equal(original.document.editor.arrowStyle,undefined);
  assert.equal(setStyle(changed,'smooth').document.editor.wireRoutes.length,1);
  assert.equal(setRoute(changed,binding.id).document.editor.wireRoutes,undefined);
  assert.equal(graph.graphDisconnect(changed,binding.id).document.editor.wireRoutes,undefined);
  assert.equal(graph.graphRemoveNode(changed,'source').document.editor.wireRoutes,undefined);
  assert.equal(graph.graphRemovePort(changed,'sink','in').document.editor.wireRoutes,undefined);
  assert.equal(graph.graphConnect(changed,binding.from,{nodeId:'sink',portId:'other'},binding.id).document.editor.wireRoutes,undefined);
  assert.equal(graph.graphConnect(changed,binding.from,binding.to,binding.id).document.editor.wireRoutes.length,1);
  assert.throws(()=>setRoute(original,'missing',points),/existing/);assert.throws(()=>setStyle(original,'curvy'),/smooth or orthogonal/);
});
test('automatic corridors roundtrip as metadata and become authored when reshaped',()=>{
  const original=draft();original.document.editor.wireRoutes=[{bindingId:binding.id,points:plain(points),automatic:true}];
  assert.equal(profileDocumentError(original.document),'');
  assert.deepEqual(plain(parseProfileDocument(exportProfileDocument(original.document))),original.document);
  assert.equal(profileDocumentError({...original.document,editor:{...original.document.editor,wireRoutes:[{...original.document.editor.wireRoutes[0],handles:[]}]}}),'');
  assert.equal(profileDocumentError({...original.document,editor:{...original.document.editor,wireRoutes:[{...original.document.editor.wireRoutes[0],automatic:false,handles:[{x:250,y:200}]}]}}),'');
  for(const edited of [setRoute(original,binding.id,points),reshapeSegment(original,binding.id,points,1,{x:300,y:0}),insertHandle(original,binding.id,points,{x:255,y:200})]){
    assert.equal(edited.document.editor.wireRoutes[0].automatic,undefined);
    assert.equal(profileDocumentError(edited.document),'');
  }
  assert.equal(original.document.editor.wireRoutes[0].automatic,true);
});
test('portable wire metadata rejects unknown routes, duplicates, diagonal, empty and unbounded points',()=>{
  const valid=setRoute(setStyle(draft(),'orthogonal'),binding.id,points).document;
  for(const editor of [
    {arrowStyle:null},{arrowStyle:'right-angle'},{wireRoutes:null},{wireRoutes:[{bindingId:'missing',points}]},
    {wireRoutes:[valid.editor.wireRoutes[0],valid.editor.wireRoutes[0]]},{wireRoutes:[{bindingId:binding.id,points:[]}]},
    {wireRoutes:[{bindingId:binding.id,points:[{x:0,y:0},{x:1,y:1}]}]},
    {wireRoutes:[{bindingId:binding.id,points:[{x:0,y:0},{x:0,y:0}]}]},
    {wireRoutes:[{bindingId:binding.id,points:[{x:0,y:0},{x:1_000_001,y:0}]}]},
    {wireRoutes:[{bindingId:binding.id,points:Array.from({length:129},(_,x)=>({x,y:0}))}]},
    {wireRoutes:[{bindingId:binding.id,points,extra:true}]},
    ...[null,'true',1,{},[]].map(automatic=>({wireRoutes:[{bindingId:binding.id,points,automatic}]})),
    {wireRoutes:[{bindingId:binding.id,points,automatic:true,handles:[{x:250,y:200}]}]},
    {wireRoutes:[{bindingId:binding.id,points,handles:null}]},
    {wireRoutes:[{bindingId:binding.id,points,handles:[{x:200,y:200}]}]},
    {wireRoutes:[{bindingId:binding.id,points,handles:[{x:250,y:200},{x:250,y:200}]}]},
    {wireRoutes:[{bindingId:binding.id,points,handles:Array.from({length:33},(_,index)=>({x:250,y:100+index}))}]},
    {wireRoutes:[{bindingId:binding.id,points,handles:[{x:250,y:200,z:0}]}]},
  ])assert.ok(profileDocumentError({...valid,editor:{...valid.editor,...editor}}),JSON.stringify(editor));
});
test('reroute points survive straight span insertion, immutable dragging, anchoring and reset',()=>{
  const original=draft(),inserted=insertHandle(original,binding.id,points,{x:255,y:200}),route=inserted.document.editor.wireRoutes[0];
  assert.deepEqual(plain(route.handles),[{x:250,y:200}]);assert.ok(route.points.some(point=>point.x===250&&point.y===200));
  assert.equal(profileDocumentError(inserted.document),'');assert.equal(original.document.editor.wireRoutes,undefined);
  const moved=moveHandle(inserted,binding.id,0,{x:330,y:210}),after=moved.document.editor.wireRoutes[0];orthogonal(after.points);
  assert.deepEqual(plain(after.points[0]),points[0]);assert.deepEqual(plain(after.points.at(-1)),points.at(-1));assert.deepEqual(plain(after.handles),[{x:330,y:210}]);assert.ok(handleOnRoute(after.handles[0],after.points));
  assert.equal(after.points[0].y,after.points[1].y);assert.equal(after.points.at(-2).y,after.points.at(-1).y);
  const anchored=anchor(route.points,{x:120,y:120},{x:550,y:420},route.handles);orthogonal(anchored);assert.ok(handleOnRoute(route.handles[0],anchored));
  const added=insertHandle(moved,binding.id,after.points,{x:400,y:380}),twice=moveHandle(added,binding.id,0,{x:340,y:220});
  assert.equal(twice.document.editor.wireRoutes[0].handles.length,2);assert.ok(twice.document.editor.wireRoutes[0].handles.every(handle=>handleOnRoute(handle,twice.document.editor.wireRoutes[0].points)));
  assert.equal(removeHandle(inserted,binding.id,0).document.editor.wireRoutes[0].handles,undefined);
  assert.equal(setRoute(inserted,binding.id).document.editor.wireRoutes,undefined);
  assert.deepEqual(plain(reshapeSegment(inserted,binding.id,route.points,1,{x:300,y:0}).document.editor.wireRoutes[0].handles),[{x:300,y:200}],'Segment reshaping translates its waypoints');
  const unrelated=insertHandle(inserted,binding.id,route.points,{x:400,y:380});
  assert.deepEqual(plain(reshapeSegment(unrelated,binding.id,unrelated.document.editor.wireRoutes[0].points,0,{x:0,y:30}).document.editor.wireRoutes[0].handles),[{x:250,y:200},{x:400,y:380}],'Segment reshaping retains unrelated waypoints');
  assert.throws(()=>moveHandle(inserted,binding.id,0,{x:Infinity,y:1}),/bounds/);assert.throws(()=>moveHandle(inserted,binding.id,2,{x:1,y:1}),/existing/);
  assert.deepEqual(plain(inserted.document.editor.wireRoutes[0]),plain(route));
});
test('vehicle projection and tab removal retain only paths whose connections remain visible',()=>{
  const {profileTabDocument,profileVehicleDocument,profileRemoveVehicleTab}=modules.get('./profile-vehicle-tabs');
  const value=setRoute(draft(),binding.id,points);value.document.eligibility.gameIds=['dcs'];
  value.document.vehicleTabs=[{id:'aircraft',gameId:'dcs',vehicleId:'FA-18C_hornet',vehicleName:'Hornet',nodeIds:['sink']}];
  assert.equal(profileTabDocument(value.document).editor.wireRoutes.length,0);
  assert.equal(profileTabDocument(value.document,'aircraft').editor.wireRoutes.length,0);
  assert.equal(profileVehicleDocument(value.document,'aircraft').editor.wireRoutes.length,1);
  const removed=profileRemoveVehicleTab(value.document,value.assignments,'aircraft');
  assert.equal(removed.document.editor.wireRoutes,undefined);assert.equal(profileDocumentError(removed.document),'');
  assert.equal(value.document.editor.wireRoutes.length,1);
});
