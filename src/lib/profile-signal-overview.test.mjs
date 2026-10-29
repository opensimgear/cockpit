import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {loadModules} from './test-module-loader.mjs';
const {signalOverview,destinationOverview,destinationOverviewId,signalBundleWidth}= (await loadModules(['./profile-signal-overview'])).get('./profile-signal-overview');
const edge=(id,from,to,output=id,input=id)=>({id,from:{nodeId:from,portId:output},to:{nodeId:to,portId:input}});
const doc=(bindings,transforms=[])=>({resources:[{id:'stick'},{id:'pedals'}],virtualOutputs:[{id:'flight'}],desktopOutputs:[{id:'keyboard'}],processingNodes:transforms.map(id=>({id})),bindings});
const plain=value=>JSON.parse(JSON.stringify(value));

test('direct signals and transformed signals share a resource-pair arrow without losing endpoints',()=>{
  const source=doc([edge('roll','stick','flight'),edge('pitch-in','stick','scale'),edge('pitch-out','scale','flight'),edge('rudder','pedals','flight')],['scale']);
  const before=JSON.stringify(source),bundles=signalOverview(source),stick=bundles.find(bundle=>bundle.binding.from.nodeId==='stick');
  assert.equal(bundles.length,2);assert.equal(stick.lanes.length,2);
  assert.deepEqual(plain(stick.lanes[1].transforms),['scale']);
  assert.equal(stick.lanes[1].id,'pitch-out');
  assert.deepEqual(plain(stick.lanes[1].bindingIds),['pitch-out','pitch-in']);
  assert.equal(JSON.stringify(source),before);
});
test('an unfinished fan-out branch is surfaced without suppressing a completed resource-to-virtual route',()=>{
  const source={id:'resource-1'},virtual={id:'virtual-1'};
  const document={resources:[source],virtualOutputs:[virtual],desktopOutputs:[],processingNodes:[{id:'transform-1'},{id:'transform-2'}],bindings:[
    edge('route-1','resource-1','transform-1','port-1','in'),
    edge('route-3','resource-1','transform-2','port-1','in'),
    edge('route-2','transform-2','virtual-1','out','button-1'),
  ]};
  const [bundle]=signalOverview(document);
  assert.equal(signalOverview(document).length,1);
  assert.deepEqual(plain([bundle.binding.from.nodeId,bundle.binding.to.nodeId]),['resource-1','virtual-1']);
  assert.deepEqual(plain(bundle.lanes[0].transforms),['transform-2']);
  assert.deepEqual(plain(bundle.lanes[0].bindingIds),['route-2','route-3']);
  assert.equal(document.bindings.find(binding=>binding.id==='route-1').to.nodeId,'transform-1');
});
test('shared transforms retain identity on each consumer and multi-input lanes preserve every source',()=>{
  const bundles=signalOverview(doc([edge('a','stick','mix'),edge('b','pedals','mix'),edge('c','mix','scale'),edge('d','scale','flight'),edge('e','scale','keyboard')],['mix','scale']));
  assert.equal(bundles.length,4);
  for(const bundle of bundles){assert.equal(bundle.lanes.length,1);assert.deepEqual(plain(bundle.lanes[0].transforms),['mix','scale']);assert.deepEqual(plain(bundle.lanes[0].sources.map(source=>source.nodeId)),['stick','pedals']);}
});
test('shared Common stages retain separate rows for each vehicle terminal',()=>{
  const document=doc([edge('input','stick','common'),edge('a-input','common','a-stage'),edge('a-output','a-stage','a'),edge('b-input','common','b-stage'),edge('b-output','b-stage','b'),edge('base-output','common','flight')],['common','a-stage','b-stage']);
  document.resources.push({id:'a'},{id:'b'});
  document.vehicleTabs=[{id:'vehicle-a',nodeIds:['a','a-stage']},{id:'vehicle-b',nodeIds:['b','b-stage']}];
  const before=plain(document),bundles=signalOverview(document),lanes=destinationOverview(bundles).flatMap(bundle=>bundle.lanes);
  assert.deepEqual(plain(bundles.map(bundle=>[bundle.binding.from.nodeId,bundle.binding.to.nodeId])),[['stick','a'],['stick','b'],['stick','flight']]);
  assert.deepEqual(plain(lanes.map(lane=>[lane.id,lane.transforms,lane.bindingIds])),[
    ['a-output',['common','a-stage'],['a-output','a-input','input']],
    ['b-output',['common','b-stage'],['b-output','b-input','input']],
    ['base-output',['common'],['base-output','input']],
  ]);
  assert.deepEqual(plain(document),before);
});
test('exclusive direct and transformed alternatives keep their own terminal into a shared input',()=>{
  const document=doc([edge('a-route','a','flight','out','axis'),edge('b-input','b','b-stage','out','in'),edge('b-route','b-stage','flight','out','axis'),edge('base-route','stick','flight','out','button')],['b-stage']);
  document.resources.push({id:'a'},{id:'b'});
  document.vehicleTabs=[{id:'vehicle-a',nodeIds:['a']},{id:'vehicle-b',nodeIds:['b','b-stage']}];
  const bundles=signalOverview(document),[overview]=destinationOverview(bundles);
  assert.deepEqual(plain(bundles.map(bundle=>bundle.binding.from.nodeId)),['a','b','stick']);
  assert.deepEqual(plain(overview.lanes.map(lane=>({id:lane.id,sources:lane.sources.map(source=>source.nodeId),transforms:lane.transforms,bindingIds:lane.bindingIds}))),[
    {id:'a-route',sources:['a'],transforms:[],bindingIds:['a-route']},
    {id:'b-route',sources:['b'],transforms:['b-stage'],bindingIds:['b-route','b-input']},
    {id:'base-route',sources:['stick'],transforms:[],bindingIds:['base-route']},
  ]);
  assert.deepEqual(plain(overview.lanes.slice(0,2).map(lane=>lane.destination)),[{nodeId:'flight',portId:'axis'},{nodeId:'flight',portId:'axis'}]);
});
test('the complete Feature Lab graph preserves both aircraft and Common signals together',async()=>{
  const document=JSON.parse(await readFile(new URL('../../examples/dcs-bios/feature-lab.profile.json',import.meta.url),'utf8')),before=plain(document);
  const bundles=signalOverview(document),lanes=destinationOverview(bundles).flatMap(bundle=>bundle.lanes);
  assert.equal(lanes.length,40);
  for(const aircraft of ['ah6-dcs','hornet-dcs'])for(const [from,to] of [['aux-panel',aircraft],[aircraft,'aux-panel']]){
    const bundle=bundles.find(bundle=>bundle.binding.from.nodeId===from&&bundle.binding.to.nodeId===to);
    assert.equal(bundle?.lanes.length,1,'Keep the aircraft lighting row '+from+' → '+to);const lane=bundle.lanes[0];
    assert.deepEqual(plain(lane.sources.map(source=>source.nodeId)),[from]);assert.equal(lane.destination.nodeId,to);
    const scopes=document.vehicleTabs.filter(tab=>[lane.destination.nodeId,...lane.sources.map(source=>source.nodeId),...lane.transforms].some(node=>tab.nodeIds.includes(node)));
    assert.equal(scopes.length,1,'Aircraft lighting rows retain their mutually exclusive scope');
  }
  assert.ok(lanes.some(lane=>lane.sources[0].nodeId==='right-stick'&&lane.destination.nodeId==='virtual-flight'));
  const panel=document.resources.find(resource=>resource.id==='aux-panel');assert.equal(panel.ports.length,8);
  assert.equal(panel.ports.filter(port=>port.direction==='output').length,4);assert.equal(panel.ports.filter(port=>port.direction==='input').length,4);
  assert.ok(!panel.ports.some(port=>port.id==='altitude-text'));assert.ok(!document.processingNodes.some(node=>node.operation.kind==='format-text'));
  for(const port of panel.ports.filter(port=>port.direction==='input'&&port.contract.schema.type==='number'))assert.equal(port.contract.schema.unit,null);
  assert.deepEqual([...new Set(lanes.filter(lane=>lane.destination.nodeId===panel.id).map(lane=>lane.destination.portId))].sort(),['aircraft-brightness','altitude-m','altitude-warning','rudder-percent']);
  assert.deepEqual(document,before);
});
test('multiple source ports feeding the same terminal are one route, not an inflated weight',()=>{
  const bundles=signalOverview(doc([edge('a','stick','combine','x'),edge('b','stick','combine','y'),edge('c','combine','flight')],['combine']));
  assert.equal(bundles.length,1);assert.equal(bundles[0].lanes.length,1);assert.equal(bundles[0].lanes[0].sources.length,2);
});
test('destination overview combines different source bundles and preserves real lane identities',()=>{
  const bundles=signalOverview(doc([edge('roll','stick','flight'),edge('rudder','pedals','flight')]));
  const [overview]=destinationOverview(bundles);
  assert.equal(destinationOverviewId('flight'),'destination:"flight"');
  assert.equal(overview.id,'destination:"flight"');
  assert.deepEqual(plain(overview.binding),{id:'destination:"flight"',from:{nodeId:'stick',portId:''},to:{nodeId:'flight',portId:''}});
  assert.deepEqual(plain(overview.lanes.map(lane=>lane.id)),['roll','rudder']);
  assert.deepEqual(plain(overview.lanes.map(lane=>lane.bindingIds)),[['roll'],['rudder']]);
});
test('a multi-input route duplicated across source arrows appears once per destination',()=>{
  const bundles=signalOverview(doc([edge('a','stick','mix'),edge('b','pedals','mix'),edge('route','mix','flight')],['mix']));
  assert.equal(bundles.length,2);
  const [overview]=destinationOverview(bundles);
  assert.equal(overview.lanes.length,1);
  assert.equal(overview.lanes[0].id,'route');
  assert.deepEqual(plain(overview.lanes[0].sources.map(source=>source.nodeId)),['stick','pedals']);
  assert.deepEqual(plain(overview.lanes[0].bindingIds),['route','a','b']);
});
test('destination overview keeps separate destinations, retains empty bundles, and leaves input unchanged',()=>{
  const source=[
    ...signalOverview(doc([edge('one','stick','flight'),edge('two','stick','keyboard')])).map(bundle=>({...bundle,lanes:[...bundle.lanes]})),
    {id:'empty-source',binding:{id:'empty-source',from:{nodeId:'pedals',portId:''},to:{nodeId:'empty-destination',portId:''}},lanes:[]},
  ];
  const before=JSON.stringify(source),overview=destinationOverview(source);
  assert.equal(JSON.stringify(source),before);
  assert.deepEqual(plain(overview.map(bundle=>bundle.binding.to.nodeId)),['flight','keyboard','empty-destination']);
  assert.deepEqual(plain(overview.map(bundle=>bundle.lanes.map(lane=>lane.id))),[['one'],['two'],[]]);
});
test('external resources stop traversal; drafts and constants do not invent resource-to-resource wires',()=>{
  const source=doc([edge('a','stick','flight'),edge('b','flight','keyboard'),edge('c','constant','keyboard'),edge('draft','pedals','unfinished')],['constant','unfinished']);
  const bundles=signalOverview(source);
  assert.deepEqual(plain(bundles.map(bundle=>[bundle.binding.from.nodeId,bundle.binding.to.nodeId])),[['stick','flight'],['flight','keyboard']]);
});
test('reconvergent DAGs are bounded by edges, not exponential path enumeration',()=>{
  const bindings=[edge('start','stick','join-0')],nodes=['join-0'];
  for(let i=1;i<=30;i++){nodes.push(`left-${i}`,`right-${i}`,`join-${i}`);bindings.push(edge(`l${i}`,`join-${i-1}`,`left-${i}`),edge(`r${i}`,`join-${i-1}`,`right-${i}`),edge(`lj${i}`,`left-${i}`,`join-${i}`),edge(`rj${i}`,`right-${i}`,`join-${i}`));}
  bindings.push(edge('end','join-30','flight'));
  const bundles=signalOverview(doc(bindings,nodes));assert.equal(bundles.length,1);assert.equal(bundles[0].lanes.length,1);assert.equal(bundles[0].lanes[0].transforms.length,nodes.length);assert.equal(bundles[0].lanes[0].bindingIds.length,bindings.length);
});
test('invalid draft cycles terminate and proportional width has no text dependency',()=>{
  const bundles=signalOverview(doc([edge('a','stick','first'),edge('b','first','second'),edge('c','second','first'),edge('d','second','flight')],['first','second']));
  assert.equal(bundles[0].lanes[0].transforms.length,2);
  for(const maximum of [1,4,24])assert.equal(signalBundleWidth(4,maximum),4*signalBundleWidth(1,maximum));
  assert.ok(signalBundleWidth(24,24)<=14);
});
