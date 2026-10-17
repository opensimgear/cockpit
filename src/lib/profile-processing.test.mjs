import assert from 'node:assert/strict';import test from 'node:test';import{readFile}from'node:fs/promises';import{loadModules}from'./test-module-loader.mjs';
const modules=await loadModules(['./profiles','./profile-inventory','./profile-graph','./profile-processing']);
const {processingTemplate:make,processingNodeError:error,processingActionPack:pack,processingSummary:summary,processingUnitConversion:conversion,PROCESSING_PRESETS}=modules.get('./profile-processing');
const plain=value=>JSON.parse(JSON.stringify(value));const bool={type:'boolean'};const number=(min=-100,max=100,unit=null)=>({type:'number',min,max,unit});const integer=(min=-32768,max=32767)=>({type:'integer',min,max,unit:null});const state=(schema=bool)=>({delivery:'state',schema,freshnessMs:500,fallback:null});const event=(schema=bool)=>({delivery:'event',schema});
const action=(parameters=[],semantics='one-shot')=>({delivery:'action',parameters,semantics,endpoint:'device/endpoint',readable:semantics==='setter',display:null,ownership:null});
const assertValid=node=>assert.equal(error(node),'',node.name);

test('built-in summaries expose the actual rule parameters and change with timing, polarity and curve edits',()=>{
  const pulse=make('pulse','pulse');const before=summary(pulse);pulse.operation.rule.durationUs=125000;pulse.operation.rule.repeatUs=500000;
  assert.notEqual(summary(pulse),before);assert.ok(summary(pulse).includes('duration 125 ms'));assert.ok(summary(pulse).includes('repeat 500 ms'));assertValid(pulse);
  for(const preset of ['tap','hold','double-tap']){const node=make(preset,preset),before=summary(node);node.operation.rule.holdUs=750000;assert.notEqual(summary(node),before);for(const value of [preset,'hold 750 ms','double 250 ms','pulse 100 ms','explicit inputs: in'])assert.ok(summary(node).includes(value),value);}
  const macro=make('macro','macro');macro.operation.rule.steps[1].afterUs=123456;assertValid(macro);assert.ok(summary(macro).includes('2 steps'));assert.ok(summary(macro).includes('0 µs on → 123.456 ms off'));
  const rules=[
    [{kind:'axis',deadZone:120,invert:true,scale:950,curve:[{input:0,output:123},{input:32767,output:32767}]},['deadzone 120','inverted','scale 950','0→123']],
    [{kind:'axis',deadZone:0,invert:false,scale:1000,curve:[]},['normal','curve linear']],
    [{kind:'split',positive:true},['positive half']],[{kind:'split',positive:false},['negative half']],
    [{kind:'button-axis',range:'bipolar',pressed:32767,released:-32768},['bipolar','pressed 32767, released -32768']],
    [{kind:'threshold',press:20000,release:10000},['press 20000, release 10000']],
    ...['merge','chord','gate','toggle'].map(kind=>[{kind},[kind,'explicit inputs: in']]),
  ];
  for(const [rule,values]of rules){const node=make('identity','summary');node.operation={kind:'builtin',rule,inputs:['in']};const before=plain(node);for(const value of values)assert.ok(summary(node).includes(value),value);assert.deepEqual(plain(node),before);}
});
test('built-in summaries remain bounded UTF-8 previews with complete parameters available in the inspector',()=>{
  const node=make('macro','bounded');node.operation.rule.steps=Array.from({length:16},(_,index)=>({afterUs:index*3000000,pressed:index%2===0}));node.operation.inputs=Array.from({length:8},()=> 'é'.repeat(40));
  const text=summary(node);assert.ok(new TextEncoder().encode(text).length<256);assert.ok(text.endsWith('…'));assert.ok(!text.includes('�'));assert.ok(text.includes('16 steps'));assert.ok(text.includes('duration 45000 ms'));assert.equal(node.operation.rule.steps.length,16);
  const pulse=make('pulse','microseconds');pulse.operation.rule.durationUs=999;assert.ok(summary(pulse).includes('999 µs'));assert.ok(summary(pulse).includes('no repeat'));
});

