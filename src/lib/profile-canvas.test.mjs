import test from 'node:test';import assert from 'node:assert/strict';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-editor','./profile-canvas','./profile-graph-history']);const{blankProfile,seedProfile,PROFILE_BLOCK_WIDTH,PROFILE_BLOCK_HEIGHT}=modules.get('./profile-editor');const{canvasNodes,canvasZoom,canvasPan,canvasFit,canvasMove,canvasStatus,canvasTransports,canvasSearch,scalarLabel,portContractLabel,canvasGameId}=modules.get('./profile-canvas');
const id='11111111-1111-4111-8111-111111111111';const plain=v=>JSON.parse(JSON.stringify(v));
const resource={id:'native:can-usb',name:'Panel',resourceType:'osg-module',availability:'online',connections:[{route:{transport:'can'}},{route:{transport:'usb'}}],attributes:{gateway:'G'},portCount:20000};

test('controller effects summarize exact authored device choices independently of inventory and runtime observations',()=>{
  const {canvasControllerEffects}=modules.get('./profile-canvas'),document=blankProfile(id);
  assert.deepEqual(plain(canvasControllerEffects(document)),{enabled:0,disabled:0,configured:0,hidden:0,virtual:0});
  document.resources=['enabled','disabled','configured'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));
  document.resources.push({id:'game',name:'Game',kind:'game',required:false,ports:[]},{id:'companion',name:'Companion',kind:'companion',required:false,ports:[]});
  document.overrides=[{resourceId:'enabled',enabled:true},{resourceId:'disabled',enabled:false}];document.visibility=[{resourceId:'disabled',mode:'hidden-while-active'},{resourceId:'configured',mode:'visible'}];
  document.virtualOutputs=[{id:'virtual',name:'Virtual',required:false,ports:[],layout:{buttons:['fire'],axes:[],hats:[]}}];
  const before=plain(document);assert.deepEqual(plain(canvasControllerEffects(document)),{enabled:1,disabled:1,configured:1,hidden:1,virtual:1});assert.deepEqual(plain(document),before);
  document.overrides=[];assert.deepEqual(plain(canvasControllerEffects(document)),{enabled:0,disabled:0,configured:3,hidden:1,virtual:1});
});

test('named editor groups frame their current members and edit independently of graph semantics',()=>{
  const {canvasSaveGroup,canvasDeleteGroup,canvasGroups,canvasTogglePorts,canvasPortCollapsed}=modules.get('./profile-canvas');
  const doc=blankProfile(id);doc.resources=['a','b','c'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));doc.editor.positions=[{nodeId:'a',x:100,y:100},{nodeId:'b',x:500,y:300}];
  const grouped=canvasSaveGroup(doc,' Controls ',['a','b']);assert.equal(grouped.editor.groups[0].id,'group-1');assert.equal(grouped.editor.groups[0].name,'Controls');assert.equal(doc.editor.groups,undefined);
  const frames=canvasGroups(grouped,canvasNodes(grouped,[],[]));assert.equal(frames.length,1);assert.equal(frames[0].x,80);assert.equal(frames[0].y,56);assert.equal(frames[0].width,400+PROFILE_BLOCK_WIDTH+40);assert.equal(frames[0].height,200+PROFILE_BLOCK_HEIGHT+64);
  const moved=canvasMove(grouped,'a',{x:-100,y:-50}),frame=canvasGroups(moved,canvasNodes(moved,[],[]))[0];assert.equal(frame.x,-120);assert.equal(frame.y,-94);
  const second=canvasSaveGroup(grouped,'Other',['c']);assert.equal(second.editor.groups[1].id,'group-2');
  const updated=canvasSaveGroup(second,'Renamed',['b'],'group-1');assert.deepEqual(plain(updated.editor.groups.find(group=>group.id==='group-1')),{id:'group-1',name:'Renamed',members:['b']});assert.equal(updated.resources.length,3);assert.deepEqual(plain(updated.bindings),[]);
  for(const [name,members,group]of [['', ['a']],['Good',['missing']],['Good',['a','a']],['Good',['c']],['Good',['a'],'missing']])assert.throws(()=>canvasSaveGroup(second,name,members,group));
  assert.throws(()=>canvasDeleteGroup(doc,'missing'),/existing/);assert.equal(canvasDeleteGroup(grouped,'group-1').editor.groups,undefined);assert.equal(canvasGroups({editor:{groups:[{id:'empty',name:'Empty',members:[]}]}},[]).length,1);
  let collapsed=doc;for(const group of ['outputs','inputs','actions']){collapsed=canvasTogglePorts(collapsed,'a',group);assert.equal(canvasPortCollapsed(collapsed,'a',group),true);}assert.equal(canvasPortCollapsed(doc,'a','outputs'),false);assert.equal(collapsed.editor.collapsedPortGroups.length,3);
  collapsed=canvasTogglePorts(collapsed,'a','outputs');assert.equal(canvasPortCollapsed(collapsed,'a','outputs'),false);assert.equal(canvasPortCollapsed(collapsed,'a','inputs'),true);collapsed=canvasTogglePorts(canvasTogglePorts(collapsed,'a','inputs'),'a','actions');assert.equal(collapsed.editor.collapsedPortGroups,undefined);assert.throws(()=>canvasTogglePorts(doc,'missing','outputs'));
});

