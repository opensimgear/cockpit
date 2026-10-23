import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {createToggleSource,createLinearGaugeFixture,createRadialGaugeFixture,createThreeStateFixture,createPcmSoundFixture,validateWidgetSource} from '../src/index.ts';
import {tinySource,rasterPng} from '../test/fixtures.mjs';

const cases=[];
function add(name,source){const checked=validateWidgetSource(source);cases.push({name,valid:checked.ok,source});}
for(const source of [createToggleSource(),createLinearGaugeFixture(),createRadialGaugeFixture(),createThreeStateFixture()])add(source.id,source);
const complete=tinySource();complete.parameters=[{id:'label',label:'Label',type:'text',default:'SAMPLE'},{id:'visible',label:'Visible',type:'boolean',default:true},{id:'scale',label:'Scale',type:'number',default:1,min:0,max:2},{id:'color',label:'Color',type:'color',default:'#ffffff80'}];
complete.assets=[{id:'pixel',mime:'image/png',base64:Buffer.from(rasterPng()).toString('base64')},createPcmSoundFixture()];
complete.sounds=[{id:'click',cue:'click',assetId:'click-wave',cooldownMs:20,stopOnExit:true,volume:.5}];
complete.behavior.regions=[{id:'mode',initial:'idle',states:['idle','on'],transitions:[]}];
complete.scene=[{id:'group',label:'Root',kind:'group',opacity:{op:'clamp',value:{ref:'scale'},min:{literal:0},max:{literal:1}},transform:{translateX:0,translateY:0,rotate:0,scaleX:1,scaleY:1,originX:16,originY:16},children:[
  {id:'text',kind:'text',x:0,y:12,text:{ref:'label'},font:'sans',fontSize:10,align:'left',fill:{ref:'color'},visible:{op:'and',values:[{ref:'visible'},{op:'not',value:{op:'eq',left:{state:'mode'},right:{literal:'on'}}}]}},
  {id:'path',kind:'path',d:'M0 0 L2 0 L2 2 Z',fill:'#ffffff'},
  {id:'image',kind:'image',x:16,y:16,width:8,height:8,assetId:'pixel'},
  {id:'line',kind:'line',x:0,y:16,x2:{op:'add',left:{literal:2},right:{literal:1}},y2:16,stroke:'#ffffff',strokeWidth:1},
  {id:'ellipse',kind:'ellipse',x:0,y:20,width:8,height:8,fill:'#ff0000'},
]}];
complete.motions=[{id:'animate',from:{},to:{},fromRegions:{mode:'idle'},toRegions:{mode:'on'},durationMs:100,fps:30,interruption:'snap',tracks:[{node:'group',property:'transform.rotate',easing:'linear',keyframes:[{at:0,value:0},{at:1,value:15}]}]}];
add('all-features',complete);
const numeric=createLinearGaugeFixture();numeric.visualDomains=[{signal:'rpm',values:Array.from({length:129},(_,i)=>i*62.5),maxErrorPixels:1,travelPixels:240,smoothingMs:120}];add('numeric-enumerated',numeric);
const finite=createToggleSource();finite.visualDomains=[{signal:'requested',values:[true,false]},{signal:'quality',values:['fresh','unknown','stale','disconnected']}];add('finite-discrete',finite);
const nested=tinySource();nested.behavior.signals=[{id:'enabled',source:'local',type:'boolean',initial:true}];let expression={ref:'enabled'};for(let i=0;i<7;i++)expression={op:'not',value:expression};nested.scene[0].visible=expression;for(let i=0;i<8;i++)nested.scene=[{id:'group'+i,kind:'group',children:nested.scene}];add('maximal-group-expression-depth',nested);
const maximal=tinySource();maximal.scene[0].visible={op:'and',values:Array.from({length:16},(_,i)=>i===15?{literal:true}:{op:'not',value:{literal:false}})};add('maximal-expression-work',maximal);
const partialNumeric=createLinearGaugeFixture();partialNumeric.outputs[0].min=1;add('numeric-port-subrange',partialNumeric);
const invalids=[
  ['unknown-field',s=>s.extra=true],['legacy-format',s=>s.formatVersion=1],['bad-id',s=>s.id='bad:id'],['bad-size',s=>s.size.width=0],['bad-revision',s=>s.revision=0],['duplicate-param',s=>s.parameters.push(s.parameters[0])],['param-type',s=>s.parameters[0].type='script'],['param-color',s=>s.parameters[1].default='red'],['signal-reference',s=>s.scene[0].text={ref:'missing'}],['region-reference',s=>s.scene[0].text={state:'missing'}],['bad-expression',s=>s.scene[0].opacity={op:'script'}],['bad-literal',s=>s.scene[0].opacity={literal:null}],['empty-operands',s=>s.scene[0].visible={op:'and',values:[]}],['bad-font',s=>s.scene[0].font='system'],['unknown-primitive',s=>s.scene[0].kind='html'],['duplicate-node',s=>s.scene.push(s.scene[0])],['outside-hit',s=>s.hitRegions[0].width=9999],['missing-event',s=>s.hitRegions[0].events.activate='missing'],['missing-emit',s=>s.inputs[0].id='missing'],['input-signal-type',s=>s.inputs[0].type='number'],['missing-output-signal',s=>s.outputs[0].signal='missing'],['unexpected-port-domain',s=>s.inputs[0].min=0],['expression-depth-overflow',s=>s.scene[0].visible={op:'not',value:expression}],['expression-work-overflow',s=>s.scene[0].visible={op:'and',values:Array.from({length:16},()=>({op:'not',value:{literal:false}}))}],['primitive-expression-child',s=>s.scene[0].visible={op:'not',value:true}],['oversized-expression-literal',s=>s.scene[0].text={literal:'x'.repeat(121)}],['partial-discrete-domain',s=>s.visualDomains=[{signal:'requested',values:[false]}]],['bad-track',s=>s.motions[0].tracks[0].property='draw'],['bad-node',s=>s.motions[0].tracks[0].node='missing'],['bad-keyframe-time',s=>s.motions[0].tracks[0].keyframes[0].at=.5],['bad-motion-range',s=>s.motions[0].from={requested:2}],['empty-selector',s=>s.motions[0].from={}],['bad-media',s=>s.assets=[{id:'bad',mime:'audio/wav',base64:'eA=='}]],
];
for(const [name,change] of invalids){const source=createToggleSource();change(source);add(name,source);if(cases.at(-1).valid)throw new Error('Expected malformed fixture: '+name);}
const output=JSON.stringify({formatVersion:1,cases},null,2)+'\n',path=new URL('../test/fixtures/native-sources.json',import.meta.url);
if(process.argv.includes('--check')){if(readFileSync(path,'utf8')!==output)throw new Error('Native source fixtures are stale');}else{mkdirSync(new URL('../test/fixtures/',import.meta.url),{recursive:true});writeFileSync(path,output);}
