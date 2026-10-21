import type { DeepReadonly, ProfileBinding, ProfileWireRoute, ProfileWireStyle } from './profiles';
import { profileAnchorWireRoute, profileWireCorners } from './profile-wire-layout';
import { refineDiagramLanes } from './profile-diagram-lanes';
export { diagramRouteQuality } from './profile-diagram-lanes';

type Edge = DeepReadonly<Pick<ProfileBinding, 'from' | 'to'>>;
export type DiagramPoint = { x:number; y:number };
export type DiagramNode = {id:string;position:DiagramPoint;width?:number;height?:number;inputAnchors?:Readonly<Record<string,DiagramPoint>>;outputAnchors?:Readonly<Record<string,DiagramPoint>>};
export type DiagramRouteOptions = {style?:ProfileWireStyle;cornerRadius?:number;wireRoutes?:readonly DeepReadonly<ProfileWireRoute>[]};
type Box = {left:number;right:number;top:number;bottom:number};

/** The side of each footer is the single attachment shared by all its signals. */
export function diagramPortAnchor(position:DiagramPoint,direction:'input'|'output',width:number,height:number):DiagramPoint {
  return {x:position.x+(direction==='output'?width:0),y:position.y+height-20};
}

/** A tangent-continuous preview uses the same attachment as a saved connection. */
export function diagramPendingPath(start:DiagramPoint,end:DiagramPoint,direction:'input'|'output',style:ProfileWireStyle='smooth',cornerRadius=24) {
  const reach=Math.max(48,Math.min(240,Math.abs(end.x-start.x)*.5)),sign=direction==='output'?1:-1;
  if(style==='orthogonal'){
    const lane=start.x+reach*sign;
    return diagramOrthogonalPath([start,{x:lane,y:start.y},{x:lane,y:end.y},end],cornerRadius);
  }
  return `M${start.x},${start.y} C${start.x+reach*sign},${start.y} ${end.x-reach*sign},${end.y} ${end.x},${end.y}`;
}
function bendRadius(points:readonly DiagramPoint[],index:number,cornerRadius:number){
  if(index<=0||index>=points.length-1)return 0;
  const a=points[index-1],b=points[index],c=points[index+1];
  if((a.x===b.x&&b.x===c.x)||(a.y===b.y&&b.y===c.y))return 0;
  return Math.min(Number.isFinite(cornerRadius)?Math.max(0,cornerRadius):24,Math.hypot(a.x-b.x,a.y-b.y)/2,Math.hypot(c.x-b.x,c.y-b.y)/2);
}

/** The draggable part of a lane excludes its rounded corners. */
export function diagramSegmentLine(points:readonly DiagramPoint[],index:number,cornerRadius=24):{start:DiagramPoint;end:DiagramPoint}|null {
  if(index<0||index>=points.length-1)return null;
  const a=points[index],b=points[index+1],length=Math.hypot(b.x-a.x,b.y-a.y);
  if(!length)return null;
  const startRadius=bendRadius(points,index,cornerRadius),endRadius=bendRadius(points,index+1,cornerRadius);
  if(length-startRadius-endRadius<1e-8)return null;
  const dx=(b.x-a.x)/length,dy=(b.y-a.y)/length;
  return {start:{x:a.x+dx*startRadius,y:a.y+dy*startRadius},end:{x:b.x-dx*endRadius,y:b.y-dy*endRadius}};
}

/** Axis-aligned lanes with circular, tangent-continuous corner arcs. */
export function diagramOrthogonalPath(points:readonly DiagramPoint[],cornerRadius=24) {
  if(!points.length)return '';
  let path=`M${points[0].x},${points[0].y}`;
  for(let i=1;i<points.length-1;i++){
    const a=points[i-1],b=points[i],c=points[i+1];
    const radius=bendRadius(points,i,cornerRadius);
    if(!radius){path+=` L${b.x},${b.y}`;continue;}
    const before={x:b.x+Math.sign(a.x-b.x)*radius,y:b.y+Math.sign(a.y-b.y)*radius};
    const after={x:b.x+Math.sign(c.x-b.x)*radius,y:b.y+Math.sign(c.y-b.y)*radius};
    const clockwise=(b.x-a.x)*(c.y-b.y)-(b.y-a.y)*(c.x-b.x)>0?1:0;
    path+=` L${before.x},${before.y} A${radius},${radius} 0 0 ${clockwise} ${after.x},${after.y}`;
  }
  const last=points.at(-1)!;return path+` L${last.x},${last.y}`;
}

