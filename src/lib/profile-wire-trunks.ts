import { diagramSmoothPath, type DiagramNode, type DiagramPoint } from './profile-diagram';
import type { ProfileWireStyle } from './profiles';
import type { DiagramWireBundle, DiagramWireRoute } from './profile-wire-bundles';

export type DiagramWireTrunk<T extends DiagramWireRoute = DiagramWireRoute> = {
  id:string;
  direction:'input'|'output';
  endpoint:DiagramPoint;
  junction:DiagramPoint;
  route:T;
  /** Original logical routes, rather than the derived paths used for drawing. */
  members:readonly T[];
};
type Box={left:number;right:number;top:number;bottom:number};
type Curve={a:DiagramPoint;b:DiagramPoint;c:DiagramPoint;d:DiagramPoint;radius?:number;sweep?:number};
type Plan={id:string;direction:'input'|'output';anchor:DiagramPoint;bus:DiagramPoint;tap:DiagramPoint;exit:DiagramPoint;radius:number;pieces:Curve[]};
type State<T extends DiagramWireRoute>={bundle:DiagramWireBundle<T>;points:DiagramPoint[];output?:Plan;input?:Plan};
const epsilon=1e-7, kappa=.5522847498307936;
const same=(a:DiagramPoint,b:DiagramPoint)=>a.x===b.x&&a.y===b.y;
const distance=(a:DiagramPoint,b:DiagramPoint)=>Math.hypot(a.x-b.x,a.y-b.y);
const middle=(a:DiagramPoint,b:DiagramPoint)=>({x:(a.x+b.x)/2,y:(a.y+b.y)/2});
const lerp=(a:DiagramPoint,b:DiagramPoint,t:number)=>({x:a.x+(b.x-a.x)*t,y:a.y+(b.y-a.y)*t});
const length=(points:readonly DiagramPoint[])=>points.slice(1).reduce((sum,point,index)=>sum+distance(point,points[index]),0);
const pointKey=(point:DiagramPoint)=>`${point.x},${point.y}`;
const compare=(a:string,b:string)=>a<b?-1:a>b?1:0;

