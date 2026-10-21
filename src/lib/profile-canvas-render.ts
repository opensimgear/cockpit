import { canvasNodeAnchor, canvasNodeSize, type CanvasNode } from './profile-canvas';
import { diagramOrthogonalPath, diagramSmoothPath, diagramPortAnchor, type DiagramNode, type DiagramPoint, type diagramRoutes } from './profile-diagram';
import { profileAnchorWireRoute, profileWireCorners } from './profile-wire-layout';
import type { ProfileWireStyle } from './profiles';

export type CanvasRoute = ReturnType<typeof diagramRoutes>[number];
export type CanvasRoutes = readonly CanvasRoute[];

/** Keep live inventory and card contents outside the expensive routing dependency. */
export function canvasRouteNodes(nodes:readonly CanvasNode[]):DiagramNode[] {
  return nodes.map(node=>({id:node.id,position:{...node.position},...canvasNodeSize(node),...(node.kind==='processing'?{
    inputAnchors:Object.fromEntries((node.processing?.inputs??[]).map(port=>[port.id,canvasNodeAnchor(node,'input',port.id)])),
    outputAnchors:Object.fromEntries((node.processing?.outputs??[]).map(port=>[port.id,canvasNodeAnchor(node,'output',port.id)]))
  }:{})}));
}

const equal=(a:DiagramPoint,b:DiagramPoint)=>a.x===b.x&&a.y===b.y;
function previewCubicPath(path:string,startDelta:DiagramPoint,endDelta:DiagramPoint):string|null {
  const number=/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi;
  if(!/^MC+$/.test(path.replace(number,'').replace(/[,\s]/g,'')))return null;
  const coordinates=path.match(number)?.map(Number)??[];
  if(coordinates.length<8||(coordinates.length-2)%6)return null;
  if(equal(startDelta,endDelta)){
    let index=0;
    return path.replace(number,value=>String(Number(value)+(index++%2?startDelta.y:startDelta.x)));
  }
  if(coordinates.length!==8)return null;
  // A direct cubic needs only its endpoints and their two horizontal tangents
  // reattached. Keep the committed fit while the full router waits for drop.
  const next=coordinates.map((value,index)=>value+(index<4?(index%2?startDelta.y:startDelta.x):(index%2?endDelta.y:endDelta.x)));
  return `M${next[0]},${next[1]} C${next[2]},${next[3]} ${next[4]},${next[5]} ${next[6]},${next[7]}`;
}
function horizontalLeads(points:readonly DiagramPoint[],start:DiagramPoint,end:DiagramPoint) {
  return points.length>=2&&equal(points[0],start)&&equal(points.at(-1)!,end)&&
    points[1].y===start.y&&points[1].x>start.x&&points.at(-2)!.y===end.y&&points.at(-2)!.x<end.x&&
    points.every((point,index)=>Number.isFinite(point.x)&&Number.isFinite(point.y)&&(!index||(point.x===points[index-1].x)!==(point.y===points[index-1].y)));
}
function previewCorners(points:readonly DiagramPoint[],start:DiagramPoint,end:DiagramPoint):DiagramPoint[] {
  const before=points[0],after=points.at(-1)!;
  const dx=start.x-before.x,dy=start.y-before.y;
  // A group drag carries its internal lanes with both connected cards.
  const anchored=dx===end.x-after.x&&dy===end.y-after.y?
    points.map(point=>({x:point.x+dx,y:point.y+dy})):profileAnchorWireRoute(points,start,end);
  if(horizontalLeads(anchored,start,end))return anchored;
  if(end.x-start.x>40){
    const lane=(start.x+end.x)/2;
    return profileWireCorners([start,{x:lane,y:start.y},{x:lane,y:end.y},end]);
  }
  // A backward edge needs separate departure and arrival leads. Its guide can
  // temporarily cross another card; the full router resolves that on drop.
  const laneY=start.y===end.y?start.y+48:(start.y+end.y)/2;
  return profileWireCorners([start,{x:start.x+20,y:start.y},{x:start.x+20,y:laneY},{x:end.x-20,y:laneY},{x:end.x-20,y:end.y},end]);
}

/** Reattach only incident wires during a drag; never run collision search per frame. */
export function canvasPreviewRoutes(routes:CanvasRoutes,nodes:readonly DiagramNode[],width:number,height:number,style:ProfileWireStyle,cornerRadius=24):CanvasRoutes {
  const byId=new Map(nodes.map(node=>[node.id,node]));
  let changed=false;
  const preview=routes.map(route=>{
    const from=byId.get(route.binding.from.nodeId),to=byId.get(route.binding.to.nodeId);
    if(!from||!to||!route.points.length)return route;
    const start=from.outputAnchors?.[route.binding.from.portId]??diagramPortAnchor(from.position,'output',from.width??width,from.height??height);
    const end=to.inputAnchors?.[route.binding.to.portId]??diagramPortAnchor(to.position,'input',to.width??width,to.height??height);
    if(equal(route.points[0],start)&&equal(route.points.at(-1)!,end))return route;
    changed=true;
    const points=previewCorners(route.points,start,end),radius=Math.min(route.cornerRadius,cornerRadius);
    const startDelta={x:start.x-route.points[0].x,y:start.y-route.points[0].y},endDelta={x:end.x-route.points.at(-1)!.x,y:end.y-route.points.at(-1)!.y};
    // Carry the assigned corridor through the cheap preview too. A direct
    // endpoint curve would collapse separate lanes for the duration of a drag.
    return {...route,points,path:style==='smooth'?(previewCubicPath(route.path,startDelta,endDelta)??diagramSmoothPath(points,route.manual?points:[])):diagramOrthogonalPath(points,radius)};
  });
  return changed?preview:routes;
}
export {bundleDiagramRoutes,type DiagramWireBundle} from './profile-wire-bundles';
