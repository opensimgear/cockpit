import assert from 'node:assert/strict';
import test from 'node:test';
import { graphCards, graphWires, graphPositions, inputPins, pinExpression } from './model.ts';

function fixture(){return {
  id:'graph',title:'Graph',size:{width:100,height:100},parameters:[],
  behavior:{version:1,signals:[{id:'intent',type:'boolean',source:'local',initial:false},{id:'input',type:'number',source:'input',initial:0,min:0,max:10},{id:'observed',type:'boolean',source:'feedback',initial:false}],events:[{id:'activate'},{id:'elapsed'}],outputs:[{id:'request',type:'boolean',neutral:false},{id:'standalone',type:'number',min:0,max:10,neutral:0}],timers:[{id:'delay',event:'elapsed',delayMs:100,repeat:true},{id:'once',event:'activate',delayMs:200}],regions:[{id:'power',initial:'off',states:['off','on'],transitions:[{id:'enable',from:'off',to:'on',event:'activate'},{id:'stay',from:'on',event:'elapsed',guard:{ref:'intent'}},{id:'global',from:'*',to:'off',event:'activate'},{id:'any-stay',from:'*',event:'elapsed'}]},{id:'other',initial:'ready',states:['ready'],transitions:[]}]},
  inputs:[{id:'request',label:'Request',type:'boolean',signal:'intent'}],outputs:[{id:'response',label:'Response',type:'boolean',signal:'observed'}],visualDomains:[{signal:'intent',values:[false,true]},{signal:'input',min:0,max:10,samples:11}],
  hitRegions:[{id:'touch',x:0,y:0,width:100,height:100,events:{press:'activate',release:'elapsed'}}],
  scene:[{id:'face',label:'Face',kind:'rectangle',x:0,y:0,width:100,height:100,visible:{ref:'intent'}}],
  design:{nodes:[{id:'derived',label:'Derived',domain:{type:'number',min:0,max:10},expression:{op:'if',condition:{ref:'intent'},then:{ref:'input'},else:{literal:0}}},{id:'state-value',label:'State value',domain:{type:'boolean'},expression:{op:'eq',left:{state:'power'},right:{literal:'on'}}}],owners:[{region:'power',signal:'intent',port:'request',values:{off:false,on:true}}],appearances:[{id:'powered',regions:{power:'on'},layers:{face:{visible:true}}}],effects:[]}
};}
const find=(items,id)=>{const item=items.find(item=>item.id===id);assert.ok(item,`Missing ${id}`);return item;};

test('States cards expose initial/current state, Any, events, touch areas and timers',()=>{
  const source=fixture(),cards=graphCards(source,'States','power');
  assert.equal(find(cards,'off').kind,'Initial state');assert.equal(find(cards,'off').state,true);
  assert.equal(find(cards,'on').kind,'State');assert.equal(find(cards,'any:power').label,'Any state');
  assert.equal(find(cards,'hit:touch').kind,'Touch area');assert.equal(find(cards,'event:activate').event,true);
  assert.match(find(cards,'timer:delay').kind,/100 ms.*repeat/);assert.doesNotMatch(find(cards,'timer:once').kind,/repeat/);
  assert.equal(cards.some(c=>c.id==='ready'),false);assert.equal(new Set(cards.map(c=>c.id)).size,cards.length);
});

test('States wires preserve global, conditional and stay transition identity and event origins',()=>{
  const source=fixture(),cards=graphCards(source,'States','power'),wires=graphWires(source,'States','power',cards);
  assert.deepEqual([find(wires,'transition:enable').from,find(wires,'transition:enable').to],['off','on']);
  const stay=find(wires,'transition:stay');assert.equal(stay.from,'on');assert.equal(stay.to,'on');assert.match(stay.label,/condition.*Stay/);assert.equal(stay.transition,'stay');
  assert.equal(find(wires,'transition:global').from,'any:power');assert.equal(find(wires,'transition:global').to,'off');
  assert.equal(find(wires,'transition:any-stay').to,'any:power');
  assert.equal(find(wires,'trigger:global').to,'any:power');assert.equal(find(wires,'trigger:enable').from,'event:activate');
  assert.deepEqual([find(wires,'hit:touch:press').from,find(wires,'hit:touch:press').to],['hit:touch','event:activate']);
  assert.equal(find(wires,'hit:touch:release').label,'release');assert.equal(find(wires,'hit:touch:release').fromPin,'release');assert.equal(find(wires,'hit:touch:press').fromPin,'press');assert.equal(find(wires,'timer:delay').to,'event:elapsed');
  assert.ok(wires.every(w=>w.event));assert.ok(wires.every(w=>cards.some(c=>c.id===w.from)&&cards.some(c=>c.id===w.to)));
});

