import type { DesignerDocument } from "../../companion-designer/src/types.ts";
import { prepareA4, prepareRgba, rgb565 } from "./assets.ts";
import type { PreparedFont, PreparedImage } from "./assets.ts";
import { raw, scalar, variable } from "./compiler.ts";
import type { Expression, NodeRecipe, ResolvedRecipe } from "./compiler.ts";
import type { DeviceProfile, Rect } from "./types.ts";

const C={background:rgb565(7,16,22),cyan:rgb565(104,224,232),white:rgb565(213,242,242),muted:rgb565(91,131,143),red:rgb565(255,99,119),amber:rgb565(255,194,87),green:rgb565(112,235,173)};
const operation=(op:"less"|"select"|"not",...args:Expression[]):Expression=>({kind:"op",op,args});
function art(id:string,width:number,height:number,kind:"background"|"button"|"warning"|"card"):PreparedImage{
  const rgba=new Uint8Array(width*height*4);
  for(let y=0;y<height;++y)for(let x=0;x<width;++x){
    const i=(y*width+x)*4,n=((x*17+y*13+(x*y)%19)%11-5)*0.18,edge=Math.min(x,y,width-1-x,height-1-y);
    let r=9+n,g=22+n,b=29+n;
    if(kind==="background"){const glow=Math.max(0,1-Math.hypot(x-width*.75,y-height*.12)/(width*.9))*6;r+=glow*.2;g+=glow;b+=glow*1.4;if(y%32===0||x%64===0){g+=2;b+=2;}}
    else{
      const shine=(1-y/height)*7;r+=shine;g+=shine;b+=shine;
      if(edge<1){r=38;g=91;b=104;}else if(edge<2){r=18;g=47;b=60;}
      if(kind==="button"||kind==="warning"){
        if(edge>=4&&edge<5){r=kind==="warning"?167:66;g=kind==="warning"?64:167;b=kind==="warning"?80:181;}
        if(kind==="warning"&&edge>=2&&edge<7&&(x+y)%10<3){r=138;g=46;b=63;}
        const dx=x-width/2,dy=y-height/2,d=Math.hypot(dx,dy);
        if(kind==="button"&&width<=height*1.2&&(Math.abs(d-width*.22)<.75||Math.abs(dx)<.7&&Math.abs(dy)<width*.33||Math.abs(dy)<.7&&Math.abs(dx)<width*.33)){r=143;g=231;b=238;}
        if(kind==="warning"&&(Math.abs(dx)<1.2&&dy>-9&&dy<4||Math.abs(dx)<1.2&&dy>7&&dy<10)){r=255;g=175;b=181;}
      }
    }
    rgba[i]=Math.max(0,Math.min(255,r));rgba[i+1]=Math.max(0,Math.min(255,g));rgba[i+2]=Math.max(0,Math.min(255,b));rgba[i+3]=255;
  }
  return prepareRgba(id,width,height,rgba).image;
}
function coverage(id:string,width:number,height:number,kind:"press"|"ring"|"stack"):PreparedImage{
  const alpha=new Uint8Array(width*height);
  for(let y=0;y<height;++y)for(let x=0;x<width;++x){
    let a=0;
    if(kind==="press"){const edge=Math.min(x,y,width-1-x,height-1-y);a=edge<2?220:edge<4?130:42;}
    else if(kind==="stack"){if(y%9<3&&x>5&&x<width-6)a=210;}
    else{
      const d=Math.hypot(x-(width-1)/2,y-(height-1)/2),radius=width*.43;
      a=Math.max(0,1-Math.abs(d-radius)/1.6)*230+Math.max(0,1-Math.abs(d-radius+4)/.7)*70;
      const angle=Math.atan2(y-height/2,x-width/2);if(Math.abs(d-radius)<8&&Math.abs(Math.sin(angle*12))<.08)a=190;
    }
    alpha[y*width+x]=Math.min(255,Math.round(a));
  }
  return prepareA4(id,width,height,alpha);
}
/** Original compact two-button panel and a roomier instrument panel; target layouts are independent. */
export function createStarter(profile:DeviceProfile,revision:"a"|"b",fontCatalog:PreparedFont[]):{document:DesignerDocument;recipe:ResolvedRecipe}{
  const large=profile.width===800,board=large?"waveshare":"sunton",pageId="systems",scale=large?1:0;
  const buttonBounds:Rect[]=large?[[32,144,184,48],[32,212,184,48]]:[[16,96,44,44],[80,96,44,44]];
  const numberBounds:Rect=large?[384,184,128,48]:[160,88,64,32],needleBounds:Rect=large?[292,308,3,28]:[160,152,3,16];
  const indicatorBounds:Rect=large?[600,128,16,16]:[288,96,8,8];
  const semanticBounds=[...buttonBounds,numberBounds,large?[280,294,260,56] as Rect:[152,144,96,40] as Rect,indicatorBounds];
  const widgetIds=["laser-button","stop-button","power-number","power-gauge","laser-indicator"],controlIds=["laser-input","stop-input"],outputIds=["power-readout","power-needle","laser-state"];
  const controls=controlIds.map((id,i)=>({id,kind:"button" as const,label:i?"Emergency stop":"Enable laser"}));
  const feedbackWidgets=outputIds.map((id,i)=>({id,label:i===2?"Laser state":"Reactor power",channelId:i===2?"laser-confirmed":"reactor-power",kind:i===2?"indicator" as const:"gauge" as const}));
  const widgets=widgetIds.map((id,i):DesignerDocument["presentation"]["widgets"][number]=>{
    const base={id,pageId,label:i===0?"Laser":i===1?"Emergency":i===2?"Reactor power":i===3?"Power needle":"Laser state",bounds:{x:semanticBounds[i][0]/profile.width,y:semanticBounds[i][1]/profile.height,width:semanticBounds[i][2]/profile.width,height:semanticBounds[i][3]/profile.height},controlIds:i<2?[controlIds[i]]:[],feedbackIds:i<2?[]:[outputIds[i-2]]};
    if(i<2)return{...base,kind:"button",config:{buttonMode:"momentary",pulseMs:0,repeatMs:0}};
    if(i===4)return{...base,kind:"indicator",config:{}};
    return{...base,kind:i===2?"numeric":"gauge",config:{min:0,max:100,unit:"%",precision:i===2?1:0}};
  });
  const document:DesignerDocument={schemaVersion:2,ui:{schemaVersion:1,id:"poc-"+board,title:"Companion systems "+revision.toUpperCase(),controls,feedbackWidgets,assets:[],
    pages:[{id:pageId,title:"Systems",items:widgets.flatMap(widget=>[...widget.controlIds.map(refId=>({kind:"control" as const,refId,bounds:widget.bounds})),...widget.feedbackIds.map(refId=>({kind:"feedback" as const,refId,bounds:widget.bounds}))])}]},
    presentation:{widgets,groups:[],viewports:[{pageId,width:profile.width,height:profile.height}],grid:{columns:16,rows:10,snap:false}},
    targets:[{id:"ship-target",label:"Ship systems",axes:[],buttons:2,hats:0}],bindings:controlIds.map((controlId,index)=>({controlId,targetId:"ship-target",kind:"button" as const,index}))};
  const images:PreparedImage[]=[art("background",profile.content[2],profile.content[3],"background")],nodes:NodeRecipe[]=[];
  const add=(node:NodeRecipe)=>nodes.push(node),image=(id:string,bounds:Rect,asset:PreparedImage)=>{images.push(asset);add({id,kind:"image565",bounds,imageId:asset.id});};
  add({id:"background",kind:"image565",bounds:profile.content,imageId:"background"});
  const labelFont=large?"rajdhani-20":"rajdhani-14",titleFont=large?"rajdhani-24":"rajdhani-18",valueFont=large?"rajdhani-36":"rajdhani-24";
  const text=(id:string,x:number,y:number,width:number,value:string,font=labelFont,color=C.white,height?:number)=>add({id,kind:"text",bounds:[x,y,width,height??fontCatalog.find(f=>f.id===font)!.lineHeight],fontId:font,text:value,color});
  text("heading",large?32:16,large?50:34,large?480:320,"SYSTEMS / MINING",titleFont,C.white);
  text("header-meta",large?600:328,large?56:40,large?168:140,"REV "+revision.toUpperCase()+" / LINK",labelFont,C.muted);
  add({id:"header-rule",kind:"rect",bounds:[large?32:16,large?98:72,large?736:448,1],color:rgb565(46,93,107)});
  if(large){
    image("actions-card",[24,116,208,166],art("actions-card",208,166,"card"));text("actions-label",36,116,180,"FLIGHT CONTROL",labelFont,C.muted);
    image("instrument-card",[260,116,292,246],art("instrument-card",292,246,"card"));
    image("status-card",[576,116,192,246],art("status-card",192,246,"card"));
    text("power-title",280,120,240,"REACTOR CAPACITOR",labelFont,C.muted);
    text("unit",486,202,32,"%",labelFont,C.muted);
    text("status-title",600,164,144,"SYSTEM STATUS",labelFont,C.muted);
    text("state-label",600,194,144,"LASER / ARMED",labelFont,C.white);
    text("telemetry-label",600,244,144,"TELEMETRY",labelFont,C.muted);
    text("status-note",600,280,144,"SOURCE / HOST",labelFont,C.white);
  }else{
    image("instrument-card",[144,80,128,104],art("instrument-card",128,104,"card"));
    text("controls-label",16,78,112,"CONTROLS",labelFont,C.muted);
    text("power-title",160,78,120,"CAPACITOR",labelFont,C.muted);
    text("unit",230,102,24,"%",labelFont,C.muted);
    text("status-title",288,116,168,"SYSTEM STATUS",labelFont,C.muted);
    text("state-label",288,138,168,"LASER / ARMED",labelFont,C.white);
    text("telemetry-label",288,176,168,"TELEMETRY",labelFont,C.muted);
  }
  const hits:ResolvedRecipe["hits"]=[];
  for(let i=0;i<2;++i){
    const b=buttonBounds[i],name=i?"stop":"laser";image(name+"-face",b,art(name+"-face",b[2],b[3],i?"warning":"button"));images.push(coverage(name+"-press",b[2],b[3],"press"));
    add({id:name+"-pressed",kind:"mask4",bounds:b,maskId:name+"-press",color:i?C.red:C.cyan,bindings:{visible:variable(name+"-pressed")}});
    if(large)text(name+"-label",b[0]+20,b[1]+12,b[2]-30,i?"EMERGENCY STOP":"ENABLE LASER",labelFont,i?C.red:C.white);
    else text(name+"-label",b[0],b[1]+48,60,i?"STOP":"LASER",labelFont,i?C.red:C.white);
    hits.push({interactionId:widgetIds[i],nodeId:name+"-face",bounds:b});
  }
  add({id:"numeric",kind:"text",bounds:numberBounds,fontId:valueFont,text:"0.0",maxTextBytes:8,precision:1,color:C.white,bindings:{number:variable("power-number")}});
  const gaugeX=large?292:160,gaugeY=large?316:156,range=large?224:60;
  add({id:"scale-track",kind:"line",bounds:[gaugeX,gaugeY,range+4,4],points:[[gaugeX,gaugeY+1],[gaugeX+range,gaugeY+1]],lineWidth:2,color:rgb565(51,87,100)});
  const ticks:(readonly [number,number])[]=[];for(let i=0;i<=4;++i){const x=gaugeX+Math.round(range*i/4);ticks.push([x,gaugeY],[x,gaugeY-4],[x,gaugeY+7],[x,gaugeY]);}add({id:"ticks",kind:"polyline",bounds:[gaugeX,gaugeY-4,range+4,12],points:ticks,lineWidth:1,color:C.muted});
  add({id:"needle",kind:"line",bounds:needleBounds,points:[[needleBounds[0]+1,needleBounds[1]],[needleBounds[0]+1,needleBounds[1]+needleBounds[3]-1]],lineWidth:2,color:C.cyan,
    bindings:{translateX:{kind:"op",op:"multiply",args:[{kind:"op",op:"divide",args:[variable("power-gauge"),scalar(100)]},scalar(range)]}}});
  const heat=operation("less",scalar(revision==="a"?70:65),variable("power-number"));
  add({id:"power-marker",kind:"rect",bounds:[large?292:160,large?290:128,large?224:64,large?6:4],color:C.cyan,bindings:{color:operation("select",heat,raw(C.amber),raw(C.cyan))}});
  add({id:"confirmed",kind:"rect",bounds:indicatorBounds,color:C.muted,bindings:{color:operation("select",variable("confirmed"),raw(C.green),raw(C.muted))}});
  const staleBounds:Rect=large?[600,258,128,10]:[288,198,48,8];
  add({id:"stale-lamp",kind:"rect",bounds:staleBounds,color:C.amber,bindings:{visible:operation("not",variable("fresh"))},timelines:[{property:"color",interpolation:"step",repeat:true,keyframes:[[0,C.amber],[500,rgb565(104,71,28)],[1000,C.amber]]}]});
  text("stale-caption",large?600:344,large?316:192,large?144:112,"AGE / CHECK",labelFont,C.muted);
  text("footer",large?32:16,large?406:270,large?736:448,revision==="a"?"A / PREPARED ARTWORK. LIVE VARIABLES.":"B / NEW COMPOSITE + SAME ENGINE.",labelFont,C.muted);
  if(revision==="b"){
    if(large){images.push(coverage("status-ring",136,136,"ring"));add({id:"status-ring",kind:"mask4",bounds:[348,140,136,136],maskId:"status-ring",color:C.cyan,bindings:{color:operation("select",heat,raw(C.amber),raw(C.cyan))}});text("ring-caption",366,244,128,"POWER / STATE",labelFont,C.muted);}
    else{images.push(coverage("stacked-warning",36,32,"stack"));add({id:"stacked-warning",kind:"mask4",bounds:[84,102,36,32],maskId:"stacked-warning",color:C.red,bindings:{visible:variable("confirmed")}});}
  }
  const needed=new Map<string,Set<number>>();
  for(const node of nodes)if(node.kind==="text"){const set=needed.get(node.fontId!)??new Set<number>();for(const char of node.text??"")set.add(char.codePointAt(0)!);if(node.bindings?.number)for(const char of "0123456789.-")set.add(char.codePointAt(0)!);set.add(63);needed.set(node.fontId!,set);}
  const fonts=fontCatalog.filter(font=>needed.has(font.id)).map(font=>({...font,glyphs:font.glyphs.filter(glyph=>needed.get(font.id)!.has(glyph.codepoint))}));
  return{document,recipe:{recipeVersion:1,id:"starter-"+board+"-"+revision,images,fonts,variables:[
    {id:"power-number",source:"feedback",outputId:"power-readout"},{id:"power-gauge",source:"feedback",outputId:"power-readout"},{id:"confirmed",source:"feedback",outputId:"laser-state"},
    {id:"laser-pressed",source:"pressed",interactionId:"laser-button"},{id:"stop-pressed",source:"pressed",interactionId:"stop-button"},
    {id:"fresh",source:"fresh",outputId:"power-readout"},{id:"available",source:"available",outputId:"power-readout"},{id:"connected",source:"connected"}],
    pages:[{id:pageId,background:C.background,nodes}],hits}};
}
