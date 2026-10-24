// Minimal compiled widgets for the DCS demo. No motion, sound or external assets.
const ref = name => ({ref:name}), literal = value => ({literal:value});
const choose = (condition, yes, no) => ({op:'if',condition,then:literal(yes),else:literal(no)});
const fresh = {op:'eq',left:ref('quality'),right:literal('fresh')};
const text = (id,x,y,value,size=16) => ({id,kind:'text',x,y,text:value,font:'sans',fontSize:size,fill:'#d2edf0'});
const surface = (width,height) => ({id:'surface',kind:'rectangle',x:0,y:0,width,height,radius:8,fill:'#20313c'});
const quality = {id:'quality',source:'input',type:'enum',initial:'unknown',values:['fresh','unknown','stale','disconnected']};
const base = (id,title,width,height) => ({id,title,size:{width,height},parameters:[],behavior:{version:1,signals:[],events:[],regions:[]},visualDomains:[],inputs:[],outputs:[],hitRegions:[],scene:[surface(width,height)]});
const button=base('dcs-demo-button','DCS momentary button',180,88);
button.parameters=[{id:'label',label:'Label',type:'text',default:'BUTTON'}];
button.behavior={version:1,signals:[{id:'pressed',source:'local',type:'boolean',initial:false}],events:[{id:'press'},{id:'release'},{id:'cancel'}],outputs:[{id:'press',type:'boolean',neutral:false}],regions:[{id:'button',initial:'ready',states:['ready'],transitions:['press','release','cancel'].map(event=>({id:event,from:'ready',event,actions:[{type:'set',signal:'pressed',value:literal(event==='press')},{type:'emit',port:'press',value:literal(event==='press')}]}))}]};
button.inputs=[{id:'press',label:'Pressed',type:'boolean',signal:'pressed'}];
button.hitRegions=[{id:'button',x:0,y:0,width:180,height:88,events:{press:'press',release:'release',cancel:'cancel'}}];
button.scene.push({id:'active',kind:'rectangle',x:0,y:0,width:180,height:88,radius:8,fill:choose(ref('pressed'),'#387464','#20313c')},text('label',14,35,ref('label'),18),text('state',14,65,choose(ref('pressed'),'PRESSED','HOLD'),12));
function gauge(id,title,max,unit) {
  const source=base(id,title,336,22);
  source.behavior.signals=[{id:'value',source:'feedback',type:'number',initial:0,min:0,max,freshnessMs:1500,qualitySignal:'quality'},quality];
  source.behavior.events=[{id:'feedback',updates:['value']}];
  source.visualDomains=[{signal:'value',min:0,max,samples:81,maxErrorPixels:7,travelPixels:336}];
  source.outputs=[{id:'value',label:title,type:'number',signal:'value',min:0,max}];
  source.scene=[{id:'track',kind:'rectangle',x:0,y:0,width:336,height:22,radius:3,fill:'#131f25'},
    {id:'value',kind:'rectangle',x:0,y:0,width:{op:'mul',left:{op:'div',left:ref('value'),right:literal(max)},right:literal(336)},height:22,fill:'#71e5c3',visible:fresh},
    {...text('quality',8,16,'UNAVAILABLE',11),visible:{op:'not',value:fresh}}];
  return source;
}
// Static labels stay outside the sampled bar so a larger page does not multiply
// the same full-card pixels across every numeric/quality presentation.
const frame=base('dcs-demo-gauge-frame','Gauge frame',368,120);
frame.parameters=[{id:'label',label:'Label',type:'text',default:'GAUGE'},{id:'range',label:'Range',type:'text',default:'0 — 100 %'}];
frame.scene.push(text('label',16,28,ref('label')),text('range',16,106,ref('range'),12));
const warning=base('dcs-demo-altitude-warning','Altitude warning',368,120);
warning.behavior.signals=[{id:'active',source:'feedback',type:'boolean',initial:false,freshnessMs:1500,qualitySignal:'quality'},quality];
warning.behavior.events=[{id:'feedback',updates:['active']}];
warning.outputs=[{id:'active',label:'Above 1000 m',type:'boolean',signal:'active'}];
warning.scene.push(text('label',16,28,'ALTITUDE > 1000 m'),
  {id:'lamp',kind:'ellipse',x:16,y:51,width:26,height:26,fill:{op:'if',condition:fresh,then:choose(ref('active'),'#f0b968','#477365'),else:literal('#65717b')}},
  text('state',58,71,{op:'if',condition:fresh,then:choose(ref('active'),'ABOVE','BELOW'),else:literal('UNAVAILABLE')},20));
export const sources=[button,gauge('dcs-demo-altitude','Altitude',20000,'m'),gauge('dcs-demo-percent','Percent',100,'%'),warning,frame];
const place=(id,label,sourceId,x,y,width,height,parameters={})=>({id,pageId:'main',label,sourceId,parameters,bounds:{x:x/800,y:y/480,width:width/800,height:height/480}});
const instrument=(id,label,sourceId,x,y,range)=>[place(`${id}-frame`,label,frame.id,x,y,368,120,{label,range}),place(id,label,sourceId,x+16,y+48,336,22)];
export const draft={id:'dcs-demo-win',title:'DCS · WIN Companion',pages:[{id:'main',title:'DCS',width:2400,height:1440,background:'#10191f'}],widgets:[
  ...['Virtual A','Virtual B','Toggle test','DCS lighting'].map((label,index)=>place(`button-${index+1}`,label,button.id,16+index*196,16,180,88,{label})),
  ...instrument('altitude','ALTITUDE · m','dcs-demo-altitude',16,120,'0 — 20000 m'),
  place('warning','Altitude warning',warning.id,416,120,368,120),
  ...instrument('rudder','RUDDER · %','dcs-demo-percent',16,256,'0 — 100 %'),
  ...instrument('brightness','PANEL BRIGHTNESS · %','dcs-demo-percent',416,256,'0 — 100 %'),
]};
