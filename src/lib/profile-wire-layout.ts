import { graphDraft, type ProfileGraphDraft } from './profile-graph';
import { profileDocumentError, type DeepReadonly, type ProfileWireStyle } from './profiles';

export type WirePoint = { x:number; y:number };
const equal=(a:WirePoint,b:WirePoint)=>a.x===b.x&&a.y===b.y;
/** Remove empty segments and redundant corners without altering the drawn path. */
export function profileWireCorners(points:readonly WirePoint[]):WirePoint[] {
  const result:WirePoint[]=[];
  for(const point of points){
    if(result.length&&equal(result.at(-1)!,point))continue;
    result.push({...point});
    while(result.length>=3){
      const [a,b,c]=result.slice(-3);
      // Only remove a corner when it lies between its neighbors. A reversal is
      // intentional geometry and cannot disappear into an unexpected shortcut.
      if((a.x===b.x&&b.x===c.x&&(b.y-a.y)*(c.y-b.y)>=0)||(a.y===b.y&&b.y===c.y&&(b.x-a.x)*(c.x-b.x)>=0))result.splice(result.length-2,1);else break;
    }
  }
  return result;
}
function validPoints(points:readonly WirePoint[]) {
  return points.length>=2&&points.length<=128&&points.every((point,index)=>Number.isFinite(point.x)&&Number.isFinite(point.y)&&Math.abs(point.x)<=1_000_000&&Math.abs(point.y)<=1_000_000&&(!index||(point.x===points[index-1].x)!==(point.y===points[index-1].y)));
}
/** Handles are geometric waypoints on a saved orthogonal route, including straight spans. */
export function profileWireHandleOnRoute(point:WirePoint,points:readonly WirePoint[]):boolean {
  return points.some((a,index)=>{const b=points[index+1];return !!b&&(a.x===b.x?point.x===a.x&&point.y>=Math.min(a.y,b.y)&&point.y<=Math.max(a.y,b.y):point.y===a.y&&point.x>=Math.min(a.x,b.x)&&point.x<=Math.max(a.x,b.x));});
}
function handleCorners(points:readonly WirePoint[],handles:readonly WirePoint[]):WirePoint[] {
  const result:WirePoint[]=[];
  for(const point of points){
    if(result.length&&equal(result.at(-1)!,point))continue;
    result.push({...point});
    while(result.length>=3){const[a,b,c]=result.slice(-3);if(handles.some(handle=>equal(handle,b)))break;if((a.x===b.x&&b.x===c.x&&(b.y-a.y)*(c.y-b.y)>=0)||(a.y===b.y&&b.y===c.y&&(b.x-a.x)*(c.x-b.x)>=0))result.splice(result.length-2,1);else break;}
  }
  return result;
}
function orthogonalJoin(a:WirePoint,b:WirePoint,axis:'x'|'y'):WirePoint[] {
  if(equal(a,b))return [a];
  if(axis==='x'?a.y===b.y:a.x===b.x)return [a,b];
  const middle=a[axis]===b[axis]?a[axis]+20:(a[axis]+b[axis])/2;
  return axis==='x'?[a,{x:middle,y:a.y},{x:middle,y:b.y},b]:[a,{x:a.x,y:middle},{x:b.x,y:middle},b];
}
/** Add a waypoint at the closest straight span without moving existing bends. */
export function profileInsertWireHandle(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,points:readonly WirePoint[],position:WirePoint):ProfileGraphDraft {
  if(!validPoints(points))throw new Error('Choose an existing connection to reshape.');
  const current=value.document.editor.wireRoutes?.find(route=>route.bindingId===bindingId),handles=(current?.handles??[]).filter(handle=>profileWireHandleOnRoute(handle,points));
  if(handles.length>=32)throw new Error('A connection supports up to 32 reroute points.');
  let best:{index:number;point:WirePoint;distance:number}|null=null;
  for(let index=0;index<points.length-1;index++){const a=points[index],b=points[index+1],point=a.x===b.x?{x:a.x,y:Math.max(Math.min(a.y,b.y),Math.min(Math.max(a.y,b.y),position.y))}:{x:Math.max(Math.min(a.x,b.x),Math.min(Math.max(a.x,b.x),position.x)),y:a.y},distance=Math.hypot(point.x-position.x,point.y-position.y);if(!best||distance<best.distance)best={index,point,distance};}
  if(!best||equal(best.point,points[0])||equal(best.point,points.at(-1)!))throw new Error('Add reroute points between the connection endpoints.');
  if(handles.some(handle=>equal(handle,best!.point)))return graphDraft(value);
  const next=points.map(point=>({...point}));next.splice(best.index+1,0,best.point);
  return profileSetWireRoute(value,bindingId,next,[...handles,best.point]);
}
/** Move a waypoint while retaining neighboring bends, other handles, and port tangents. */
export function profileMoveWireHandle(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,handleIndex:number,position:WirePoint):ProfileGraphDraft {
  const route=value.document.editor.wireRoutes?.find(route=>route.bindingId===bindingId),handles=route?.handles?.map(point=>({...point})),handle=handles?.[handleIndex];
  if(!route||!handles||!handle||!Number.isInteger(handleIndex))throw new Error('Choose an existing reroute point.');
  if(!Number.isFinite(position.x)||!Number.isFinite(position.y)||Math.abs(position.x)>1_000_000||Math.abs(position.y)>1_000_000)throw new Error('Keep reroute points within the canvas bounds.');
  const points=route.points.map(point=>({...point}));let index=points.findIndex(point=>equal(point,handle));
  if(index<0){index=points.findIndex((a,i)=>!!points[i+1]&&profileWireHandleOnRoute(handle,[a,points[i+1]]))+1;points.splice(index,0,handle);}
  if(index<=0||index>=points.length-1)throw new Error('Keep reroute points between the connection endpoints.');
  const a=points[index-1],b=points[index+1],incoming=a.y===handle.y?'x':'y',outgoing=b.y===handle.y?'x':'y';
  const replacement=[...orthogonalJoin(a,position,incoming),...orthogonalJoin(position,b,outgoing).slice(1)];
  points.splice(index-1,3,...replacement);handles[handleIndex]={...position};
  return profileSetWireRoute(value,bindingId,points,handles);
}
export function profileRemoveWireHandle(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,handleIndex:number):ProfileGraphDraft {
  const route=value.document.editor.wireRoutes?.find(route=>route.bindingId===bindingId);if(!route?.handles?.[handleIndex])throw new Error('Choose an existing reroute point.');
  return profileSetWireRoute(value,bindingId,route.points,route.handles.filter((_,index)=>index!==handleIndex));
}
/** Move one segment on its perpendicular axis. Physical port endpoints stay fixed. */
export function profileMoveWireSegment(points:readonly WirePoint[],index:number,position:WirePoint):WirePoint[] {
  if(!validPoints(points)||!Number.isInteger(index)||index<0||index>=points.length-1)throw new Error('Choose an existing horizontal or vertical connection segment.');
  const reduced=profileWireCorners(points);
  if(reduced.length!==points.length){const a=points[index],b=points[index+1],span=reduced.findIndex((point,i)=>!!reduced[i+1]&&profileWireHandleOnRoute(a,[point,reduced[i+1]])&&profileWireHandleOnRoute(b,[point,reduced[i+1]]));if(span<0)throw new Error('Choose an existing horizontal or vertical connection segment.');return profileMoveWireSegment(reduced,span,position);}
  const next=points.map(point=>({...point})),a=next[index],b=next[index+1],horizontal=a.y===b.y;
  const coordinate=horizontal?position.y:position.x;
  if(!Number.isFinite(coordinate)||Math.abs(coordinate)>1_000_000)throw new Error('Keep connection segments within the canvas bounds.');
  if(coordinate===(horizontal?a.y:a.x))return next;
  // A direct connection has no free bends yet. Add departure and arrival leads
  // so dragging its only horizontal segment creates a useful movable dogleg.
  if(next.length===2){
    if(!horizontal)throw new Error('Connection endpoints require horizontal departure and arrival.');
    const reach=Math.min(20,Math.max(1,Math.abs(b.x-a.x)/3));
    const corners=profileWireCorners([a,{x:a.x+reach,y:a.y},{x:a.x+reach,y:coordinate},{x:b.x-reach,y:coordinate},{x:b.x-reach,y:b.y},b]);
    if(!validPoints(corners))throw new Error('This segment move exceeds the bounded orthogonal path.');
    return corners;
  }
  if(index===0){
    if(!horizontal)throw new Error('Connection endpoints require horizontal departure.');
    const lead={x:a.x+Math.min(20,Math.max(1,Math.abs(b.x-a.x)/2)),y:a.y};
    next.splice(1,1,lead,{x:lead.x,y:coordinate},{x:b.x,y:coordinate});
  }else if(index===next.length-2){
    if(!horizontal)throw new Error('Connection endpoints require horizontal arrival.');
    const lead={x:b.x-Math.min(20,Math.max(1,Math.abs(b.x-a.x)/2)),y:b.y};
    next.splice(index,1,{x:a.x,y:coordinate},{x:lead.x,y:coordinate},lead);
  }else if(horizontal){a.y=coordinate;b.y=coordinate;}else{a.x=coordinate;b.x=coordinate;}
  const corners=profileWireCorners(next);
  if(!validPoints(corners))throw new Error('This segment move exceeds the bounded orthogonal path.');
  return corners;
}
/** Segment edits retain unrelated waypoints and translate those on the moved span. */
export function profileReshapeWireSegment(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,points:readonly WirePoint[],index:number,position:WirePoint):ProfileGraphDraft {
  const moved=profileMoveWireSegment(points,index,position),reduced=profileWireCorners(points),a=points[index],b=points[index+1],span=reduced.findIndex((point,i)=>!!reduced[i+1]&&profileWireHandleOnRoute(a,[point,reduced[i+1]])&&profileWireHandleOnRoute(b,[point,reduced[i+1]]));
  const start=reduced[span],end=reduced[span+1],handles:WirePoint[]=[];
  for(const handle of value.document.editor.wireRoutes?.find(route=>route.bindingId===bindingId)?.handles??[]){const next=profileWireHandleOnRoute(handle,[start,end])?(start.y===end.y?{x:handle.x,y:position.y}:{x:position.x,y:handle.y}):{...handle};if(profileWireHandleOnRoute(next,moved)&&!handles.some(old=>equal(old,next)))handles.push(next);}
  return profileSetWireRoute(value,bindingId,moved,handles);
}
/** Re-anchor a saved path after blocks move, retaining its manually chosen lanes. */
export function profileAnchorWireRoute(points:readonly WirePoint[],start:WirePoint,end:WirePoint,handles:readonly WirePoint[]=[]):WirePoint[] {
  if(!validPoints(points))return [];
  if(equal(points[0],start)&&equal(points.at(-1)!,end))return points.map(point=>({...point}));
  if(handles.length){
    const expanded:WirePoint[]=[];
    for(let index=0;index<points.length-1;index++){const a=points[index],b=points[index+1],span=handles.filter(handle=>!equal(handle,a)&&!equal(handle,b)&&profileWireHandleOnRoute(handle,[a,b])).sort((left,right)=>Math.hypot(left.x-a.x,left.y-a.y)-Math.hypot(right.x-a.x,right.y-a.y));expanded.push({...a},...span.map(point=>({...point})));}expanded.push({...points.at(-1)!});
    const inner=expanded.slice(1,-1);if(!inner.length)return [];
    const anchored=handleCorners([...orthogonalJoin(start,inner[0],'x'),...inner.slice(1),...orthogonalJoin(inner.at(-1)!,end,'x').slice(1)],handles);
    return validPoints(anchored)?anchored:[];
  }
  if(points.length===2){
    if(start.y===end.y&&end.x>start.x)return [start,end].map(point=>({...point}));
    const lane=Math.max(start.x+20,(start.x+end.x)/2);
    return profileWireCorners([start,{x:lane,y:start.y},{x:lane,y:end.y},end]);
  }
  const inner=points.slice(1,-1).map(point=>({...point}));
  // A single shared bend cannot serve two different endpoint heights. The
  // complete saved router normally has either zero or at least two bends.
  if(inner.length===1)return [];
  inner[0].y=start.y;inner.at(-1)!.y=end.y;
  // Preserve established bend columns when possible. If a block overtakes one,
  // add a short horizontal lead and reconnect orthogonally to the saved lane.
  const departure=inner[0].x>start.x?[]:[{x:start.x+20,y:start.y},{x:start.x+20,y:points[1].y},{x:inner[0].x,y:points[1].y}];
  if(departure.length)inner[0].y=points[1].y;
  const arrival=inner.at(-1)!.x<end.x?[]:[{x:inner.at(-1)!.x,y:points.at(-2)!.y},{x:end.x-20,y:points.at(-2)!.y},{x:end.x-20,y:end.y}];
  if(arrival.length)inner.at(-1)!.y=points.at(-2)!.y;
  const anchored=profileWireCorners([start,...departure,...inner,...arrival,end]);
  return validPoints(anchored)?anchored:[];
}
export function profileSetWireStyle(value:DeepReadonly<ProfileGraphDraft>,style:ProfileWireStyle):ProfileGraphDraft {
  if(style!=='smooth'&&style!=='orthogonal')throw new Error('Choose smooth or orthogonal arrows.');
  const next=graphDraft(value);next.document.editor.arrowStyle=style;return next;
}
export function profileSetWireRoute(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,points?:readonly WirePoint[]|null,handles:readonly WirePoint[]=[]):ProfileGraphDraft {
  const next=graphDraft(value);
  if(!next.document.bindings.some(binding=>binding.id===bindingId))throw new Error('Choose an existing connection to reshape.');
  if(!points){
    if(next.document.editor.wireRoutes){next.document.editor.wireRoutes=next.document.editor.wireRoutes.filter(route=>route.bindingId!==bindingId);if(!next.document.editor.wireRoutes.length)delete next.document.editor.wireRoutes;}
    return next;
  }
  const corners=handleCorners(points,handles);
  if(!validPoints(corners))throw new Error('Use a bounded orthogonal connection path.');
  next.document.editor.wireRoutes=[...(next.document.editor.wireRoutes??[]).filter(route=>route.bindingId!==bindingId),{bindingId,points:corners,...(handles.length?{handles:handles.map(point=>({...point}))}:{})}];
  const error=profileDocumentError(next.document);if(error)throw new Error(error);return next;
}