test('Signals cards include all declaration kinds, standalone outputs and presentation',()=>{
  const source=fixture(),cards=graphCards(source,'Signals','power');
  assert.match(find(cards,'intent').kind,/Local state/);assert.match(find(cards,'input').kind,/External input/);assert.match(find(cards,'observed').kind,/Feedback/);
  assert.equal(find(cards,'derived').label,'Derived');assert.equal(find(cards,'derived').expression,source.design.nodes[0].expression);
  assert.equal(find(cards,'output:request').label,'Request');assert.deepEqual(find(cards,'output:request').expression,{ref:'intent'});
  assert.match(find(cards,'output:standalone').kind,/Output binding/);assert.equal(cards.filter(c=>c.id==='output:request').length,1);
  assert.equal(find(cards,'feedback:response').label,'Response');assert.equal(find(cards,'domain:input').kind,'Rendering samples');
  assert.equal(find(cards,'state:other').label,'other');assert.equal(find(cards,'layer:face').label,'Face');
  assert.equal(cards.some(c=>c.id==='event:activate'),false);assert.equal(new Set(cards.map(c=>c.id)).size,cards.length);
});

test('Signals wires identify expression pins, region reads, owners, profile feedback and rendering',()=>{
  const source=fixture(),cards=graphCards(source,'Signals','power'),wires=graphWires(source,'Signals','power',cards);
  assert.ok(wires.some(w=>w.from==='intent'&&w.to==='derived'&&w.pin==='condition'));
  assert.ok(wires.some(w=>w.from==='input'&&w.to==='derived'&&w.pin==='then'));
  assert.ok(wires.some(w=>w.from==='state:power'&&w.to==='state-value'&&w.pin==='left'));
  assert.ok(wires.some(w=>w.from==='intent'&&w.to==='output:request'&&w.pin==='value'));
  assert.ok(wires.some(w=>w.from==='feedback:response'&&w.to==='observed'&&w.label==='feedback'));
  assert.ok(wires.some(w=>w.from==='input'&&w.to==='domain:input'&&w.label==='samples'));
  assert.ok(wires.some(w=>w.from==='state:power'&&w.to==='intent'&&w.label==='owns'));
  assert.ok(wires.some(w=>w.from==='intent'&&w.to==='layer:face'&&w.label==='appearance'));
  assert.ok(wires.some(w=>w.from==='state:power'&&w.to==='layer:face'&&w.label==='powered'));
  assert.ok(wires.every(w=>cards.some(c=>c.id===w.from)&&cards.some(c=>c.id===w.to)));
});

test('nested presentation groups never introduce dangling wire endpoints',()=>{
  const source=fixture();source.scene=[{id:'container',kind:'group',opacity:{ref:'input'},children:source.scene}];source.design.appearances[0].layers.container={opacity:.5};
  const cards=graphCards(source,'Signals','power'),wires=graphWires(source,'Signals','power',cards);
  assert.ok(cards.some(c=>c.id==='layer:face'));
  assert.ok(cards.some(c=>c.id==='layer:container'));
  assert.ok(wires.some(w=>w.from==='input'&&w.to==='layer:container'));
  assert.ok(wires.some(w=>w.from==='state:power'&&w.to==='layer:container'&&w.label==='powered'));
  assert.ok(wires.every(w=>cards.some(c=>c.id===w.from)&&cards.some(c=>c.id===w.to)),JSON.stringify(wires));
});