type Cubic={start:DiagramPoint;first:DiagramPoint;second:DiagramPoint;end:DiagramPoint};
function smoothCurves(points:readonly DiagramPoint[],maximumHandle=Infinity,preserved:readonly DiagramPoint[]=[]):Cubic[]{
  const corners=profileWireCorners(points);
  points=corners.flatMap((point,index)=>{
    const next=corners[index+1];if(!next)return [point];
    const interior=preserved.filter(handle=>((point.x===next.x&&handle.x===point.x&&handle.y>Math.min(point.y,next.y)&&handle.y<Math.max(point.y,next.y))||(point.y===next.y&&handle.y===point.y&&handle.x>Math.min(point.x,next.x)&&handle.x<Math.max(point.x,next.x)))).sort((a,b)=>(Math.abs(a.x-point.x)+Math.abs(a.y-point.y))-(Math.abs(b.x-point.x)+Math.abs(b.y-point.y)));
    return [point,...interior];
  });
  if(points.length<2)return [];
  const unit=(x:number,y:number)=>{const length=Math.hypot(x,y);return length?{x:x/length,y:y/length}:{x:1,y:0};};
  const tangents=points.map((point,index)=>{
    if(!index||index===points.length-1)return {x:1,y:0};
    const previous=points[index-1],next=points[index+1],incoming=unit(point.x-previous.x,point.y-previous.y),outgoing=unit(next.x-point.x,next.y-point.y);
    return unit(incoming.x+outgoing.x,incoming.y+outgoing.y);
  });
  const handles=points.map((point,index)=>{
    const previous=points[Math.max(0,index-1)],next=points[Math.min(points.length-1,index+1)];
    const before=Math.hypot(point.x-previous.x,point.y-previous.y),after=Math.hypot(next.x-point.x,next.y-point.y);
    return Math.min(maximumHandle,index&&index<points.length-1?Math.min(before,after)*.5:Math.max(48,Math.min(240,(before||after)*.5)));
  });
  return points.slice(1).map((end,index)=>({start:points[index],first:{x:points[index].x+tangents[index].x*handles[index],y:points[index].y+tangents[index].y*handles[index]},second:{x:end.x-tangents[index+1].x*handles[index+1],y:end.y-tangents[index+1].y*handles[index+1]},end}));
}
function cubicPath(curves:readonly Cubic[],start?:DiagramPoint){
  if(!curves.length)return start?`M${start.x},${start.y}`:'';
  return `M${curves[0].start.x},${curves[0].start.y}`+curves.map(({first,second,end})=>` C${first.x},${first.y} ${second.x},${second.y} ${end.x},${end.y}`).join('');
}
function flowingCurves(points:readonly DiagramPoint[]):Cubic[]{
  points=profileWireCorners(points);
  if(points.length<3)return smoothCurves(points);
  const middle=(a:DiagramPoint,b:DiagramPoint)=>({x:(a.x+b.x)/2,y:(a.y+b.y)/2});
  return points.slice(1,-1).map((corner,index)=>{
    const start=index?middle(points[index],corner):points[0],end=index===points.length-3?points.at(-1)!:middle(corner,points[index+2]);
    return {start,first:{x:start.x+(corner.x-start.x)*2/3,y:start.y+(corner.y-start.y)*2/3},second:{x:end.x+(corner.x-end.x)*2/3,y:end.y+(corner.y-end.y)*2/3},end};
  });
}
/** Cheap previews share the flowing fit. Authored knots remain interpolated. */
export function diagramSmoothPath(points:readonly DiagramPoint[],preserved:readonly DiagramPoint[]=[]) {
  return cubicPath(preserved.length?smoothCurves(points,18,preserved):flowingCurves(points),points[0]);
}

function cubicCrossesBox(curve:Cubic,box:Box,depth=0):boolean {
  const {start,first,second,end}=curve;
  if(Math.max(start.x,first.x,second.x,end.x)<=box.left||Math.min(start.x,first.x,second.x,end.x)>=box.right||Math.max(start.y,first.y,second.y,end.y)<=box.top||Math.min(start.y,first.y,second.y,end.y)>=box.bottom)return false;
  const inside=(p:DiagramPoint)=>p.x>box.left&&p.x<box.right&&p.y>box.top&&p.y<box.bottom;
  if(inside(curve.start)||inside(curve.end))return true;
  if(depth===12)return inside({x:(curve.start.x+3*curve.first.x+3*curve.second.x+curve.end.x)/8,y:(curve.start.y+3*curve.first.y+3*curve.second.y+curve.end.y)/8});
  const middle=(a:DiagramPoint,b:DiagramPoint)=>({x:(a.x+b.x)/2,y:(a.y+b.y)/2});
  const a=middle(curve.start,curve.first),b=middle(curve.first,curve.second),c=middle(curve.second,curve.end),d=middle(a,b),e=middle(b,c),m=middle(d,e);
  return cubicCrossesBox({start:curve.start,first:a,second:d,end:m},box,depth+1)||cubicCrossesBox({start:m,first:e,second:c,end:curve.end},box,depth+1);
}
function smoothPathCrossesBoxes(path:string,boxes:readonly Box[]):boolean {
  const values=path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi)?.map(Number)??[];
  let start={x:values[0],y:values[1]};
  for(let index=2;index+5<values.length;index+=6){
    const curve={start,first:{x:values[index],y:values[index+1]},second:{x:values[index+2],y:values[index+3]},end:{x:values[index+4],y:values[index+5]}};
    if(boxes.some(box=>cubicCrossesBox(curve,box)))return true;
    start=curve.end;
  }
  return false;
}

function arcCrossesBox(points:readonly DiagramPoint[],index:number,cornerRadius:number,box:Box){
  const radius=bendRadius(points,index,cornerRadius);if(!radius)return false;
  const a=points[index-1],b=points[index],c=points[index+1];
  const incoming={x:Math.sign(b.x-a.x),y:Math.sign(b.y-a.y)},outgoing={x:Math.sign(c.x-b.x),y:Math.sign(c.y-b.y)};
  const center={x:b.x-incoming.x*radius+outgoing.x*radius,y:b.y-incoming.y*radius+outgoing.y*radius};
  // A quarter circle is monotone in both coordinates. Reflect its quadrant
  // into positive x/y, then compare the rectangle's nearest/farthest radii.
  const signX=incoming.x||-outgoing.x,signY=incoming.y||-outgoing.y;
  const horizontal=[(box.left-center.x)*signX,(box.right-center.x)*signX],vertical=[(box.top-center.y)*signY,(box.bottom-center.y)*signY];
  const left=Math.max(0,Math.min(...horizontal)),right=Math.min(radius,Math.max(...horizontal)),top=Math.max(0,Math.min(...vertical)),bottom=Math.min(radius,Math.max(...vertical));
  if(left>=right||top>=bottom)return false;
  const squared=radius*radius;
  return left*left+top*top<squared-1e-8&&right*right+bottom*bottom>squared+1e-8;
}