test('empty group creation finds a disjoint frame and bounds persist independently of member positions',()=>{
  const {canvasCreateGroup,canvasRenameGroup,canvasGroups,canvasDeleteGroup,canvasGroupAt}=modules.get('./profile-canvas');
  const document=blankProfile(id),first=canvasCreateGroup(document,' Controls ',{x:20,y:30}),second=canvasCreateGroup(first,'Outputs',{x:20,y:30});
  assert.equal(document.editor.groups,undefined);assert.equal(first.editor.groups[0].name,'Controls');assert.deepEqual(plain(first.editor.groups[0].members),[]);
  const frames=canvasGroups(second,canvasNodes(second,[],[]));assert.equal(frames.length,2);assert.ok(frames[1].x>=frames[0].x+frames[0].width);assert.deepEqual(plain(frames[0].bounds),{x:20,y:30,width:440,height:360});
  assert.equal(canvasGroupAt(second,{x:60,y:50}),null);assert.equal(canvasGroupAt(second,{x:60,y:90}),'group-1');assert.equal(canvasGroupAt(second,{x:-40,y:-40}),null);assert.throws(()=>canvasGroupAt(second,{x:NaN,y:0}),/finite/);
  const renamed=canvasRenameGroup(second,'group-1',' Renamed ');assert.equal(renamed.editor.groups[0].name,'Renamed');assert.deepEqual(plain(renamed.editor.groups[0].bounds),plain(first.editor.groups[0].bounds));assert.throws(()=>canvasRenameGroup(second,'missing','Other'),/existing/);assert.throws(()=>canvasRenameGroup(second,'group-1',' '));assert.throws(()=>canvasCreateGroup(document,'Controls',{x:Infinity,y:0}),/finite/);
  assert.deepEqual(plain(canvasDeleteGroup(second,'group-1').editor.groups[0]),plain(second.editor.groups[1]));
});

test('dragging a block between groups and outside updates membership and position in one undo action',()=>{
  const {canvasCreateGroup,canvasMoveIntoGroup,canvasDeleteGroup}=modules.get('./profile-canvas'),{ProfileGraphHistory}=modules.get('./profile-graph-history');
  const document=blankProfile(id);document.resources=[{id:'wheel',name:'Wheel',kind:'device',required:false,ports:[]}];document.editor.positions=[{nodeId:'wheel',x:-400,y:0}];
  const initial=canvasCreateGroup(canvasCreateGroup(document,'Controls'),'Outputs'),history=new ProfileGraphHistory({document:initial,assignments:[]});
  const drop=(position)=>history.commit({document:canvasMoveIntoGroup(history.current.document,'wheel',position),assignments:[]},'Move component');
  drop({x:40,y:70});assert.deepEqual(plain(history.current.document.editor.groups.map(group=>group.members)),[['wheel'],[]]);
  drop({x:504,y:70});assert.deepEqual(plain(history.current.document.editor.groups.map(group=>group.members)),[[],['wheel']]);assert.equal(history.current.document.editor.positions[0].x,504);
  history.undo();assert.deepEqual(plain(history.current.document.editor.groups.map(group=>group.members)),[['wheel'],[]]);assert.equal(history.current.document.editor.positions[0].x,40);history.redo();
  drop({x:-700,y:70});assert.deepEqual(plain(history.current.document.editor.groups.map(group=>group.members)),[[],[]]);assert.equal(history.current.document.resources.length,1);history.undo();
  const removed=canvasDeleteGroup(history.current.document,'group-2');assert.equal(removed.resources.length,1);assert.equal(removed.editor.positions[0].x,504);assert.deepEqual(plain(removed.bindings),[]);assert.equal(removed.editor.groups.length,1);
  assert.throws(()=>canvasMoveIntoGroup(initial,'missing',{x:0,y:0}),/existing/);assert.deepEqual(plain(initial.editor.groups.map(group=>group.members)),[[],[]]);
});

