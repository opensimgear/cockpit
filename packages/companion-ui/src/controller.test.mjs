import assert from 'node:assert/strict';
import test from 'node:test';
import { axisPosition, axisValue, createPreviewController, fallbackValue } from './controller.ts';
import { feedbackPresentation, movedBounds, resizedBounds, safeAssetUrl } from './presentation.ts';
const base = { pageId:'page',label:'Widget',bounds:{x:0,y:0,width:.2,height:.2},feedbackIds:[] };
function doc(widgets, controls) {
  return {schemaVersion:2,ui:{schemaVersion:1,id:'test',title:'Test',controls,pages:[{id:'page',title:'Page',items:[]}],feedbackWidgets:[],assets:[]},presentation:{widgets:widgets.map(widget=>({...base,...widget})),groups:[],viewports:[{pageId:'page',width:1000,height:800}],grid:{columns:24,rows:16,snap:true}},targets:[],bindings:[]};
}
const button=(id='button')=>({id,label:id,kind:'button'});
const axis=(id='axis',fallback=.25,axisMode='bipolar')=>({id,label:id,kind:'axis',axisMode,fallback});
const bWidget=(buttonMode='momentary',id='button',controlIds=[id])=>({id,kind:'button',controlIds,config:{buttonMode,pulseMs:['pulse','repeat'].includes(buttonMode)?40:0,repeatMs:buttonMode==='repeat'?100:0}});
const xyWidget={id:'xy',kind:'xy',controlIds:['x','y'],config:{centering:'spring',deadzone:.1,invertX:false,invertY:true}};
function engine(document){let now=0;const changes=[];const preview=createPreviewController(document,{clock:()=>now,onchange:(value,reason)=>changes.push({value,reason})});return{preview,changes,time(value){now=value;preview.tick(now);}};}