function clearCornerRadius(points:readonly DiagramPoint[],requested:number,boxes:readonly Box[]){
  const safe=(radius:number)=>points.every((_,index)=>!index||index===points.length-1||boxes.every(box=>!arcCrossesBox(points,index,radius,box)));
  if(safe(requested))return requested;
  // Manual lanes remain in place. Only reduce their rounding if an oversized
  // corner would cut through a block, retaining the largest safe radius.
  let low=0,high=requested;
  for(let attempt=0;attempt<24;attempt++){const middle=(low+high)/2;if(safe(middle))low=middle;else high=middle;}
  return low;
}

/** Rank the directed graph, keeping independent signal chains in separate bands. */
export function diagramLayout(ids:readonly string[], bindings:readonly Edge[], width:number, height:number) {
  const known=new Set(ids), outgoing=new Map(ids.map(id=>[id,new Set<string>()])), incoming=new Map(ids.map(id=>[id,new Set<string>()]));
  for(const edge of bindings)if(known.has(edge.from.nodeId)&&known.has(edge.to.nodeId)){
    outgoing.get(edge.from.nodeId)!.add(edge.to.nodeId);incoming.get(edge.to.nodeId)!.add(edge.from.nodeId);
  }
  // Collapse feedback loops before ranking: every edge between components flows right.
  const visited=new Set<string>(), finished:string[]=[];
  function visit(id:string){if(visited.has(id))return;visited.add(id);for(const next of outgoing.get(id)!)visit(next);finished.push(id);}
  ids.forEach(visit);
  const component=new Map<string,number>(), members:string[][]=[];
  function assign(id:string,index:number){if(component.has(id))return;component.set(id,index);members[index].push(id);for(const next of incoming.get(id)!)assign(next,index);}
  for(const id of finished.reverse())if(!component.has(id)){members.push([]);assign(id,members.length-1);}
  const order=new Map(ids.map((id,index)=>[id,index]));
  members.forEach((group,index)=>{
    const remaining=new Set(group.sort((a,b)=>order.get(a)!-order.get(b)!)),left:string[]=[],right:string[]=[];
    const degree=(id:string,neighbors:Map<string,Set<string>>)=>[...neighbors.get(id)!].filter(peer=>remaining.has(peer)&&peer!==id).length;
    // A feedback cycle cannot put every edge forwards. Keep its dominant flow
    // rightwards and leave only the necessary return routes around the blocks.
    while(remaining.size){
      const sink=[...remaining].find(id=>degree(id,outgoing)===0);
      if(sink!==undefined){right.push(sink);remaining.delete(sink);continue;}
      const source=[...remaining].find(id=>degree(id,incoming)===0);
      const chosen=source??[...remaining].sort((a,b)=>(degree(b,outgoing)-degree(b,incoming))-(degree(a,outgoing)-degree(a,incoming)))[0];
      left.push(chosen);remaining.delete(chosen);
    }
    members[index]=[...left,...right.reverse()];
  });
  const next=members.map(()=>new Set<number>()), previous=members.map(()=>new Set<number>());
  for(const id of ids)for(const to of outgoing.get(id)!){const a=component.get(id)!,b=component.get(to)!;if(a!==b){next[a].add(b);previous[b].add(a);}}
  const pending=previous.map(group=>group.size), ranks=members.map(()=>0), queue=pending.flatMap((count,index)=>count?[]:[index]);
  for(let i=0;i<queue.length;i++){const a=queue[i];for(const b of next[a]){ranks[b]=Math.max(ranks[b],ranks[a]+members[a].length);if(--pending[b]===0)queue.push(b);}}
  const nodeRank=new Map(ids.map(id=>{const c=component.get(id)!;return[id,ranks[c]+members[c].indexOf(id)];}));
  const bands:string[][]=[], included=new Set<string>();
  for(const root of ids){if(included.has(root))continue;const band=[root];included.add(root);for(let i=0;i<band.length;i++)for(const neighbor of [...outgoing.get(band[i])!,...incoming.get(band[i])!])if(!included.has(neighbor)){included.add(neighbor);band.push(neighbor);}bands.push(band);}
  const result=new Map<string,DiagramPoint>();let top=48;
  // Isolated blocks share a compact final band rather than interrupting routed chains.
  const isolated=bands.filter(band=>band.length===1&&!outgoing.get(band[0])!.size&&!incoming.get(band[0])!.size).flat();
  for(const band of bands.filter(band=>!isolated.includes(band[0]))){
    const columns=new Map<number,string[]>();
    for(const id of band){const rank=nodeRank.get(id)!;columns.set(rank,[...(columns.get(rank)??[]),id]);}
    const keys=[...columns.keys()].sort((a,b)=>a-b), row=new Map<string,number>();
    for(const nodes of columns.values())nodes.sort((a,b)=>order.get(a)!-order.get(b)!).forEach((id,index)=>row.set(id,index));
    // Barycentre sweeps align branches and reduce crossings without changing ranks.
    for(let sweep=0;sweep<6;sweep++)for(const rank of sweep%2?[...keys].reverse():keys){
      const neighbors=sweep%2?outgoing:incoming;
      const score=(id:string)=>{const peers=[...neighbors.get(id)!].filter(peer=>nodeRank.get(peer)!==rank);return peers.length?peers.reduce((sum,peer)=>sum+row.get(peer)!,0)/peers.length:row.get(id)!;};
      columns.get(rank)!.sort((a,b)=>score(a)-score(b)||row.get(a)!-row.get(b)!).forEach((id,index)=>row.set(id,index));
    }
    const rows=Math.max(...[...columns.values()].map(nodes=>nodes.length));
    for(const [rank,nodes]of columns)nodes.forEach((id,index)=>result.set(id,{x:48+rank*(width+144),y:top+(index+(rows-nodes.length)/2)*(height+80)}));
    top+=rows*(height+80)+48;
  }
  const count=Math.max(1,Math.ceil(Math.sqrt(isolated.length)));
  isolated.forEach((id,index)=>result.set(id,{x:48+index%count*(width+144),y:top+Math.floor(index/count)*(height+80)}));
  return ids.map(nodeId=>({nodeId,...result.get(nodeId)!}));
}