test('moving a group moves its members, prevents overlap and guards resize bounds without changing routes',()=>{
  const {canvasCreateGroup,canvasMoveIntoGroup,canvasMoveGroup,canvasResizeGroup,canvasSetGroupBounds}=modules.get('./profile-canvas');
  const document=blankProfile(id);document.resources=[{id:'wheel',name:'Wheel',kind:'device',required:false,ports:[]}];
  const initial=canvasMoveIntoGroup(canvasCreateGroup(canvasCreateGroup(document,'Controls'),'Outputs'),'wheel',{x:40,y:70});
  const moved=canvasMoveGroup(initial,'group-1',{x:-100,y:100});assert.equal(moved.editor.groups[0].bounds.x,-100);assert.deepEqual(plain(moved.editor.positions[0]),{nodeId:'wheel',x:-60,y:170});assert.deepEqual(plain(moved.editor.groups[0].members),['wheel']);assert.deepEqual(plain(moved.bindings),[]);
  assert.throws(()=>canvasMoveGroup(initial,'group-1',{x:300,y:0}),/overlap/);assert.deepEqual(plain(initial.editor.groups[0].bounds),{x:0,y:0,width:440,height:360});
  const resized=canvasResizeGroup(initial,'group-1',{width:440,height:500});assert.equal(resized.editor.groups[0].bounds.height,500);
  assert.throws(()=>canvasResizeGroup(initial,'group-1',{width:600,height:360}),/overlap/);assert.throws(()=>canvasResizeGroup(initial,'group-1',{width:100,height:200}));assert.throws(()=>canvasResizeGroup(initial,'missing',{width:400,height:400}),/existing/);assert.throws(()=>canvasMoveGroup(initial,'missing',{x:0,y:0}),/existing/);assert.throws(()=>canvasMoveGroup(initial,'group-1',{x:NaN,y:0}),/finite/);
  assert.throws(()=>canvasSetGroupBounds(initial,'group-1',{x:0,y:210,width:440,height:360}),/large enough/);assert.throws(()=>canvasSetGroupBounds(initial,'missing',{x:0,y:0,width:440,height:360}),/existing/);
});

test('legacy groups stay unchanged on display and become stationary disjoint frames on the first authored drop',()=>{
  const {canvasGroups,canvasMoveIntoGroup}=modules.get('./profile-canvas'),document=blankProfile(id);
  document.resources=['a','b'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));document.editor.positions=[{nodeId:'a',x:20,y:50},{nodeId:'b',x:40,y:70}];document.editor.groups=[{id:'a-group',name:'A',members:['a']},{id:'b-group',name:'B',members:['b']}];
  const before=plain(document),frames=canvasGroups(document,canvasNodes(document,[],[]));assert.ok(frames[0].x+frames[0].width>frames[1].x);assert.deepEqual(plain(document),before);
  const next=canvasMoveIntoGroup(document,'a',{x:-500,y:-500});assert.deepEqual(plain(document),before);assert.deepEqual(plain(next.editor.groups[0].members),[]);assert.ok(next.editor.groups.every(group=>group.bounds));assert.ok(next.editor.groups[1].bounds.x>=next.editor.groups[0].bounds.x+next.editor.groups[0].bounds.width);assert.equal(next.editor.positions.find(row=>row.nodeId==='b').x,372);
  const stationary=plain(next.editor.groups[0].bounds),again=canvasMoveIntoGroup(next,'a',{x:-600,y:-600});assert.deepEqual(plain(again.editor.groups[0].bounds),stationary);
});

