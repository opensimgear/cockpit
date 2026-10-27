import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from './test-module-loader.mjs';
const {signalOverview,signalBundleWidth}= (await loadModules(['./profile-signal-overview'])).get('./profile-signal-overview');
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
test('multiple source ports feeding the same terminal are one route, not an inflated weight',()=>{
  const bundles=signalOverview(doc([edge('a','stick','combine','x'),edge('b','stick','combine','y'),edge('c','combine','flight')],['combine']));
  assert.equal(bundles.length,1);assert.equal(bundles[0].lanes.length,1);assert.equal(bundles[0].lanes[0].sources.length,2);
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
