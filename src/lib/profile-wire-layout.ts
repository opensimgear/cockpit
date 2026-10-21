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
/** Move one segment on its perpendicular axis. Physical port endpoints stay fixed. */
export function profileMoveWireSegment(points:readonly WirePoint[],index:number,position:WirePoint):WirePoint[] {
  if(!validPoints(points)||!Number.isInteger(index)||index<0||index>=points.length-1)throw new Error('Choose an existing horizontal or vertical connection segment.');
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
/** Re-anchor a saved path after blocks move, retaining its manually chosen lanes. */
export function profileAnchorWireRoute(points:readonly WirePoint[],start:WirePoint,end:WirePoint):WirePoint[] {
  if(!validPoints(points))return [];
  if(equal(points[0],start)&&equal(points.at(-1)!,end))return points.map(point=>({...point}));
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
export function profileSetWireRoute(value:DeepReadonly<ProfileGraphDraft>,bindingId:string,points?:readonly WirePoint[]|null):ProfileGraphDraft {
  const next=graphDraft(value);
  if(!next.document.bindings.some(binding=>binding.id===bindingId))throw new Error('Choose an existing connection to reshape.');
  if(!points){
    if(next.document.editor.wireRoutes){next.document.editor.wireRoutes=next.document.editor.wireRoutes.filter(route=>route.bindingId!==bindingId);if(!next.document.editor.wireRoutes.length)delete next.document.editor.wireRoutes;}
    return next;
  }
  const corners=profileWireCorners(points);
  if(!validPoints(corners))throw new Error('Use a bounded orthogonal connection path.');
  next.document.editor.wireRoutes=[...(next.document.editor.wireRoutes??[]).filter(route=>route.bindingId!==bindingId),{bindingId,points:corners}];
  const error=profileDocumentError(next.document);if(error)throw new Error(error);return next;
}
