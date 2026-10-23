import type { Expression } from '../../../widget-runtime/src/behavior.ts';
import type { BehaviorDefinition } from '../../../widget-runtime/src/behavior.ts';
import { encodeWidgetBase64 } from '../../../widget-runtime/src/artifact.ts';
import type { WidgetSource } from '../types.ts';
const ref=(name:string):Expression=>({ref:name}),literal=(value:string|number|boolean):Expression=>({literal:value});
const choose=(condition:Expression,yes:string|number|boolean,no:string|number|boolean):Expression=>({op:'if',condition,then:literal(yes),else:literal(no)});
/** New generalization fixtures are deliberately outside the shipped seed catalog. */
function gaugeBehavior():BehaviorDefinition{
  const fresh:Expression={op:'eq',left:ref('quality'),right:literal('fresh')};
  return {version:1,signals:[{id:'rpm',source:'feedback',type:'number',initial:0,min:0,max:8000,freshnessMs:500,qualitySignal:'quality',requireGameConfirmed:true},{id:'quality',source:'input',type:'enum',initial:'unknown',values:['fresh','unknown','stale','disconnected']}],events:[{id:'feedback',updates:['rpm']},{id:'feedback-expired'}],timers:[{id:'freshness',event:'feedback-expired',delayMs:500}],regions:[
    {id:'alarm',initial:'normal',states:['normal','redline'],transitions:[
      {id:'redline-enter',from:'normal',event:'feedback',guard:{op:'and',values:[fresh,{op:'gte',left:ref('rpm'),right:literal(7000)}]},to:'redline',actions:[{type:'cue',cue:'redline'}]},
      {id:'redline-exit',from:'redline',event:'feedback',guard:{op:'or',values:[{op:'not',value:fresh},{op:'lte',left:ref('rpm'),right:literal(6800)}]},to:'normal'},
      {id:'expired',from:'*',event:'feedback-expired',to:'normal'},
    ]},
    {id:'sampling',initial:'ready',states:['ready'],transitions:[{id:'received',from:'ready',event:'feedback',actions:[{type:'start-timer',timer:'freshness'}]}]},
  ]};
}
function gaugeStatus():Expression{return {op:'if',condition:{op:'eq',left:ref('quality'),right:literal('fresh')},then:literal('RPM'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('stale')},then:literal('RPM stale'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('disconnected')},then:literal('RPM disconnected'),else:literal('RPM unknown')}}};}
function gaugeColor(normal:Expression|string):Expression{return {op:'if',condition:{op:'eq',left:{state:'alarm'},right:literal('redline')},then:literal('#f07373'),else:typeof normal==='string'?literal(normal):normal};}
export function createLinearGaugeFixture():WidgetSource{
  return {id:'fixture-linear-rpm',title:'Linear RPM fixture',size:{width:256,height:64},parameters:[{id:'accent',label:'Fill',type:'color',default:'#71e5c3'}],behavior:gaugeBehavior(),visualDomains:[{signal:'rpm',min:0,max:8000,samples:129,maxErrorPixels:1,travelPixels:240}],inputs:[],outputs:[{id:'rpm',label:'RPM',type:'number',signal:'rpm',min:0,max:8000}],hitRegions:[],scene:[{id:'track',kind:'rectangle',x:8,y:16,width:240,height:32,radius:4,fill:'#20313c'},{id:'fill',kind:'rectangle',x:8,y:16,width:{op:'mul',left:{op:'div',left:ref('rpm'),right:literal(8000)},right:literal(240)},height:32,fill:gaugeColor(ref('accent'))},{id:'quality-label',kind:'text',x:8,y:12,text:gaugeStatus(),font:'mono',fontSize:10,fill:'#d2edf0'}],motions:[],sounds:[]};
}
export function createRadialGaugeFixture():WidgetSource{
  return {id:'fixture-radial-rpm',title:'Radial RPM fixture',size:{width:192,height:192},parameters:[],behavior:gaugeBehavior(),visualDomains:[{signal:'rpm',min:0,max:8000,samples:97,maxErrorPixels:2,travelPixels:72*Math.PI*1.5}],inputs:[],outputs:[{id:'rpm',label:'RPM',type:'number',signal:'rpm',min:0,max:8000}],hitRegions:[],scene:[{id:'dial',kind:'ellipse',x:8,y:8,width:176,height:176,fill:'#182630',stroke:'#557075',strokeWidth:2},{id:'needle',kind:'line',x:96,y:96,x2:96,y2:24,stroke:gaugeColor('#71e5c3'),strokeWidth:3,transform:{originX:96,originY:96,rotate:{op:'add',left:literal(-135),right:{op:'mul',left:{op:'div',left:ref('rpm'),right:literal(8000)},right:literal(270)}}}},{id:'hub',kind:'ellipse',x:91,y:91,width:10,height:10,fill:'#d2edf0'},{id:'quality-label',kind:'text',x:96,y:181,text:gaugeStatus(),font:'mono',fontSize:10,align:'center',fill:'#d2edf0'}],motions:[],sounds:[]};
}
export function createThreeStateFixture():WidgetSource{
  const next:Expression={op:'if',condition:{op:'eq',left:ref('position'),right:literal('low')},then:literal('middle'),else:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('middle')},then:literal('high'),else:literal('low')}};
  return {id:'fixture-three-position',title:'Three-position fixture',size:{width:192,height:64},parameters:[],behavior:{version:1,signals:[{id:'position',source:'local',type:'enum',initial:'low',values:['low','middle','high']}],events:[{id:'activate'}],outputs:[{id:'select',type:'enum',neutral:'low',values:['low','middle','high']}],regions:[{id:'selection',initial:'ready',states:['ready'],transitions:[{id:'next',from:'ready',event:'activate',actions:[{type:'set',signal:'position',value:next},{type:'emit',port:'select',value:next}]}]}]},visualDomains:[],inputs:[{id:'select',label:'Position',type:'enum',signal:'position',values:['low','middle','high']}],outputs:[],hitRegions:[{id:'surface',x:0,y:0,width:192,height:64,events:{activate:'activate'}}],scene:[{id:'track',kind:'rectangle',x:2,y:2,width:188,height:60,radius:20,fill:'#20313c'},{id:'marker',kind:'ellipse',x:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('low')},then:literal(10),else:{op:'if',condition:{op:'eq',left:ref('position'),right:literal('middle')},then:literal(72),else:literal(134)}},y:12,width:40,height:40,fill:'#71e5c3'}],motions:[],sounds:[]};
}
/** Small deterministic PCM fixture; optional sound assets remain ordinary author data. */
export function createPcmSoundFixture():{id:string;mime:'audio/wav';base64:string}{
  const samples=800,bytes=new Uint8Array(44+samples*2),view=new DataView(bytes.buffer);const tag=(offset:number,text:string)=>{for(let i=0;i<text.length;i++)bytes[offset+i]=text.charCodeAt(i);};tag(0,'RIFF');view.setUint32(4,bytes.length-8,true);tag(8,'WAVE');tag(12,'fmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,16000,true);view.setUint32(28,32000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);tag(36,'data');view.setUint32(40,samples*2,true);for(let index=0;index<samples;index++)view.setInt16(44+index*2,Math.round(Math.sin(index*Math.PI/8)*2000*(1-index/samples)),true);return {id:'click-wave',mime:'audio/wav',base64:encodeWidgetBase64(bytes)};
}