test('input pins and operands preserve variadic, unary, binary and leaf expressions',()=>{
  const literal={literal:true},ref={ref:'intent'},state={state:'power'};
  for(const expression of [literal,ref,state]){assert.deepEqual(inputPins(expression),['value']);assert.equal(pinExpression(expression,'value'),expression);}
  const expressions=[{op:'not',value:ref},{op:'eq',left:ref,right:literal},{op:'and',values:[ref,literal]},{op:'clamp',value:ref,min:{literal:0},max:{literal:1}}];
  assert.deepEqual(inputPins(expressions[0]),['value']);assert.deepEqual(inputPins(expressions[1]),['left','right']);assert.deepEqual(inputPins(expressions[2]),['values.0','values.1']);assert.deepEqual(inputPins(expressions[3]),['value','min','max']);
  for(const expression of expressions)for(const pin of inputPins(expression))assert.ok(pinExpression(expression,pin));
  assert.equal(pinExpression(expressions[2],'values.0'),ref);assert.equal(pinExpression(expressions[1],'right'),literal);
});

test('empty and missing regions are repairable without optional declarations',()=>{
  const source=fixture();delete source.design;delete source.behavior.timers;delete source.behavior.outputs;source.behavior.regions=[];source.behavior.events=[];source.hitRegions=[];source.inputs=[];source.outputs=[];source.scene[0].label=undefined;
  assert.deepEqual(graphWires(source,'States','missing',graphCards(source,'States','missing')),[]);
  assert.equal(graphCards(source,'States','missing').some(c=>c.id==='any:missing'),true);
  const cards=graphCards(source,'Signals','missing');assert.equal(find(cards,'layer:face').label,'face');
  assert.ok(graphWires(source,'Signals','missing',cards).some(w=>w.to==='layer:face'));
});

test('new graph cards clear partial saved layouts without moving saved states or mutating inputs',()=>{
  const cards=graphCards(fixture(),'States','power'),saved={off:{x:484,y:119},on:{x:80,y:48},stale:{x:40,y:40}},before=structuredClone({cards,saved});
  const positions=graphPositions(cards,saved);
  assert.deepEqual(positions.off,saved.off);assert.deepEqual(positions.on,saved.on);assert.equal(Object.hasOwn(positions,'stale'),false);
  assert.equal(positions['hit:touch'].x,40);assert.ok(positions['hit:touch'].y>=293);
  assert.ok(positions['event:activate'].y>=364);
  for(const card of cards)for(const other of cards){
    if(card===other||Object.hasOwn(saved,card.id)&&Object.hasOwn(saved,other.id))continue;
    const a=positions[card.id],b=positions[other.id],height=c=>c.state?225:c.id.startsWith('hit:')?170:110;
    assert.ok(a.x+230<=b.x||b.x+230<=a.x||a.y+height(card)+20<=b.y||b.y+height(other)+20<=a.y,`${card.id} overlaps ${other.id}`);
  }
  assert.deepEqual({cards,saved},before);positions.off.x=0;assert.equal(saved.off.x,484);
});

test('fresh default collisions use measured sizes, vertical clearance and touch/state fallbacks',()=>{
  const card=(id,x,y,extra={})=>({id,label:id,kind:'Event',x,y,...extra});
  const cards=[card('first',40,40),card('second',40,40),card('state',40,40,{state:true}),card('hit:touch',40,40),card('last',40,40),card('separate',1000,40)];
  const sizes={first:{width:400,height:180}},saved={},before=structuredClone({cards,sizes,saved});
  const positions=graphPositions(cards,saved,sizes);
  assert.deepEqual(positions.first,{x:40,y:40});assert.equal(positions.second.y,240);assert.equal(positions.state.y,370);assert.equal(positions['hit:touch'].y,615);assert.equal(positions.last.y,805);assert.deepEqual(positions.separate,{x:1000,y:40});
  assert.deepEqual({cards,sizes,saved},before);assert.deepEqual(graphPositions([], {stale:{x:0,y:0}}),{});
  const laterSaved=graphPositions([card('new',40,40),card('saved',40,40)],{saved:{x:40,y:40}},{saved:{width:210,height:300}});assert.equal(laterSaved.new.y,360);assert.deepEqual(laterSaved.saved,{x:40,y:40});
  assert.deepEqual(graphPositions([card('a',0,0),card('b',230,0)],{}).b,{x:230,y:0});
  assert.deepEqual(graphPositions([card('wide',40,40),card('beside',300,40)],{},{wide:{width:400,height:180}}).beside,{x:300,y:240});
});
