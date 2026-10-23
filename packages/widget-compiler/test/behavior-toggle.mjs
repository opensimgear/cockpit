const ref=name=>({ref:name}),literal=value=>({literal:value});
const choose=(condition,yes,no)=>({op:'if',condition,then:literal(yes),else:literal(no)});
/** The sole shipped seed. Its familiar shape has no runtime-specific behavior. */
export function behaviorToggleSource(){
  const toggle={op:'not',value:ref('requested')};
  return {
    id:'toggle-v2',title:'Toggle',size:{width:240,height:144},
    parameters:[{id:'label',label:'Title',type:'text',default:'TOGGLE'},{id:'accent',label:'Thumb',type:'color',default:'#bde7ec'},{id:'background',label:'Track',type:'color',default:'#252d30'},{id:'foreground',label:'Text',type:'color',default:'#d2edf0'}],
    behavior:{version:1,signals:[
      {id:'requested',source:'local',type:'boolean',initial:false},
      {id:'confirmed',source:'feedback',type:'boolean',initial:false,freshnessMs:1000,qualitySignal:'quality',requireGameConfirmed:true},
      {id:'quality',source:'input',type:'enum',initial:'unknown',values:['fresh','unknown','stale','disconnected']},
      {id:'pressed',source:'local',type:'boolean',initial:false},
      {id:'available',source:'input',type:'boolean',initial:true},
    ],events:[{id:'activate'},{id:'press'},{id:'release'},{id:'cancel'},{id:'feedback',updates:['confirmed']},{id:'availability',updates:['available']}],outputs:[{id:'toggle',type:'boolean',neutral:false}],regions:[{id:'interaction',initial:'ready',states:['ready'],transitions:[
      {id:'request',from:'ready',event:'activate',guard:ref('available'),actions:[{type:'set',signal:'requested',value:toggle},{type:'emit',port:'toggle',value:toggle},{type:'cue',cue:'click'}]},
      {id:'press',from:'ready',event:'press',guard:ref('available'),actions:[{type:'set',signal:'pressed',value:literal(true)}]},
      {id:'release',from:'ready',event:'release',actions:[{type:'set',signal:'pressed',value:literal(false)}]},
      {id:'cancel',from:'ready',event:'cancel',actions:[{type:'set',signal:'pressed',value:literal(false)}]},
    ]}]},
    visualDomains:[],inputs:[{id:'toggle',label:'Toggle',type:'boolean',signal:'requested'}],outputs:[{id:'confirmation',label:'Game confirmation',type:'boolean',signal:'confirmed'}],
    hitRegions:[{id:'surface',x:0,y:0,width:240,height:144,events:{press:'press',release:'release',activate:'activate',cancel:'cancel'}}],
    scene:[
      {id:'title',kind:'text',x:120,y:27,text:ref('label'),font:'sans',fontSize:28,align:'center',fill:ref('foreground')},
      {id:'body',kind:'group',opacity:choose(ref('available'),1,.4),children:[
        {id:'track-halo',kind:'rectangle',x:23,y:47,width:194,height:66,radius:33,fill:'none',stroke:ref('accent'),strokeWidth:6,opacity:.08},
        {id:'track',kind:'rectangle',x:24,y:48,width:192,height:64,radius:32,fill:ref('background'),stroke:ref('accent'),strokeWidth:choose(ref('pressed'),3,2)},
        {id:'thumb-halo',kind:'ellipse',x:choose(ref('requested'),151,25),y:49,width:62,height:62,fill:ref('accent'),opacity:.12},
        {id:'thumb',kind:'ellipse',x:choose(ref('requested'),157,31),y:55,width:50,height:50,fill:ref('accent')},
        {id:'thumb-highlight',kind:'ellipse',x:choose(ref('requested'),164,38),y:62,width:36,height:36,fill:'#ffffff',opacity:.12},
      ]},
      {id:'confirmation',kind:'text',x:120,y:135,text:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('fresh')},then:choose(ref('confirmed'),'Game: ON','Game: OFF'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('stale')},then:literal('Game: stale'),else:{op:'if',condition:{op:'eq',left:ref('quality'),right:literal('disconnected')},then:literal('Game: disconnected'),else:literal('Game: unknown')}}},font:'mono',fontSize:11,align:'center',fill:ref('foreground')},
    ],
    motions:[
      {id:'on',from:{requested:false},to:{requested:true},durationMs:160,fps:30,interruption:'reverse',tracks:[{node:'thumb',property:'x',keyframes:[{at:0,value:31},{at:1,value:157}],easing:'ease-in-out'},{node:'thumb-halo',property:'x',keyframes:[{at:0,value:25},{at:1,value:151}],easing:'ease-in-out'},{node:'thumb-highlight',property:'x',keyframes:[{at:0,value:38},{at:1,value:164}],easing:'ease-in-out'}]},
      {id:'off',from:{requested:true},to:{requested:false},durationMs:160,fps:30,interruption:'reverse',tracks:[{node:'thumb',property:'x',keyframes:[{at:0,value:157},{at:1,value:31}],easing:'ease-in-out'},{node:'thumb-halo',property:'x',keyframes:[{at:0,value:151},{at:1,value:25}],easing:'ease-in-out'},{node:'thumb-highlight',property:'x',keyframes:[{at:0,value:164},{at:1,value:38}],easing:'ease-in-out'}]},
    ],sounds:[],assets:[],
  };
}