function compact(points:readonly DiagramPoint[],protectedPoints:readonly DiagramPoint[]=[]):DiagramPoint[]{
  const result:DiagramPoint[]=[];
  for(const point of points){
    if(result.length&&same(result.at(-1)!,point))continue;
    result.push({...point});
    while(result.length>=3){
      const [a,b,c]=result.slice(-3);
      if(protectedPoints.some(point=>same(point,b)))break;
      if((a.x===b.x&&b.x===c.x&&(b.y-a.y)*(c.y-b.y)>=0)||(a.y===b.y&&b.y===c.y&&(b.x-a.x)*(c.x-b.x)>=0))result.splice(result.length-2,1);
      else break;
    }
  }
  return result;
}
function crosses(a:DiagramPoint,b:DiagramPoint,box:Box):boolean{
  return a.x===b.x?a.x>box.left+epsilon&&a.x<box.right-epsilon&&Math.max(a.y,b.y)>box.top+epsilon&&Math.min(a.y,b.y)<box.bottom-epsilon:
    a.y===b.y?a.y>box.top+epsilon&&a.y<box.bottom-epsilon&&Math.max(a.x,b.x)>box.left+epsilon&&Math.min(a.x,b.x)<box.right-epsilon:true;
}
function intersects(a:DiagramPoint,b:DiagramPoint,c:DiagramPoint,d:DiagramPoint):boolean{
  const rx=b.x-a.x,ry=b.y-a.y,sx=d.x-c.x,sy=d.y-c.y,qx=c.x-a.x,qy=c.y-a.y,denominator=rx*sy-ry*sx;
  if(Math.abs(denominator)<epsilon){
    if(Math.abs(qx*ry-qy*rx)>epsilon)return false;
    const span=Math.hypot(rx,ry);if(span<epsilon)return false;
    const from=(qx*rx+qy*ry)/span,to=from+(sx*rx+sy*ry)/span;
    return Math.min(span,Math.max(from,to))-Math.max(0,Math.min(from,to))>epsilon;
  }
  const t=(qx*sy-qy*sx)/denominator,u=(qx*ry-qy*rx)/denominator;
  return t>=-epsilon&&t<=1+epsilon&&u>=-epsilon&&u<=1+epsilon;
}
function simple(points:readonly DiagramPoint[]):boolean{
  for(let index=1;index<points.length;index++){
    if(index>1){const a=points[index-2],b=points[index-1],c=points[index];if((a.x===b.x&&b.x===c.x&&(b.y-a.y)*(c.y-b.y)<0)||(a.y===b.y&&b.y===c.y&&(b.x-a.x)*(c.x-b.x)<0))return false;}
    for(let other=1;other<index-1;other++)if(intersects(points[other-1],points[other],points[index-1],points[index]))return false;
  }
  return true;
}
function clearGuide(points:readonly DiagramPoint[],boxes:readonly Box[]):boolean{
  return points.length>=2&&points.length<=128&&points.every((point,index)=>Number.isFinite(point.x)&&Number.isFinite(point.y)&&Math.abs(point.x)<=1_000_000&&Math.abs(point.y)<=1_000_000&&(!index||(point.x===points[index-1].x)!==(point.y===points[index-1].y)))&&
    points[1].y===points[0].y&&points[1].x>points[0].x&&points.at(-1)!.y===points.at(-2)!.y&&points.at(-1)!.x>points.at(-2)!.x&&
    !points.slice(1).some((point,index)=>boxes.some(box=>crosses(points[index],point,box)))&&simple(points);
}
function straight(a:DiagramPoint,d:DiagramPoint):Curve{return {a:{...a},b:lerp(a,d,1/3),c:lerp(a,d,2/3),d:{...d}};}
function turn(a:DiagramPoint,corner:DiagramPoint,d:DiagramPoint,radius:number):Curve{
  return {a,b:lerp(a,corner,kappa),c:lerp(d,corner,kappa),d,radius,sweep:(corner.x-a.x)*(d.y-corner.y)-(corner.y-a.y)*(d.x-corner.x)>0?1:0};
}
function reverse(pieces:readonly Curve[]):Curve[]{return [...pieces].reverse().map(piece=>({...piece,a:piece.d,b:piece.c,c:piece.b,d:piece.a,sweep:piece.sweep===undefined?undefined:1-piece.sweep}));}
function path(pieces:readonly Curve[],style:ProfileWireStyle):string{
  if(!pieces.length)return '';
  return `M${pointKey(pieces[0].a)}`+pieces.map(piece=>style==='smooth'?` C${pointKey(piece.b)} ${pointKey(piece.c)} ${pointKey(piece.d)}`:piece.radius?` A${piece.radius},${piece.radius} 0 0 ${piece.sweep} ${pointKey(piece.d)}`:` L${pointKey(piece.d)}`).join('');
}
function curveCrosses(curve:Curve,box:Box,depth=0):boolean{
  const {a,b,c,d}=curve;
  if(Math.max(a.x,b.x,c.x,d.x)<=box.left+epsilon||Math.min(a.x,b.x,c.x,d.x)>=box.right-epsilon||Math.max(a.y,b.y,c.y,d.y)<=box.top+epsilon||Math.min(a.y,b.y,c.y,d.y)>=box.bottom-epsilon)return false;
  const inside=(p:DiagramPoint)=>p.x>box.left+epsilon&&p.x<box.right-epsilon&&p.y>box.top+epsilon&&p.y<box.bottom-epsilon;
  if(inside(a)||inside(d))return true;
  if(depth>=10)return inside({x:(a.x+3*b.x+3*c.x+d.x)/8,y:(a.y+3*b.y+3*c.y+d.y)/8});
  const ab=middle(a,b),bc=middle(b,c),cd=middle(c,d),abc=middle(ab,bc),bcd=middle(bc,cd),m=middle(abc,bcd);
  return curveCrosses({a,b:ab,c:abc,d:m},box,depth+1)||curveCrosses({a:m,b:bcd,c:cd,d},box,depth+1);
}
function curvePoints(pieces:readonly Curve[]):DiagramPoint[]{
  const result:DiagramPoint[]=[];
  for(const {a,b,c,d} of pieces)for(let step=0;step<=8;step++){
    const t=step/8,s=1-t,point={x:s*s*s*a.x+3*s*s*t*b.x+3*s*t*t*c.x+t*t*t*d.x,y:s*s*s*a.y+3*s*s*t*b.y+3*s*t*t*c.y+t*t*t*d.y};
    if(!result.length||distance(result.at(-1)!,point)>epsilon)result.push(point);
  }
  return result;
}
function safeCurves(pieces:readonly Curve[],boxes:readonly Box[],checkSimple=true):boolean{
  return !pieces.some(piece=>boxes.some(box=>curveCrosses(piece,box)))&&(!checkSimple||simple(curvePoints(pieces)));
}
function rounded(points:readonly DiagramPoint[],radius:number):Curve[]{
  const corners=compact(points);if(corners.length<2)return [];
  const result:Curve[]=[];let cursor=corners[0];
  for(let index=1;index<corners.length-1;index++){
    const previous=corners[index-1],corner=corners[index],next=corners[index+1],r=Math.min(radius,distance(previous,corner)/2,distance(corner,next)/2);
    const before=lerp(corner,previous,r/distance(previous,corner)),after=lerp(corner,next,r/distance(corner,next));
    if(!same(cursor,before))result.push(straight(cursor,before));
    if(r)result.push(turn(before,corner,after,r));
    cursor=after;
  }
  if(!same(cursor,corners.at(-1)!))result.push(straight(cursor,corners.at(-1)!));
  return result;
}
function parseSmooth(value:string):Curve[]{
  const values=value.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi)?.map(Number)??[];
  if(values.length<8||(values.length-2)%6)return [];
  let a={x:values[0],y:values[1]};const result:Curve[]=[];
  for(let index=2;index<values.length;index+=6){const d={x:values[index+4],y:values[index+5]};result.push({a,b:{x:values[index],y:values[index+1]},c:{x:values[index+2],y:values[index+3]},d});a=d;}
  return result;
}
function centerPieces(points:readonly DiagramPoint[],style:ProfileWireStyle,radius:number,boxes:readonly Box[],before:readonly Curve[],after:readonly Curve[],preview=false):Curve[]|null{
  if(points.length<2)return [];
  // The committed path is validated against every card and its complete curve.
  // Drag previews retain the already validated orthogonal topology rather than
  // repeating a curve/card intersection pass for every signal on every frame.
  if(preview)return style==='smooth'?rounded(points,Math.min(radius,8)):rounded(points,radius);
  const safe=(pieces:Curve[])=>safeCurves([...before,...pieces,...after],boxes);
  if(style==='smooth'){
    const a=points[0],d=points.at(-1)!,first=points[1],last=points.at(-2)!,firstLength=distance(a,first),lastLength=distance(last,d),span=distance(a,d);
    // Try one curve before retaining obstacle detours. Trunk joins retain the
    // actual guide tangent, including backwards-running branches.
    for(const factor of [.45,.25,.1]){
      const reach=Math.min(240,span*factor),curve={a,b:lerp(a,first,reach/firstLength),c:lerp(d,last,reach/lastLength),d};
      const controlLength=distance(a,curve.b)+distance(curve.b,curve.c)+distance(curve.c,d);
      if(controlLength<=length(points)*1.5&&safe([curve]))return [curve];
    }
    const fitted=parseSmooth(diagramSmoothPath(points));
    const aligned=(x:number,y:number,dx:number,dy:number)=>x*dx+y*dy>epsilon&&Math.abs(x*dy-y*dx)<=epsilon*Math.max(1,Math.hypot(x,y)*Math.hypot(dx,dy));
    // The generic two-point fitter assumes eastbound endpoints. A bus branch
    // can leave westbound, so preserve its real tangent at both composed joins.
    const firstCurve=fitted[0],lastCurve=fitted.at(-1);
    if(firstCurve&&lastCurve&&aligned(firstCurve.b.x-firstCurve.a.x,firstCurve.b.y-firstCurve.a.y,first.x-a.x,first.y-a.y)&&
      aligned(lastCurve.d.x-lastCurve.c.x,lastCurve.d.y-lastCurve.c.y,d.x-last.x,d.y-last.y)&&safe(fitted))return fitted;
  }
  for(const limit of [radius,Math.min(radius,8),Math.min(radius,2),0]){const pieces=rounded(points,limit);if(safe(pieces))return pieces;}
  return null;
}
function protectedPoints<T extends DiagramWireRoute>(state:State<T>):DiagramPoint[]{return [state.output?.anchor,state.output?.bus,state.output?.tap,state.output?.exit,state.input?.anchor,state.input?.bus,state.input?.tap,state.input?.exit].filter((point):point is DiagramPoint=>!!point);}

