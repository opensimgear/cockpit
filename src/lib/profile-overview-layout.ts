import {diagramRoutes,diagramRouteQuality,type DiagramNode} from './profile-diagram';
import {canvasOverviewRouteNodes} from './profile-canvas-render';

/** Rank the same separate input anchors and obstacle-avoiding routes that the
 * overview renders, rather than the engine's shared-footer wire plan. */
export function canvasOverviewLayoutScore(nodes:readonly DiagramNode[], bindings:Parameters<typeof diagramRoutes>[1], positions:readonly {nodeId:string;x:number;y:number}[], width:number, height:number):number[] {
  const placed=new Map(positions.map(position=>[position.nodeId,position]));
  const geometry=nodes.map(node=>({...node,position:placed.get(node.id)??node.position}));
  const routes=diagramRoutes(canvasOverviewRouteNodes(geometry,bindings),bindings,width,height,{style:'orthogonal',clearance:36});
  const quality=diagramRouteQuality(routes);
  return [quality.crossings,quality.overlap,quality.length,quality.bends];
}
