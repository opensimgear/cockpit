import {diagramOrthogonalPath,diagramSmoothPath,type DiagramPoint} from './profile-diagram';
import type {ProfileWireStyle} from './profiles';
export type LaneRect={left:number;right:number;top:number;bottom:number};
const point=(rect:LaneRect,side:'left'|'right'):DiagramPoint=>({x:rect[side],y:(rect.top+rect.bottom)/2});
/** Match canvas curves and corner settings while keeping the row's actual input
 * centres, including fan-in and horizontally scrolled transform chains. */
export function laneEndpointWires(sources:readonly LaneRect[],blocks:readonly LaneRect[],destination:LaneRect,track:LaneRect,style:ProfileWireStyle,radius:number):{side:'source'|'destination'|'direct';path:string}[]{
  const path=(from:DiagramPoint,to:DiagramPoint)=>{
    const middle=(from.x+to.x)/2,points=[from,{x:middle,y:from.y},{x:middle,y:to.y},to];
    return style==='orthogonal'?diagramOrthogonalPath(points,radius):diagramSmoothPath(points);
  };
  const end=point(destination,'left');
  if(!blocks.length)return sources.map(source=>({side:'direct',path:path(point(source,'right'),end)}));
  const first=point(blocks[0],'left'),last=point(blocks.at(-1)!,'right');
  first.x=Math.max(track.left,Math.min(track.right,first.x));last.x=Math.max(track.left,Math.min(track.right,last.x));
  return [...sources.map(source=>({side:'source' as const,path:path(point(source,'right'),first)})),{side:'destination',path:path(last,end)}];
}