/** Reuse an existing horizontal corridor; never search a new visibility grid.
 * The opposite endpoint's already accepted bus is an immutable suffix. */
function attachBus<T extends DiagramWireRoute>(state:State<T>,direction:'input'|'output',busX:number,boxes:readonly Box[],preview:boolean):DiagramPoint[]|null{
  const outgoing=direction==='output',oriented=outgoing?state.points:[...state.points].reverse(),anchor=oriented[0],bus={x:busX,y:anchor.y},opposite=outgoing?state.input:state.output;
  const protectedIndex=opposite?oriented.findIndex(point=>same(point,opposite.exit)):oriented.length-1;
  if(protectedIndex<1)return null;
  const limit=Math.min(protectedIndex,preview?10:18),oldLength=length(oriented);let best:DiagramPoint[]|null=null,bestCost=Infinity;
  for(let index=0;index<limit;index++){
    const a=oriented[index],b=oriented[index+1];if(!b||a.y!==b.y)continue;
    const tap={x:busX,y:a.y};
    const candidate=compact([anchor,bus,tap,...oriented.slice(index+1)], [...protectedPoints(state),bus,tap]);
    const normal=outgoing?candidate:[...candidate].reverse();
    const candidateLength=length(candidate);if(candidateLength>oldLength+Math.max(192,oldLength*.25)||!clearGuide(normal,preview?[]:boxes))continue;
    const cost=candidateLength+candidate.length*8+index*.01;
    if(cost<bestCost){best=normal;bestCost=cost;}
  }
  return best;
}
function makePlan(points:DiagramPoint[],direction:'input'|'output',id:string,radius:number):{plan:Plan;points:DiagramPoint[]}|null{
  const outgoing=direction==='output',oriented=outgoing?points:[...points].reverse(),anchor=oriented[0],bus=oriented[1];
  let tap=oriented[2];
  if(!bus||!tap)return null;
  // A straight connection can have the bus station followed by its target.
  if(bus.y===tap.y)tap=bus;
  const vertical=Math.sign(tap.y-anchor.y),next=oriented[tap===bus?2:3];
  if(!next)return null;
  const horizontal=Math.sign(next.x-tap.x),exitRadius=vertical?Math.min(radius,distance(tap,next)/2):0;
  if(!horizontal)return null;
  const fork={x:bus.x-Math.sign(bus.x-anchor.x)*radius,y:anchor.y},pieces:Curve[]=[];
  if(!same(anchor,fork))pieces.push(straight(anchor,fork));
  let exit:DiagramPoint;
  if(vertical){
    const entry={x:bus.x,y:anchor.y+vertical*radius},before={x:bus.x,y:tap.y-vertical*exitRadius};
    if(radius)pieces.push(turn(fork,bus,entry,radius));
    if(!same(entry,before))pieces.push(straight(entry,before));
    exit={x:bus.x+horizontal*exitRadius,y:tap.y};
    if(exitRadius)pieces.push(turn(before,tap,exit,exitRadius));
  }else{
    exit=bus;if(!same(fork,bus))pieces.push(straight(fork,bus));
  }
  const tapIndex=tap===bus?1:2,withExit=[...oriented.slice(0,tapIndex+1),exit,...oriented.slice(tapIndex+1)];
  const result=outgoing?withExit:[...withExit].reverse();
  return {plan:{id,direction,anchor:{...anchor},bus:{...bus},tap:{...tap},exit,radius,pieces},points:result.filter((point,index)=>!index||!same(point,result[index-1])).map(point=>({...point}))};
}