function crosses(a:DiagramPoint,b:DiagramPoint,box:Box){
  return a.x===b.x?a.x>box.left&&a.x<box.right&&Math.max(a.y,b.y)>box.top&&Math.min(a.y,b.y)<box.bottom:
    a.y>box.top&&a.y<box.bottom&&Math.max(a.x,b.x)>box.left&&Math.min(a.x,b.x)<box.right;
}

/** Shared compressed visibility grid; search only visits the channels an edge needs. */
type DiagramRoute={binding:DeepReadonly<ProfileBinding>;points:DiagramPoint[];path:string;cornerRadius:number;manual:boolean;customBlocked:boolean};
export function diagramRoutes(nodes:readonly DiagramNode[],bindings:readonly DeepReadonly<ProfileBinding>[],width:number,height:number,options:DiagramRouteOptions={}):DiagramRoute[]{
  return routeDiagram(nodes,bindings,width,height,options);
}
function routeDiagram(nodes:readonly DiagramNode[], bindings:readonly DeepReadonly<ProfileBinding>[],width:number,height:number,options:DiagramRouteOptions={},retained:readonly DiagramRoute[]=[]):DiagramRoute[]{
  const requestedRadius=Number.isFinite(options.cornerRadius)?Math.max(0,options.cornerRadius!):24;
  const expanded=options.style==='orthogonal'?Math.max(20,Math.ceil(requestedRadius*(1-Math.SQRT1_2))+1):20;
  const expandedBoxes=nodes.map(node=>({left:node.position.x-expanded,right:node.position.x+(node.width??width)+expanded,top:node.position.y-expanded,bottom:node.position.y+(node.height??height)+expanded}));
  // Wide corners can claim more room in spacious layouts. Existing tight
  // layouts keep their channels and cap rounding instead of hiding wires.
  const crowded=expandedBoxes.some((a,index)=>expandedBoxes.slice(index+1).some(b=>a.left<b.right&&a.right>b.left&&a.top<b.bottom&&a.bottom>b.top));
  const clearance=crowded?20:expanded,laneSpacing=20, byId=new Map(nodes.map(node=>[node.id,node]));
  const saved=new Map(options.wireRoutes?.map(route=>[route.bindingId,route.points])??[]);
  const savedHandles=new Map(options.wireRoutes?.map(route=>[route.bindingId,('handles' in route?route.handles:[]) as readonly DiagramPoint[]])??[]);
  const boxes=nodes.map(node=>({left:node.position.x-clearance,right:node.position.x+(node.width??width)+clearance,top:node.position.y-clearance,bottom:node.position.y+(node.height??height)+clearance}));
  const manualBoxes=nodes.map(node=>({left:node.position.x-20,right:node.position.x+(node.width??width)+20,top:node.position.y-20,bottom:node.position.y+(node.height??height)+20}));
  const blockBoxes=nodes.map(node=>({left:node.position.x,right:node.position.x+(node.width??width),top:node.position.y,bottom:node.position.y+(node.height??height)}));
  const valid=bindings.filter(binding=>byId.has(binding.from.nodeId)&&byId.has(binding.to.nodeId));
  const parallel=new Map<string,string[]>();
  for(const binding of valid){const key=`${binding.from.nodeId}\0${binding.to.nodeId}`,group=parallel.get(key)??[];group.push(binding.id);parallel.set(key,group);}
  for(const group of parallel.values())group.sort();
  function pathFor(points:DiagramPoint[],binding:DeepReadonly<ProfileBinding>,manual=false){
    const cornerRadius=options.style==='orthogonal'?clearCornerRadius(points,requestedRadius,blockBoxes):requestedRadius;
    if(options.style==='orthogonal')return {path:diagramOrthogonalPath(points,cornerRadius),cornerRadius};
    const safe=(curves:Cubic[])=>!curves.some(curve=>blockBoxes.some(box=>cubicCrossesBox(curve,box)));
    // The router's clear channels guide a continuously curved detour. Constrain
    // the control polygon to the assigned lane instead of replacing it with a
    // direct shortcut that silently loses spacing between nearby connections.
    const handles=manual?savedHandles.get(binding.id)??[]:[];
    if(!manual){const flowing=flowingCurves(points);if(safe(flowing))return {path:cubicPath(flowing),cornerRadius};}
    let curves=smoothCurves(points,18,handles);
    for(const limit of [9,4.5,1,.1]){if(safe(curves))break;curves=smoothCurves(points,limit,handles);}
    return {path:cubicPath(curves),cornerRadius};
  }
  const anchors=new Map<string,{start:DiagramPoint;end:DiagramPoint}>();
  for(const binding of valid){const from=byId.get(binding.from.nodeId)!,to=byId.get(binding.to.nodeId)!;anchors.set(binding.id,{start:from.outputAnchors?.[binding.from.portId]??diagramPortAnchor(from.position,'output',from.width??width,from.height??height),end:to.inputAnchors?.[binding.to.portId]??diagramPortAnchor(to.position,'input',to.width??width,to.height??height)});}
  const fan=new Map<string,{departure:DiagramPoint[];arrival:DiagramPoint[]}>();
  const endpointGroups=new Map<string,DeepReadonly<ProfileBinding>[]>();
  for(const binding of valid)for(const direction of ['start','end'] as const){const point=anchors.get(binding.id)![direction],key=`${direction}:${point.x}:${point.y}`,group=endpointGroups.get(key)??[];group.push(binding);endpointGroups.set(key,group);}
  for(const binding of valid){const {start,end}=anchors.get(binding.id)!;fan.set(binding.id,{departure:[start,{x:start.x+clearance,y:start.y}],arrival:[{x:end.x-clearance,y:end.y},end]});}
  for(const [key,group]of endpointGroups){
    if(group.length<2)continue;
    const outgoing=key.startsWith('start:'),opposite=outgoing?'end':'start';
    group.sort((a,b)=>{const ap=anchors.get(a.id)![opposite],bp=anchors.get(b.id)![opposite];return ap.y-bp.y||ap.x-bp.x||a.id.localeCompare(b.id);});
    for(let index=0;index<group.length;index++){
      const binding=group[index],anchor=anchors.get(binding.id)![outgoing?'start':'end'],offset=index-(group.length-1)/2;
      const reach=clearance+Math.min(48,Math.max(0,(group.length-1)/2-Math.abs(offset))*6),x=anchor.x+(outgoing?reach:-reach),shifted={x,y:anchor.y+offset*12};
      const candidate=outgoing?[anchor,{x,y:anchor.y},shifted]:[shifted,{x,y:anchor.y},anchor];
      // Tight saved layouts may not have room for an entire fan. Keep their
      // ordinary departure when a fan would collide with any expanded block.
      if(candidate.slice(1).every((point,i)=>boxes.every((box,nodeIndex)=>(nodes[nodeIndex].id===(outgoing?binding.from.nodeId:binding.to.nodeId)&&i===(outgoing?0:candidate.length-2))||!crosses(candidate[i],point,box))))fan.get(binding.id)![outgoing?'departure':'arrival']=candidate;
    }
  }
  const xs=new Set([...boxes.flatMap(box=>[box.left,box.right]),...[...anchors.values()].flatMap(pair=>[pair.start.x+clearance,pair.end.x-clearance])]);
  const ys=new Set([...boxes.flatMap(box=>[box.top,box.bottom]),...[...anchors.values()].flatMap(pair=>[pair.start.y,pair.end.y])]);
  for(const {departure,arrival}of fan.values())for(const point of [...departure,...arrival]){xs.add(point.x);ys.add(point.y);}
  const corridorXs=[...xs].sort((a,b)=>a-b),corridorYs=[...ys].sort((a,b)=>a-b);
  const segmentCache=new Map<string,boolean>();
  type Interval={start:number;end:number};
  const horizontal=new Map<number,Interval[]>(),vertical=new Map<number,Interval[]>();
  const reservedPaths=new Set<string>();
  const pathKey=(points:readonly DiagramPoint[])=>points.map(point=>`${point.x},${point.y}`).join(':');
  function overlap(a:DiagramPoint,b:DiagramPoint){
    const upright=a.x===b.x,track=(upright?vertical:horizontal).get(upright?a.x:a.y)??[];
    const start=Math.min(upright?a.y:a.x,upright?b.y:b.x),end=Math.max(upright?a.y:a.x,upright?b.y:b.x);
    return track.reduce((total,segment)=>total+Math.max(0,Math.min(end,segment.end)-Math.max(start,segment.start)),0);
  }
  function reserve(points:DiagramPoint[]){
    reservedPaths.add(pathKey(profileWireCorners(points)));
    for(let i=1;i<points.length;i++){
      const a=points[i-1],b=points[i],upright=a.x===b.x,tracks=upright?vertical:horizontal,key=upright?a.x:a.y;
      tracks.set(key,[...(tracks.get(key)??[]),{start:Math.min(upright?a.y:a.x,upright?b.y:b.x),end:Math.max(upright?a.y:a.x,upright?b.y:b.x)}]);
      // Offer the next connection its own parallel lane on either side.
      const coordinates=upright?xs:ys;coordinates.add(key-laneSpacing);coordinates.add(key+laneSpacing);
    }
  }
  function clear(a:DiagramPoint,b:DiagramPoint){const key=`${a.x},${a.y}:${b.x},${b.y}`;let value=segmentCache.get(key);if(value===undefined){value=!boxes.some(box=>crosses(a,b,box));segmentCache.set(key,value);}return value;}
  // Most signal graphs have a clear corridor between their blocks. Checking a
  // bounded set of complete corridors avoids exploring the Cartesian visibility
  // grid once per connection, especially for many inputs on a shared footer.
  // The full search remains the fallback for obstacles that need more bends.
  function corridor(start:DiagramPoint,end:DiagramPoint,rank:number):DiagramPoint[]{
    const middleX=(start.x+end.x)/2,middleY=(start.y+end.y)/2;
    const near=(values:number[],targets:number[],count=12)=>{
      const candidates=new Set<number>();
      for(const target of targets){let low=0,high=values.length;while(low<high){const mid=(low+high)>>1;if(values[mid]<target)low=mid+1;else high=mid;}for(let i=Math.max(0,low-count);i<Math.min(values.length,low+count);i++)candidates.add(values[i]);}
      const distance=(value:number)=>{let result=Infinity;for(const target of targets)result=Math.min(result,Math.abs(value-target));return result;};
      return [...candidates].sort((a,b)=>distance(a)-distance(b)||a-b).slice(0,count);
    };
    const offset=Math.ceil((rank+1)/2)*laneSpacing*(rank%2?-1:1);
    const columns=new Set([middleX,start.x,end.x,middleX+offset,middleX-offset,...near(corridorXs,[start.x,end.x,middleX])]);
    const rows=new Set([middleY,start.y,end.y,start.y+offset,start.y-offset,end.y+offset,end.y-offset,...near(corridorYs,[start.y,end.y,middleY])]);
    let best:DiagramPoint[]=[],bestCost=Infinity;
    function consider(candidate:DiagramPoint[]){
      const points=profileWireCorners(candidate);let cost=0,direction=1;
      for(let i=1;i<points.length;i++){
        const a=points[i-1],b=points[i],axis=a.x===b.x?2:1;
        if(!clear(a,b))return;
        cost+=Math.abs(a.x-b.x)+Math.abs(a.y-b.y)+(direction!==axis?16:0)+(i===points.length-1&&axis===2?16:0)+(axis===2&&direction!==2&&vertical.has(a.x)?256:0)+overlap(a,b)*6;
        if(cost+Math.abs(b.x-end.x)+Math.abs(b.y-end.y)>=bestCost)return;
        const tracks=axis===1?vertical:horizontal,low=Math.min(axis===1?a.x:a.y,axis===1?b.x:b.y),high=Math.max(axis===1?a.x:a.y,axis===1?b.x:b.y);
        let crossings=0;
        for(const [coordinate,segments]of tracks)if(coordinate>low&&coordinate<high&&segments.some(segment=>{const at=axis===1?a.y:a.x;return at>segment.start&&at<segment.end;}))crossings++;
        cost+=crossings*3200;
        direction=axis;
        if(cost>=bestCost)return;
      }
      if(cost<bestCost&&!reservedPaths.has(pathKey(profileWireCorners([{x:start.x-clearance,y:start.y},...points,{x:end.x+clearance,y:end.y}])))){best=points;bestCost=cost;}
    }
    for(const x of columns)consider([start,{x,y:start.y},{x,y:end.y},end]);
    for(const y of rows)consider([start,{x:start.x,y},{x:end.x,y},end]);
    // Separate dense parallel signals after their short shared port leads.
    // Their bend columns can spread too, instead of every wire sharing one
    // vertical trunk even when plenty of space is available between blocks.
    if(end.x-start.x>laneSpacing*2){
      const lead=Math.min((end.x-start.x)/3,Math.max(laneSpacing,Math.abs(offset)));
      for(const y of rows)consider([start,{x:start.x+lead,y:start.y},{x:start.x+lead,y},{x:end.x-lead,y},{x:end.x-lead,y:end.y},end]);
    }
    if(!best.length){
      // An intermediate column can block departure and arrival at different
      // heights. A pair of short clear leads reaches a common horizontal lane
      // without expanding a two-dimensional A* search through every other wire.
      const departures=near(corridorXs,[start.x],12).filter(x=>x>=start.x&&clear(start,{x,y:start.y}));
      const arrivals=near(corridorXs,[end.x],12).filter(x=>x<=end.x&&clear({x,y:end.y},end));
      const detours=[...corridorYs].sort((a,b)=>Math.abs(a-start.y)+Math.abs(a-end.y)-Math.abs(b-start.y)-Math.abs(b-end.y)||a-b);
      for(const y of detours){
        const exits=departures.filter(x=>clear({x,y:start.y},{x,y})),entries=arrivals.filter(x=>clear({x,y},{x,y:end.y}));
        for(const x of exits)for(const toX of entries)consider([start,{x,y:start.y},{x,y},{x:toX,y},{x:toX,y:end.y},end]);
        if(best.length)break;
      }
    }
    return best;
  }
  function route(start:DiagramPoint,end:DiagramPoint):DiagramPoint[]{
    const columns=[...xs].sort((a,b)=>a-b),rows=[...ys].sort((a,b)=>a-b);
    const xIndex=new Map(columns.map((x,index)=>[x,index])),yIndex=new Map(rows.map((y,index)=>[y,index]));
    const point=(index:number)=>({x:columns[index%columns.length],y:rows[Math.floor(index/columns.length)]});
    const source=yIndex.get(start.y)!*columns.length+xIndex.get(start.x)!,target=yIndex.get(end.y)!*columns.length+xIndex.get(end.x)!;
    type Step={key:number;cost:number;estimate:number;remaining:number};
    const heap:Step[]=[],costs=new Map<number,number>(),parent=new Map<number,number>();
    const before=(a:Step,b:Step)=>a.estimate<b.estimate||a.estimate===b.estimate&&a.remaining<b.remaining;
    function push(step:Step){let index=heap.length;heap.push(step);while(index){const up=(index-1)>>1;if(!before(step,heap[up]))break;heap[index]=heap[up];index=up;}heap[index]=step;}
    function pop(){const first=heap[0],last=heap.pop()!;if(heap.length){let index=0;while(index*2+1<heap.length){let child=index*2+1;if(child+1<heap.length&&before(heap[child+1],heap[child]))child++;if(!before(heap[child],last))break;heap[index]=heap[child];index=child;}heap[index]=last;}return first;}
    const distance=(id:number)=>{const p=point(id);return Math.abs(p.x-end.x)+Math.abs(p.y-end.y);};
    costs.set(source*3+1,0);push({key:source*3+1,cost:0,estimate:distance(source),remaining:distance(source)});
    while(heap.length){
      const current=pop();if(current.cost!==costs.get(current.key))continue;
      const id=Math.floor(current.key/3),direction=current.key%3;
      if(id===target){const points:DiagramPoint[]=[];let key:number|undefined=current.key;while(key!==undefined){points.push(point(Math.floor(key/3)));key=parent.get(key);}return points.reverse();}
      const x=id%columns.length,y=Math.floor(id/columns.length),p=point(id);
      for(const [nx,ny,axis]of [[x-1,y,1],[x+1,y,1],[x,y-1,2],[x,y+1,2]]){
        if(nx<0||ny<0||nx>=columns.length||ny>=rows.length)continue;
        const next=ny*columns.length+nx,q=point(next);if(!clear(p,q))continue;
        const crossing=(axis===1?vertical.get(q.x):horizontal.get(q.y))?.some(segment=>{const at=axis===1?q.y:q.x;return at>segment.start&&at<segment.end;});
        // Disjoint lines on one vertical column can still look like a single wire.
        // Prefer a fresh bend column and avoid crossings when a short detour works.
        const reuse=axis===2&&direction!==2&&vertical.has(p.x)?256:0;
        // Sharing the exact footer is intentional. Elsewhere prefer distinct lanes,
        // while allowing a short shared trunk when crowded layouts require one.
        const key=next*3+axis,cost=current.cost+Math.abs(p.x-q.x)+Math.abs(p.y-q.y)+(direction!==axis?16:0)+(next===target&&axis===2?16:0)+reuse+(crossing?3200:0)+overlap(p,q)*6;
        if(cost>=(costs.get(key)??Infinity))continue;
        costs.set(key,cost);parent.set(key,current.key);const remaining=distance(next);push({key,cost,estimate:cost+remaining,remaining});
      }
    }
    return [];
  }
  // Previously planned, unaffected routes reserve their lanes first. This keeps
  // unrelated parts of a diagram still when one block is moved.
  const retainedById=new Map(retained.map(route=>[route.binding.id,route]));
  for(const previous of retained)reserve(previous.points);
  // Short connections claim their compact paths first; longer ones use free lanes.
  const span=(binding:DeepReadonly<ProfileBinding>)=>{const {start,end}=anchors.get(binding.id)!;return Math.abs(start.x-end.x)+Math.abs(start.y-end.y);};
  const routed=[...valid].sort((a,b)=>Number(saved.has(b.id))-Number(saved.has(a.id))||span(a)-span(b)||a.id.localeCompare(b.id)).flatMap(binding=>{
    const {start,end}=anchors.get(binding.id)!;
    const previous=retainedById.get(binding.id);if(previous)return [{...previous,binding}];
    const custom=saved.get(binding.id),authored=custom?profileAnchorWireRoute(custom,start,end,savedHandles.get(binding.id)):[];
    // Manual bends are retained while their channels are clear. A newly moved
    // block may cover a lane; use the safe automatic route until it clears.
    const horizontalLeads=authored.length>=2&&authored[1].y===start.y&&authored[1].x>start.x&&authored.at(-2)!.y===end.y&&authored.at(-2)!.x<end.x;
    const customBlocked=!!custom&&(!horizontalLeads||authored.some((point,index)=>index>0&&manualBoxes.some((box,nodeIndex)=>!(index===1&&nodes[nodeIndex].id===binding.from.nodeId)&&!(index===authored.length-1&&nodes[nodeIndex].id===binding.to.nodeId)&&crosses(authored[index-1],point,box))));
    if(authored.length&&!customBlocked){reserve(authored);return[{binding,points:authored,path:'',cornerRadius:requestedRadius,manual:true,customBlocked:false}];}
    const {departure,arrival}=fan.get(binding.id)!;
    const exit=departure.at(-1)!,entry=arrival[0];
    // Moving one block can bury a port's departure or arrival in another
    // obstacle. Every visibility-grid step would then cross that obstacle;
    // subdividing the grid cannot create a route. Reject it before a futile
    // whole-grid search (and retry) on a graph with many other connections.
    if([exit,entry].some(point=>boxes.some(box=>point.x>box.left&&point.x<box.right&&point.y>box.top&&point.y<box.bottom)))return[];
    const rank=parallel.get(`${binding.from.nodeId}\0${binding.to.nodeId}`)!.indexOf(binding.id);
    let channel=corridor(exit,entry,rank);
    if(!channel.length)channel=route(exit,entry);
    // Older saved layouts can leave less than one normal lane between blocks.
    // Subdivide those tight channels only when the regular tracks are occupied.
    for(let attempt=0;!channel.length&&attempt<2;attempt++){
      for(const coordinates of [xs,ys]){
        const sorted=[...coordinates].sort((a,b)=>a-b);
        for(let i=1;i<sorted.length;i++){const gap=sorted[i]-sorted[i-1];if(gap>2&&gap<=clearance*2)coordinates.add((sorted[i]+sorted[i-1])/2);}
      }
      channel=route(exit,entry);
    }
    const points=[...departure.slice(0,-1),...channel,...arrival.slice(1)];
    // Overlapping manually placed blocks can enclose a port completely. Never draw
    // an invented direct line through those blocks when there is no visible route.
    if(!channel.length)return[];
    const corners=profileWireCorners(points);
    reserve(corners);
    return[{binding,points:corners,path:'',cornerRadius:requestedRadius,manual:false,customBlocked}];
  });
  const clearRoute=(route:DiagramRoute,points:DiagramPoint[])=>points.slice(1).every((point,index)=>{
    const previous=points[index];if(previous.x===point.x&&previous.y===point.y||previous.x!==point.x&&previous.y!==point.y)return false;
    // Keep the direction of every lane, including its short port leads. A
    // nudge cannot create a reversal or hide a newly introduced self-loop.
    const originalA=route.points[index],originalB=route.points[index+1];
    if(Math.sign(point.x-previous.x)!==Math.sign(originalB.x-originalA.x)||Math.sign(point.y-previous.y)!==Math.sign(originalB.y-originalA.y))return false;
    return manualBoxes.every((box,nodeIndex)=>((index===0&&nodes[nodeIndex].id===route.binding.from.nodeId)||(index===points.length-2&&nodes[nodeIndex].id===route.binding.to.nodeId))||!crosses(previous,point,box));
  });
  // Retained geometry is immutable during incremental repair. Its lanes still
  // participate in congestion costs, but only affected routes may be refined.
  const refined=refineDiagramLanes(routed.map(route=>retainedById.has(route.binding.id)?{...route,manual:true}:route),clearRoute);
  const byBinding=new Map(refined.map(route=>{
    const previous=retainedById.get(route.binding.id);if(previous)return[route.binding.id,{...previous,binding:route.binding}];
    return[route.binding.id,{...route,...pathFor(route.points,route.binding,route.manual)}];
  }));
  return bindings.flatMap(binding=>byBinding.has(binding.id)?[byBinding.get(binding.id)!]:[]);
}