test('axis normalization handles bipolar/unipolar clamping, inversion, deadzone and nonzero fallbacks',()=>{
  assert.equal(axisValue(.5,'bipolar',.1),0);
  assert.equal(axisValue(.1,'unipolar',.2),0);
  assert.equal(axisValue(.6,'unipolar',.2),.5);
  assert.equal(axisValue(.725,'bipolar',.1),.3889);
  assert.equal(axisValue(-10,'bipolar',0),-1);
  assert.equal(axisValue(10,'unipolar',0),1);
  assert.equal(axisValue(.25,'bipolar',0,true),.5);
  for(const values of [[NaN,0],[0,NaN],[0,-1],[0,1]])assert.throws(()=>axisValue(values[0],'bipolar',values[1]));
  for(const mode of ['unipolar','bipolar'])for(const inverted of [true,false])for(const value of mode==='unipolar'?[0,.3,1]:[-1,-.3,0,.3,1])assert.equal(axisValue(axisPosition(value,mode,.1,inverted),mode,.1,inverted),value);
  assert.equal(fallbackValue(button()),false);assert.equal(fallbackValue(axis()),.25);assert.equal(fallbackValue({kind:'hat'}),-1);
});
test('held button and XY own controls independently, cancellation affects only its pointer',()=>{
  const {preview:p}=engine(doc([bWidget(),xyWidget],[button(),axis('x',.3),axis('y',-.2)]));
  assert.equal(p.begin(1,'button'),false);p.arm();
  assert.equal(p.begin(1,'button'),true);assert.equal(p.begin(1,'xy',{x:1,y:0}),false);
  assert.equal(p.begin(2,'xy',{x:1,y:0}),true);assert.equal(p.snapshot().activePointers,2);
  assert.equal(p.move(2,{x:.5,y:.5}),true);
  assert.equal(p.release(2,'cancelled',true),true);
  assert.deepEqual(p.snapshot().values,{button:true,x:.3,y:-.2});
  p.release(1);assert.equal(p.snapshot().values.button,false);assert.equal(p.release(1),false);
});
test('invalid pointers, missing controls, incomplete axes and read-only widgets are rejected',()=>{
  const {preview:p}=engine(doc([bWidget(),{...xyWidget},{id:'missing',kind:'button',controlIds:['none'],config:bWidget().config},{id:'empty',kind:'text',controlIds:[],config:{prefix:'',suffix:''}},{id:'text',kind:'text',controlIds:['button'],config:{prefix:'',suffix:''}}],[button(),axis('x'),axis('y')]));p.arm();
  for(const [pointer,id,data]of[[1.1,'button'],[1,'absent'],[1,'missing'],[1,'empty'],[1,'text'],[1,'xy',{x:NaN,y:0}],[1,'xy',{x:1}]])assert.equal(p.begin(pointer,id,data),false);
  assert.equal(p.move(1,{x:1}),false);p.begin(1,'button');assert.equal(p.move(1,{x:1}),false);
  assert.equal(p.begin(2,'button'),false);p.disarm();assert.equal(p.snapshot().activePointers,0);
});
test('toggles and latched axes retain on normal release but clear on cancellation, Stop and document replacement',()=>{
  const latched={id:'slider',kind:'slider',controlIds:['a'],config:{centering:'latched',deadzone:0,inverted:false,orientation:'horizontal'}};
  const original=doc([bWidget('toggle'),latched],[button(),axis('a',.4,'unipolar')]);const {preview:p}=engine(original);p.arm();
  p.begin(1,'button');p.release(1);assert.equal(p.snapshot().values.button,true);
  p.begin(1,'button');p.release(1);assert.equal(p.snapshot().values.button,false);
  p.begin(2,'slider',{x:.8});p.release(2);assert.equal(p.snapshot().values.a,.8);
  p.begin(2,'slider',{x:.7});p.release(2,'keyboard blur',true);assert.equal(p.snapshot().values.a,.4);
  p.begin(1,'button');p.release(1);p.begin(2,'slider',{x:.9});p.release(2);p.disarm('Page changed');
  assert.deepEqual(p.snapshot().values,{button:false,a:.4});assert.equal(p.snapshot().armed,false);
  p.arm();p.begin(2,'slider',{x:1});p.setDocument(doc([bWidget('momentary','next')],[button('next')]));
  assert.equal(p.snapshot().armed,false);assert.deepEqual(p.snapshot().values,{next:false});assert.equal(p.begin(3,'next'),false);
});
test('vertical spring sliders use y, inversion and declared fallback on release',()=>{
  const slider={id:'slider',kind:'slider',controlIds:['a'],config:{centering:'spring',deadzone:0,inverted:true,orientation:'vertical'}};const{preview:p}=engine(doc([slider],[axis('a',.3,'unipolar')]));p.arm();
  assert.equal(p.begin(1,'slider',{y:.2}),true);assert.equal(p.snapshot().values.a,.8);p.release(1);assert.equal(p.snapshot().values.a,.3);
  assert.equal(p.begin(1,'slider',{x:1}),false);
});
test('selector validates options, preserves selection and returns to declared option on Stop',()=>{
  const selector={id:'selector',kind:'selector',controlIds:['a'],config:{options:[{label:'Low',value:0},{label:'High',value:1}]}};const{preview:p}=engine(doc([selector],[axis('a',0,'unipolar')]));p.arm();
  assert.equal(p.begin(1,'selector',{value:.5}),false);p.begin(1,'selector',{value:1});p.release(1);assert.equal(p.snapshot().values.a,1);p.disarm();assert.equal(p.snapshot().values.a,0);
});
test('all hat directions are discrete and release to neutral',()=>{
  const hat={id:'hat',kind:'hat',controlIds:['hat'],config:{}};const{preview:p}=engine(doc([hat],[{id:'hat',label:'Hat',kind:'hat',fallback:-1}]));p.arm();
  for(const value of [undefined,-1,8,.5])assert.equal(p.begin(1,'hat',{value}),false);
  for(let value=0;value<8;value++){assert.equal(p.begin(1,'hat',{value}),true);assert.equal(p.snapshot().values.hat,value);p.release(1);assert.equal(p.snapshot().values.hat,-1);}
});
test('pulse transitions remain ordered after pointer release and finish at deadline',()=>{
  const{preview:p,time}=engine(doc([bWidget('pulse')],[button()]));p.arm();p.begin(1,'button');p.release(1);assert.equal(p.snapshot().values.button,true);
  time(39);assert.equal(p.snapshot().values.button,true);assert.equal(p.begin(2,'button'),false);assert.equal(p.snapshot().reason,'Pulse pending; activation rejected');time(40);assert.equal(p.snapshot().values.button,false);
  assert.deepEqual(p.snapshot().events.map(e=>e.changes[0].value),[true,false]);assert.deepEqual(p.snapshot().events.map(e=>e.atMs),[0,40]);
  p.begin(2,'button');p.release(2,'cancelled',true);assert.equal(p.snapshot().values.button,false);time(80);assert.equal(p.snapshot().values.button,false);p.begin(3,'button');p.release(3);p.cancelWidget('button');assert.equal(p.snapshot().values.button,false);time(120);assert.equal(p.snapshot().values.button,false);p.begin(4,'button');p.cancelWidget('button');assert.equal(p.snapshot().activePointers,0);
});
test('repeat emits each press/release, cancels on release and stops after clock gap',()=>{
  const{preview:p,time}=engine(doc([bWidget('repeat')],[button()]));p.arm();p.begin(1,'button');time(240);
  assert.deepEqual(p.snapshot().events.map(e=>e.changes[0].value),[true,false,true,false,true,false]);
  p.release(1);const count=p.snapshot().events.length;time(400);assert.equal(p.snapshot().events.length,count);
  p.begin(1,'button');time(1501);assert.equal(p.snapshot().armed,false);assert.equal(p.snapshot().values.button,false);assert.equal(p.snapshot().reason,'Preview timing gap');
  time(NaN);time(-1);assert.equal(p.snapshot().armed,false);
});
test('encoder maps signed increments to distinct bounded cw/ccw pulses and optional repeat',()=>{
  const encoder={id:'encoder',kind:'encoder',controlIds:['cw','ccw'],config:{pulseMs:20,repeatMs:0}};const{preview:p,time}=engine(doc([encoder],[button('cw'),button('ccw')]));p.arm();
  p.begin(1,'encoder',{direction:-1});p.release(1);assert.deepEqual(p.snapshot().values,{cw:false,ccw:true});time(20);assert.equal(p.snapshot().values.ccw,false);
  p.begin(2,'encoder',{direction:1});assert.equal(p.snapshot().values.cw,true);p.release(2,'blur',true);assert.equal(p.snapshot().values.cw,false);
});
test('subscriptions and snapshots detach state, bounded recorder preserves final edges and rearm clears held controls',()=>{
  const{preview:p}=engine(doc([bWidget()],[button()]));const seen=[];const unsubscribe=p.subscribe(snapshot=>seen.push(snapshot));p.arm();
  for(let i=0;i<40;i++){p.begin(i,'button');p.release(i);}assert.equal(p.snapshot().events.length,64);assert.equal(p.snapshot().events.at(-1).changes[0].value,false);
  const detached=p.snapshot();detached.values.button=true;detached.events[0].changes[0].value='bad';assert.equal(p.snapshot().values.button,false);assert.notEqual(p.snapshot().events[0].changes[0].value,'bad');
  p.begin(99,'button');p.arm();assert.equal(p.snapshot().values.button,false);assert.equal(p.snapshot().activePointers,0);const count=seen.length;unsubscribe();p.disarm();assert.equal(seen.length,count);
});
test('asset URLs permit bounded raster data only',()=>{
  for(const mime of ['png','jpeg','webp'])assert.equal(safeAssetUrl(`data:image/${mime};base64,YQ==`),`data:image/${mime};base64,YQ==`);
  for(const value of [null,'https://x/y.png','file:///a.png','data:image/svg+xml;base64,YQ==','data:image/png;base64, x','x'.repeat(3*1024*1024+1)])assert.equal(safeAssetUrl(value),null);
});
test('typed feedback presentation exposes source/provenance/freshness and never manufactures confirmed feedback',()=>{
  const ui={feedbackWidgets:[{id:'text',channelId:'message',kind:'text'},{id:'number',channelId:'rpm',kind:'gauge'},{id:'flag',channelId:'flag',kind:'indicator'}]};
  const sample=(value,extra={})=>({value,sourceAgeMs:0,receivedAtMs:0,provenance:'host-submitted',sourceKind:'host',unit:'',...extra});
  const text={kind:'text',feedbackIds:['text'],config:{prefix:'[',suffix:']'}};
  assert.equal(feedbackPresentation(text,ui,{},0).text,'Unavailable');assert.equal(feedbackPresentation(text,ui,{message:sample(1)},0).available,false);
  assert.equal(feedbackPresentation(text,ui,{message:sample('hello')},0).text,'[hello]');assert.equal(feedbackPresentation(text,ui,{message:sample('hello')},300).freshness,'aging');assert.equal(feedbackPresentation(text,ui,{message:sample('hello')},1001).freshness,'stale');
  assert.equal(feedbackPresentation({...text,feedbackIds:['missing']},ui,{},0).provenance,'No source');
  const number={kind:'numeric',feedbackIds:['number'],config:{precision:1,unit:'rpm'}};
  assert.equal(feedbackPresentation(number,ui,{rpm:sample(123.45)},0).text,'123.5 rpm');assert.equal(feedbackPresentation({...number,kind:'gauge',config:{precision:0,unit:''}},ui,{rpm:sample(3)},0).text,'3');
  assert.equal(feedbackPresentation(number,ui,{rpm:sample(NaN)},0).available,false);assert.equal(feedbackPresentation(number,ui,{rpm:sample(3,{sourceAgeMs:-1})},0).available,false);
  const flag={kind:'indicator',feedbackIds:['flag'],config:{}};assert.equal(feedbackPresentation(flag,ui,{flag:sample(true)},0).text,'Active');assert.equal(feedbackPresentation(flag,ui,{flag:sample(false)},0).text,'Inactive');
  assert.equal(feedbackPresentation(flag,ui,{flag:sample('true')},0).available,false);assert.equal(feedbackPresentation(flag,ui,{flag:sample(true)},0).provenance,'host-submitted · host');
});