test('numeric presets preserve raw ranges and expose every conversion, condition and formatting parameter',()=>{
  const numeric=['identity','invert','scale','range-map','unit-convert','deadzone','curve','compare','hysteresis','format','fallback','constant','script'];
  for(const kind of numeric){const node=make(kind,`node-${kind}`,state(number(-100,100,kind==='unit-convert'?'ft':null)));assertValid(node);assert.equal(node.outputs.length,1);assert.equal(node.inputs.length,kind==='constant'?0:1);assert.ok(summary(node).length>0);}
  const range=make('range-map','axis-normalization',state(number(0,65535)),state(integer(0,32767)));assert.deepEqual(plain(range.operation),{kind:'range-map',inputMin:0,inputMax:65535,outputMin:0,outputMax:32767,rounding:'nearest'});assert.equal(range.outputs[0].contract.schema.type,'integer');assert.ok(summary(range).includes('65535'));
  const units=make('unit-convert','altitude',state(number(0,100000,'ft')),state(number(0,30480,'m')));assert.equal(units.operation.from,'ft');assert.equal(units.operation.to,'m');assert.ok(summary(units).includes('ft → m'));assertValid(units);
  const independent=make('range-map','independent',state(number(0,10)));independent.outputs[0].contract.schema.unit='m';assert.equal(independent.inputs[0].contract.schema.unit,null);assert.equal(make('constant','next').outputs[0].contract.schema.type,'boolean');
});
test('boolean logic and timing presets are explicit typed nodes with deterministic named ports and no implicit wires',()=>{
  for(const kind of ['not','gate','any','all','priority','event-edge','select','toggle','pulse','tap','hold','double-tap','macro']){const node=make(kind,`node-${kind}`);assertValid(node);assert.equal(node.inputs.every(port=>port.direction==='input'),true);assert.equal(node.outputs.every(port=>port.direction==='output'),true);}
  const priority=make('priority','axis-priority',state(integer()));assert.deepEqual(plain(priority.operation.order),['first','second']);assert.ok(summary(priority).includes('first → second'));
  const macro=make('macro','macro');assert.deepEqual(plain(macro.operation.rule.steps),[{afterUs:0,pressed:true},{afterUs:100000,pressed:false}]);assert.ok(summary(macro).includes('explicit inputs: in'));
  const gate=make('gate','telemetry-gate',state(number()));assert.deepEqual(plain(gate.inputs.map(port=>port.id)),['gate','value']);assert.equal(gate.outputs[0].contract.schema.type,'number');
});
test('event conversion remains explicit and incompatible type or delivery choices fail before graph changes',()=>{
  assertValid(make('event-to-state','event-state',event(number())));assertValid(make('event-edge','number-change',state(number())));assert.equal(make('event-edge','number-change',state(number())).operation.edge,'change');
  assert.throws(()=>make('event-to-state','wrong',state()),/event source/);assert.throws(()=>make('priority','event-priority',event()),/Event to state/);assert.throws(()=>make('not','number-not',state(number())),/boolean/);assert.throws(()=>make('scale','bool-scale',state()),/integer or number/);assert.throws(()=>make('scale','action-scale',action()),/argument pack/);assert.throws(()=>make('unit-convert','unitless',state(number())),/declared source unit/);assert.throws(()=>make('unit-convert','unknown-unit',state(number(0,100,'unknown'))),/supported explicit/);assert.throws(()=>make('not-supported','unknown'),/supported visible/);
  assert.throws(()=>make('identity','invalid',state({...number(),min:NaN})),/valid declared/);
  const changed=make('event-edge','edge',state(number()));changed.operation.edge='rising';assert.ok(error(changed));
});
test('numeric edits reject overflow, unit mismatch, integer precision loss and invalid range/curve/hysteresis configuration',()=>{
  const scale=make('scale','scale',state(number(0,100)));scale.operation.factor=2;assert.match(error(scale),/output range/);scale.operation.factor=Infinity;assert.ok(error(scale));
  const ranged=make('range-map','range',state(number(0,10)));ranged.operation.inputMin=5;assert.ok(error(ranged));ranged.operation.inputMin=0;ranged.outputs[0].contract.schema.unit='m';assert.match(error(ranged),/units/);
  const precise=make('scale','precision',state(integer(0,10)));precise.operation.factor=.5;assert.match(error(precise),/rounding/);
  const dead=make('deadzone','dead',state(integer()));dead.operation.radius=20;dead.operation.rounding='none';assert.ok(error(dead));
  const curve=make('curve','curve',state(number()));curve.operation.points.reverse();assert.ok(error(curve));
  const hysteresis=make('hysteresis','hysteresis',state(number()));hysteresis.operation.release=hysteresis.operation.press;assert.ok(error(hysteresis));hysteresis.operation.comparison='less-equal';hysteresis.operation.release=100;assertValid(hysteresis);
  const formatted=make('format','format',state(number()));formatted.operation.prefix='\n';assert.ok(error(formatted));
});
test('nearest rounding follows native negative-half behavior and supported conversion values cannot be mutated',()=>{
  const node=make('range-map','negative',state(number(-1,0)),state(integer(-1,0)));node.operation.outputMin=-.5;node.operation.outputMax=0;node.outputs[0].contract.schema.min=0;assert.match(error(node),/output range/);
  assert.deepEqual(plain(conversion('ft','m')),[381/1250,0]);const copy=conversion('ft','m');copy[0]=999;assert.deepEqual(plain(conversion('ft','m')),[381/1250,0]);assert.deepEqual(plain(conversion('any','any')),[1,0]);assert.equal(conversion('__proto__','constructor'),null);assert.equal(conversion('ft','seconds'),null);
});
test('atomic action pack creates every ordered argument and a distinct arbitrary event input even for reserved-looking argument names',()=>{
  const parameters=[{id:'fire',name:'Fire value',schema:bool},{id:'invoke',name:'Intensity',schema:number(0,100)},{id:'trigger',name:'Trigger label',schema:{type:'text',maxBytes:32,encoding:'utf8'}}],native=action(parameters);
  const node=pack('invoke-action',native,'action-pack',event({type:'enum',values:['press','release']}));assertValid(node);assert.deepEqual(plain(node.operation.arguments),['fire','invoke','trigger']);assert.equal(node.inputs[0].id,'fire-1');assert.equal(node.inputs[0].contract.delivery,'event');assert.equal(node.outputs[0].id,'invoke-1');assert.equal(node.outputs[0].contract.delivery,'invocation');assert.equal(node.outputs[0].contract.semantics,'one-shot');assert.equal(node.inputs[2].contract.schema.max,100);assert.ok(summary(node).includes('3 ordered arguments'));node.outputs[0].contract.parameters[0].name='Edited';assert.equal(native.parameters[0].name,'Fire value');
  const zero=pack('zero-argument-command',action(),'action-pack');assertValid(zero);assert.equal(zero.inputs.length,1);assert.deepEqual(plain(zero.operation.arguments),[]);
});
test('setter and one-shot semantics cannot collapse into an implicit raw value-to-action connection',()=>{
  const parameters=[{id:'value',name:'Value',schema:bool}];const setter=pack('setter',action(parameters,'setter'),'set-on-change');assertValid(setter);assert.equal(setter.inputs.length,1);assert.equal(setter.inputs[0].contract.delivery,'state');assert.ok(summary(setter).includes('On argument change'));
  assert.throws(()=>pack('wrong',action(parameters),'set-on-change'),/One-shot/);assert.throws(()=>pack('wrong',action(parameters),'action-pack',state()),/event trigger/);assert.throws(()=>pack('wrong',state(),'action-pack'),/advertised action/);
  const invalid=pack('invalid',action(parameters),'action-pack');invalid.inputs[1].contract={delivery:'event',schema:bool};assert.ok(error(invalid));invalid.inputs[1].contract=state();invalid.operation.arguments=['wrong'];assert.ok(error(invalid));
});
test('scripts retain bounded source, declared ports and typed parameters with an empty transaction starter',()=>{
  const node=make('script','script',state(number()));assertValid(node);assert.equal(node.inputs[0].id,'in');assert.equal(node.outputs[0].id,'out');assert.ok(node.operation.source.includes('transaction(ctx.state, #{}, [])'));assert.ok(!node.operation.source.includes('button('));assert.ok(summary(node).includes('1 declared inputs'));
  node.operation.parameters={threshold:5,label:'text',enabled:true};assertValid(node);node.operation.parameters.threshold=NaN;assert.ok(error(node));node.operation.parameters={};node.operation.source='é'.repeat(8193);assert.ok(error(node));
});
test('declared port and builtin bounds remain consistent with native validation',()=>{
  const node=make('not','not');node.inputs[0].id='out';assert.ok(error(node));node.inputs[0].id='in';node.inputs[0].direction='output';assert.ok(error(node));
  const pulse=make('pulse','pulse');pulse.operation.rule.repeatUs=pulse.operation.rule.durationUs;assert.ok(error(pulse));pulse.operation.rule.repeatUs=200000;assertValid(pulse);
  const macro=make('macro','macro');macro.operation.rule.steps[1].pressed=true;assert.ok(error(macro));
  const enumNode=make('event-to-state','enum',event({type:'enum',values:['one','two']}));enumNode.outputs[0].contract.schema.values.reverse();assert.ok(error(enumNode));
  assert.equal(PROCESSING_PRESETS.length,27);
});
test('native-validated fixed authoring fixture retains the exact generated preset and atomic invocation contracts',async()=>{
  const fixture=JSON.parse(await readFile(new URL('../../tests/fixtures/profile-authoring/visible-presets.json',import.meta.url),'utf8'));
  assert.equal(fixture.processingNodes.length,29);assert.equal(fixture.bindings.length,66);
  for(const node of fixture.processingNodes){let generated;const kind=node.id.slice('preset-'.length);if(kind==='action-pack'||kind==='set-on-change'){const binding=fixture.bindings.find(binding=>binding.from.nodeId===node.id),sink=fixture.resources.find(resource=>resource.id===binding.to.nodeId).ports.find(port=>port.id===binding.to.portId);generated=pack(node.id,sink.contract,kind);}else generated=make(kind,node.id,node.inputs[0]?.contract??node.outputs[0].contract);assert.deepEqual(plain(generated),node);assertValid(node);}
  assert.ok(!fixture.processingNodes.find(node=>node.operation.kind==='script').operation.source.includes('on_timer'));
});