/** Keep route geometry stable across inventory, selection and viewport updates.
 * The cache belongs to one canvas and has a bounded history, so switching tabs
 * or undoing a move can reuse geometry without retaining an entire workspace.
 */
export function createDiagramRouter(maxEntries=4):typeof diagramRoutes {
  type EndpointKeys={start:string;end:string};
  type Snapshot={routes:DiagramRoute[];settings:string;nodes:Map<string,{key:string;box:Box}>;bindings:Map<string,string>;endpoints:Map<string,EndpointKeys>};
  const cache=new Map<string,Snapshot>(),limit=Math.max(1,Math.min(16,Math.floor(maxEntries)||1));
  let latest:Snapshot|undefined;
  return (nodes,bindings,width,height,options={})=>{
    const settings=JSON.stringify([width,height,options.style??'smooth',options.cornerRadius??24,options.wireRoutes]);
    const nodeKeys=nodes.map(node=>[node.id,node.position.x,node.position.y,node.width??width,node.height??height,node.inputAnchors,node.outputAnchors]);
    const bindingKeys=bindings.map(binding=>[binding.id,binding.from.nodeId,binding.from.portId,binding.to.nodeId,binding.to.portId]);
    const key=JSON.stringify([settings,nodeKeys,bindingKeys]);
    const previous=cache.get(key);
    if(previous){
      const byId=new Map(bindings.map(binding=>[binding.id,binding]));
      const routes=previous.routes.some(route=>route.binding!==byId.get(route.binding.id))?previous.routes.map(route=>({...route,binding:byId.get(route.binding.id)!})):previous.routes;
      const snapshot={...previous,routes};latest=snapshot;cache.delete(key);cache.set(key,snapshot);return routes;
    }
    const currentNodes=new Map(nodes.map((node,index)=>[node.id,{key:JSON.stringify(nodeKeys[index]),box:{left:node.position.x-40,right:node.position.x+(node.width??width)+40,top:node.position.y-40,bottom:node.position.y+(node.height??height)+40}}]));
    const currentBindings=new Map(bindings.map((binding,index)=>[binding.id,JSON.stringify(bindingKeys[index])]));
    const byNodeId=new Map(nodes.map(node=>[node.id,node]));
    const currentEndpoints=new Map<string,EndpointKeys>();
    for(const binding of bindings){
      const from=byNodeId.get(binding.from.nodeId),to=byNodeId.get(binding.to.nodeId);if(!from||!to)continue;
      const start=from.outputAnchors?.[binding.from.portId]??diagramPortAnchor(from.position,'output',from.width??width,from.height??height),end=to.inputAnchors?.[binding.to.portId]??diagramPortAnchor(to.position,'input',to.width??width,to.height??height);
      currentEndpoints.set(binding.id,{start:`output:${start.x}:${start.y}`,end:`input:${end.x}:${end.y}`});
    }
    let retained:DiagramRoute[]=[];
    if(latest?.settings===settings){
      const changed=new Set([...currentNodes.keys(),...latest.nodes.keys()].filter(id=>currentNodes.get(id)?.key!==latest!.nodes.get(id)?.key));
      const affected=[...changed].flatMap(id=>[currentNodes.get(id)?.box,latest!.nodes.get(id)?.box].filter((box):box is Box=>!!box));
      // A new or reconnected signal changes the ordered fan of every signal
      // using its physical attachments. Retaining the former single centerline
      // would leave it covered by the new fan's wider pointer hit area. Resource
      // ports intentionally share one attachment, even with different port IDs.
      const changedFans=new Set<string>();
      for(const id of new Set([...currentBindings.keys(),...latest.bindings.keys()]))if(currentBindings.get(id)!==latest.bindings.get(id)){
        for(const endpoints of [currentEndpoints.get(id),latest.endpoints.get(id)])if(endpoints){changedFans.add(endpoints.start);changedFans.add(endpoints.end);}
      }
      retained=latest.routes.filter(route=>{
        const endpoints=currentEndpoints.get(route.binding.id);
        return !(route.customBlocked&&changed.size)&&!!endpoints&&!changedFans.has(endpoints.start)&&!changedFans.has(endpoints.end)&&currentBindings.get(route.binding.id)===latest!.bindings.get(route.binding.id)&&!changed.has(route.binding.from.nodeId)&&!changed.has(route.binding.to.nodeId)&&!route.points.slice(1).some((point,index)=>affected.some(box=>crosses(route.points[index],point,box)))&&(options.style==='orthogonal'||!smoothPathCrossesBoxes(route.path,affected));
      });
    }
    const routes=routeDiagram(nodes,bindings,width,height,options,retained);
    const snapshot={routes,settings,nodes:currentNodes,bindings:currentBindings,endpoints:currentEndpoints};latest=snapshot;cache.set(key,snapshot);
    if(cache.size>limit)cache.delete(cache.keys().next().value!);
    return routes;
  };
}