test('edit gesture layout quantizes only when requested and clamps to the safe content region',()=>{
  const bounds={x:.1,y:.1,width:.2,height:.3};
  assert.deepEqual(movedBounds(bounds,.031,.02,{columns:10,rows:10,snap:true}),{x:.1,y:.1,width:.2,height:.3});
  assert.deepEqual(movedBounds(bounds,.061,.07,{columns:10,rows:10,snap:true}),{x:.2,y:.2,width:.2,height:.3});
  assert.deepEqual(movedBounds(bounds,.031,.02,{columns:10,rows:10,snap:false}),{x:.131,y:.12000000000000001,width:.2,height:.3});
  assert.deepEqual(movedBounds(bounds,2,-2,{columns:10,rows:10,snap:true}),{x:.8,y:0,width:.2,height:.3});
  assert.deepEqual(bounds,{x:.1,y:.1,width:.2,height:.3});
});

test('resize gestures honor snap, minimum grid size, content boundaries and preserve the original',()=>{
  const bounds={x:.2,y:.2,width:.3,height:.3};
  assert.deepEqual(resizedBounds(bounds,.06,.06,{columns:10,rows:10,snap:true}),{x:.2,y:.2,width:.4,height:.4});
  assert.deepEqual(resizedBounds(bounds,.06,.06,{columns:10,rows:10,snap:false}),{x:.2,y:.2,width:.36,height:.36});
  assert.deepEqual(resizedBounds(bounds,-2,2,{columns:10,rows:10,snap:true}),{x:.2,y:.2,width:.1,height:.8});
  assert.deepEqual(bounds,{x:.2,y:.2,width:.3,height:.3});
  const edge=resizedBounds({x:.99,y:.99,width:.01,height:.01},2,2,{columns:10,rows:10,snap:true});assert.ok(edge.x+edge.width<=1);assert.ok(edge.y+edge.height<=1);
});
test('repeat configurations with no off interval are rejected rather than emitting collapsed edges',()=>{
  const bad=bWidget('repeat');bad.config.repeatMs=bad.config.pulseMs;const{preview:p}=engine(doc([bad],[button()]));p.arm();assert.equal(p.begin(1,'button'),false);assert.equal(p.snapshot().events.length,0);
});