/** Share physical endpoint buses without changing signal identities or saved
 * geometry. Only a bounded set of existing horizontal corridors is examined;
 * dragging never invokes ELK or a global route search. */
export function buildDiagramWireTrunks<T extends DiagramWireRoute>(
  bundles:readonly DiagramWireBundle<T>[],nodes:readonly DiagramNode[],width:number,height:number,style:ProfileWireStyle,cornerRadius:number,
  preview=false,selectedBindingId:string|null=null,
):{bundles:DiagramWireBundle<T>[];trunks:DiagramWireTrunk<T>[]} {
  const boxes=nodes.map(node=>({left:node.position.x,right:node.position.x+(node.width??width),top:node.position.y,bottom:node.position.y+(node.height??height)}));
  const requestedRadius=Number.isFinite(cornerRadius)?Math.max(0,Math.min(120,cornerRadius)):24;
  const states:State<T>[]=bundles.map(bundle=>({bundle,points:bundle.route.points.map(point=>({...point}))}));
  const eligible=(state:State<T>)=>!state.bundle.members.some(route=>route.manual||route.binding.id===selectedBindingId)&&!state.bundle.route.customBlocked&&clearGuide(state.points,preview?[]:boxes);
  for(const direction of ['output','input'] as const){
    const outgoing=direction==='output',sign=outgoing?1:-1,groups=new Map<string,State<T>[]>();
    for(const state of states){
      if(!eligible(state))continue;
      const anchor=outgoing?state.points[0]:state.points.at(-1)!,nodeId=outgoing?state.bundle.route.binding.from.nodeId:state.bundle.route.binding.to.nodeId;
      const id=`trunk:${JSON.stringify([direction,nodeId,anchor.x,anchor.y])}`,group=groups.get(id)??[];group.push(state);groups.set(id,group);
    }
    for(const [id,group] of groups){
      if(group.length<2)continue;
      group.sort((a,b)=>compare(a.bundle.id,b.bundle.id));
      const anchor=outgoing?group[0].points[0]:group[0].points.at(-1)!;
      const forward=group.map(state=>{const other=outgoing?state.points.at(-1)!:state.points[0];return (other.x-anchor.x)*sign;}).filter(value=>value>0);
      const maximum=Math.min(120,forward.length?Math.min(...forward)*.4:120);
      const reaches=[maximum,...(preview?[72,48,24]:[96,72,48,32,24])].filter((reach,index,array)=>reach>=16&&reach<=maximum&&array.indexOf(reach)===index);
      let chosen:{state:State<T>;points:DiagramPoint[]}[]=[],chosenReach=0;
      for(const reach of reaches){
        const busX=anchor.x+sign*reach;if(boxes.some(box=>crosses(anchor,{x:busX,y:anchor.y},box)))continue;
        const candidates=group.flatMap(state=>{const points=attachBus(state,direction,busX,boxes,preview);return points?[{state,points}]:[];});
        if(candidates.length>chosen.length||candidates.length===chosen.length&&reach>chosenReach){chosen=candidates;chosenReach=reach;}
        if(chosen.length===group.length)break;
      }
      if(chosen.length<2)continue;
      const verticalDistances=chosen.map(({points})=>{const oriented=outgoing?points:[...points].reverse();return Math.abs((oriented[2]?.y??anchor.y)-anchor.y);}).filter(value=>value>0);
      let radius=Math.min(requestedRadius,chosenReach/3,verticalDistances.length?Math.min(...verticalDistances)/3:requestedRadius);
      // Rounded bus entry/exit curves must obey the same actual card clearance
      // as the guide. Tight passages retain a smaller shared corner radius.
      let accepted:{state:State<T>;plan:Plan;points:DiagramPoint[]}[]=[];
      for(const limit of [radius,Math.min(radius,8),Math.min(radius,2),0]){
        const plans=chosen.flatMap(({state,points})=>{const result=makePlan(points,direction,id,limit);return result&&(preview||safeCurves(result.plan.pieces,boxes))?[{state,...result}]:[];});
        if(plans.length>accepted.length){accepted=plans;radius=limit;}
        if(plans.length===chosen.length)break;
      }
      if(accepted.length<2)continue;
      for(const {state,plan,points} of accepted){state.points=compact(points,[...protectedPoints(state),plan.anchor,plan.bus,plan.tap,plan.exit]);state[direction]=plan;}
    }
  }
  const successful=new Set<State<T>>(),rendered:DiagramWireBundle<T>[]=[];
  for(const state of states){
    const {bundle,output,input}=state;if(!output&&!input){rendered.push(bundle);continue;}
    const begin=output?state.points.findIndex(point=>same(point,output.exit)):0,end=input?state.points.findIndex(point=>same(point,input.exit)):state.points.length-1;
    if(begin<0||end<=begin){rendered.push(bundle);continue;}
    const before=output?.pieces??[],after=input?reverse(input.pieces):[],central=centerPieces(state.points.slice(begin,end+1),style,requestedRadius,boxes,before,after,preview);
    if(!central){rendered.push(bundle);continue;}
    const pieces=[...before,...central,...after],route={...bundle.route,points:state.points,path:path(pieces,style),cornerRadius:requestedRadius} as T;
    rendered.push({...bundle,route});successful.add(state);
  }
  const trunks:DiagramWireTrunk<T>[]=[],plans=new Map<string,{state:State<T>;plan:Plan}[]>();
  for(const state of states)if(successful.has(state))for(const plan of [state.output,state.input])if(plan){const group=plans.get(plan.id)??[];group.push({state,plan});plans.set(plan.id,group);}
  const add=(id:string,entries:{state:State<T>;plan:Plan}[],pieces:Curve[],points:DiagramPoint[])=>{
    if(entries.length<2||!pieces.length||length(points)<4)return;
    const first=entries[0],members=entries.flatMap(entry=>[...entry.state.bundle.members]).sort((a,b)=>compare(a.binding.id,b.binding.id)),direction=first.plan.direction;
    const ordered=direction==='output'?pieces:reverse(pieces),geometry=direction==='output'?points:[...points].reverse();
    trunks.push({id,direction,endpoint:first.plan.anchor,junction:points.at(-1)!,members,route:{...first.state.bundle.route,points:geometry,path:path(ordered,style),cornerRadius:requestedRadius} as T});
  };
  for(const [id,entries] of plans){
    const {plan}=entries[0],sign=Math.sign(plan.bus.x-plan.anchor.x),fork={x:plan.bus.x-sign*plan.radius,y:plan.anchor.y};
    add(`${id}:stem`,entries,[straight(plan.anchor,fork)],[plan.anchor,fork]);
    for(const vertical of [-1,1]){
      const branch=entries.filter(entry=>Math.sign(entry.plan.tap.y-entry.plan.anchor.y)===vertical);if(branch.length<2)continue;
      const entry={x:plan.bus.x,y:plan.anchor.y+vertical*plan.radius};
      if(plan.radius)add(`${id}:turn:${vertical}`,branch,[turn(fork,plan.bus,entry,plan.radius)],[fork,plan.bus,entry]);
      const ends=branch.map(({plan:member})=>member.pieces.find(piece=>piece.radius&&same(piece.d,member.exit))?.a.y??member.tap.y);
      const stations=[entry.y,...ends].sort((a,b)=>(a-b)*vertical).filter((value,index,array)=>index===0||value!==array[index-1]);
      for(let index=1;index<stations.length;index++){
        const from=stations[index-1],to=stations[index],members=branch.filter((_,member)=>(ends[member]-to)*vertical>=-epsilon);
        add(`${id}:spine:${vertical}:${from}:${to}`,members,[straight({x:plan.bus.x,y:from},{x:plan.bus.x,y:to})],[{x:plan.bus.x,y:from},{x:plan.bus.x,y:to}]);
      }
    }
  }
  // A failed last-stage curve fit must not leave a one-member bus advertised as
  // shared. Its derived geometry remains a safe ordinary route.
  return {bundles:rendered,trunks};
}