test('group drops freeze implicit block positions so unrelated components do not jump with the moved block',()=>{
  const {canvasMoveIntoGroup,canvasCreateGroup}=modules.get('./profile-canvas'),document=blankProfile(id);
  document.resources=['a','b','c'].map(id=>({id,name:id,kind:'device',required:false,ports:[]}));
  const before=canvasNodes(document,[],[]),moved=canvasMoveIntoGroup(document,'a',{x:900,y:1200}),after=canvasNodes(moved,[],[]);
  const point=node=>({x:node.position.x,y:node.position.y});
  for(const id of ['b','c'])assert.deepEqual(point(after.find(node=>node.id===id)),point(before.find(node=>node.id===id)));
  assert.equal(document.editor.positions.length,0);assert.equal(moved.editor.positions.length,3);
  const grouped=canvasCreateGroup(document,'Controls');for(const node of canvasNodes(grouped,[],[]))assert.deepEqual(point(node),point(before.find(original=>original.id===node.id)));
});
test('canvas retains saved blocks and exact graph objects when inventory goes offline or disappears',()=>{const seed=seedProfile(id,[resource]);const online=canvasNodes(seed.document,seed.assignments,[resource]);assert.equal(canvasStatus(online[0]),'Online');assert.deepEqual(plain(canvasTransports(online[0])),['CAN','USB']);const offline=canvasNodes(seed.document,seed.assignments,[]);assert.equal(offline.length,1);assert.match(canvasStatus(offline[0]),/Offline/);assert.equal(offline[0].assignment.sourceId,resource.id);assert.deepEqual(plain(offline[0].position),plain(online[0].position));assert.equal(canvasStatus(canvasNodes(seed.document,[],[])[0]),'Unassigned');assert.equal(canvasStatus(canvasNodes(seed.document,seed.assignments,[{...resource,availability:'unavailable'}])[0]),'Unavailable / dormant');assert.equal(canvasSearch(online,'usb').length,1);assert.equal(canvasSearch(online,'companion').length,0);assert.equal(canvasSearch(online,'  ').length,1);});
test('missing positions are placed safely below saved nodes and processing/virtual types stay distinct',()=>{const doc=blankProfile(id);doc.resources=[{id:'saved',name:'Saved',kind:'device',required:false,ports:[]},{id:'new',name:'New',kind:'game',required:false,ports:[]}];doc.processingNodes=[{id:'transform',name:'Convert',inputs:[],outputs:[],operation:{kind:'identity'}}];doc.virtualOutputs=[{id:'vjoy',name:'Output',required:false,ports:[],layout:{buttons:[],axes:[],hats:[]}}];doc.editor.positions=[{nodeId:'saved',x:30,y:800}];const nodes=canvasNodes(doc,[],[]);assert.equal(nodes[0].position.y,800);assert.ok(nodes[1].position.y>800+PROFILE_BLOCK_HEIGHT);assert.equal(canvasStatus(nodes[2]),'Explicit transformation');assert.equal(canvasStatus(nodes[3]),'Profile output');assert.equal(nodes[1].category,'Game integration');const moved=canvasMove(doc,'saved',{x:1e9,y:-1e9});assert.deepEqual(plain(moved.editor.positions.find(p=>p.nodeId==='saved')),{nodeId:'saved',x:1e6,y:-1e6});assert.equal(doc.editor.positions[0].y,800);assert.throws(()=>canvasMove(doc,'missing',{x:0,y:0}),/existing/);assert.throws(()=>canvasMove(doc,'saved',{x:NaN,y:0}),/finite/);});
test('pan and anchored zoom remain finite and bounded without altering world geometry',()=>{const view={x:20,y:40,zoom:1};const at={x:200,y:150};const next=canvasZoom(view,2,at);assert.equal((at.x-view.x)/view.zoom,(at.x-next.x)/next.zoom);assert.equal((at.y-view.y)/view.zoom,(at.y-next.y)/next.zoom);assert.equal(canvasZoom(view,100,at).zoom,4);assert.equal(canvasZoom(view,0.001,at).zoom,0.1);assert.deepEqual(plain(canvasPan(view,{x:1e9,y:-1e9})),{x:1e6,y:-1e6,zoom:1});assert.throws(()=>canvasZoom(view,-1,at));assert.throws(()=>canvasPan(view,{x:NaN,y:0}));assert.throws(()=>canvasZoom({...view,zoom:0},1,at));});
test('fit includes every block and handles empty/small viewports without nonfinite zoom',()=>{const nodes=[{position:{x:-100,y:-20}},{position:{x:1000,y:800}}];const view=canvasFit(nodes,1000,700);for(const node of nodes){assert.ok(node.position.x*view.zoom+view.x>=31);assert.ok((node.position.x+PROFILE_BLOCK_WIDTH)*view.zoom+view.x<=969);assert.ok(node.position.y*view.zoom+view.y>=31);assert.ok((node.position.y+PROFILE_BLOCK_HEIGHT)*view.zoom+view.y<=669);}assert.deepEqual(plain(canvasFit([],400,300)),{x:0,y:0,zoom:1});assert.equal(canvasFit(nodes,20,20).zoom,0.1);assert.throws(()=>canvasFit(nodes,0,10));});

test('compact transforms size to named rows and use their actual bounds for fit and group drops',()=>{
  const {canvasNodeSize,canvasNodeAnchor,canvasCreateGroup,canvasMoveIntoGroup,canvasResizeGroup,canvasGroups,canvasSaveGroup}=modules.get('./profile-canvas');
  const contract={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null},port=(id,direction)=>({id,name:id,direction,contract});
  const processing={id:'transform',name:'Identity',operation:{kind:'identity'},inputs:[port('value','input')],outputs:[port('result','output')]},document=blankProfile(id);document.processingNodes=[processing];
  const node=canvasNodes(document,[],[])[0];assert.equal(node.category,'Transform');assert.deepEqual(plain(canvasNodeSize(node)),{width:224,height:114});
  assert.deepEqual(plain(canvasNodeAnchor({...node,position:{x:40,y:50}},'input','value')),{x:40,y:142});assert.deepEqual(plain(canvasNodeAnchor({...node,position:{x:40,y:50}},'output','result')),{x:264,y:142});
  const multi={...node,position:{x:40,y:50},processing:{...processing,inputs:[port('first','input'),port('second','input'),port('third','input')]}};
  assert.deepEqual(plain(canvasNodeSize(multi)),{width:224,height:170});assert.deepEqual(plain(canvasNodeAnchor(multi,'input','third')),{x:40,y:198});assert.deepEqual(plain(canvasNodeAnchor(multi,'output','result')),{x:264,y:142});
  assert.deepEqual(plain(canvasNodeAnchor({kind:'resource',position:{x:40,y:50}},'output','anything')),{x:328,y:218});
  const fitted=canvasFit([{...node,position:{x:0,y:0}}],400,300);assert.equal(fitted.zoom,1.5);assert.equal(fitted.x,32);assert.equal(fitted.y,64.5);
  const grouped=canvasMoveIntoGroup(canvasCreateGroup(document,'Transforms'),'transform',{x:300,y:70});assert.deepEqual(plain(grouped.editor.groups[0].members),['transform']);assert.equal(canvasResizeGroup(grouped,'group-1',{width:430,height:300}).editor.groups[0].bounds.width,430);
  const legacy={...document,processingNodes:[processing,{...processing,id:'second'}],editor:{...document.editor,positions:[{nodeId:'transform',x:0,y:50},{nodeId:'second',x:300,y:50}]}};
  assert.equal(canvasGroups(canvasSaveGroup(legacy,'Compact',['transform','second']),canvasNodes(legacy,[],[]))[0].width,564);
});
test('typed port labels preserve units, unknowns, ordered arguments and game settings identity metadata',()=>{assert.equal(scalarLabel({type:'integer',min:0,max:100,unit:'ft'}),'integer 0…100 ft');assert.equal(scalarLabel({type:'number',min:0,max:1,unit:null}),'number 0…1');assert.match(scalarLabel({type:'text',maxBytes:50,encoding:'utf8'}),/50 bytes/);assert.equal(scalarLabel({type:'enum',values:['off','on']}),'enum · off / on');assert.equal(scalarLabel({type:'rgb'}),'rgb');assert.match(portContractLabel({delivery:'state',schema:{type:'boolean'},freshnessMs:500,fallback:null}),/fallback unknown/);assert.match(portContractLabel({delivery:'state',schema:{type:'boolean'},freshnessMs:500,fallback:false}),/fallback false/);assert.match(portContractLabel({delivery:'event',schema:{type:'boolean'}}),/^event/);assert.match(portContractLabel({delivery:'action',semantics:'one-shot',parameters:[]}),/no arguments/);assert.match(portContractLabel({delivery:'invocation',semantics:'setter',parameters:[{id:'value',name:'Value',schema:{type:'boolean'}}]}),/Value: boolean/);for(const gameId of ['dcs','assetto-corsa-rally','x-plane-11','x-plane-12'])assert.equal(canvasGameId({observation:{attributes:{gameId}}}),gameId);assert.equal(canvasGameId({name:'DCS',observation:{attributes:{gameId:'other'}}}),null);assert.equal(canvasGameId({name:'DCS'}),null);});
