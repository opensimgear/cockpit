import { canvasNodeAnchor, canvasNodeSize, type CanvasNode } from './profile-canvas';
import type { DiagramNode, diagramRoutes } from './profile-diagram';

export type CanvasRoute = ReturnType<typeof diagramRoutes>[number];
export type CanvasRoutes = readonly CanvasRoute[];

/** Keep live inventory and card contents outside the expensive routing dependency. */
export function canvasRouteNodes(nodes:readonly CanvasNode[]):DiagramNode[] {
  return nodes.map(node=>({id:node.id,position:{...node.position},...canvasNodeSize(node),...(node.kind==='processing'?{
    inputAnchors:Object.fromEntries((node.processing?.inputs??[]).map(port=>[port.id,canvasNodeAnchor(node,'input',port.id)])),
    outputAnchors:Object.fromEntries((node.processing?.outputs??[]).map(port=>[port.id,canvasNodeAnchor(node,'output',port.id)]))
  }:{})}));
}

/** Every incoming overview arrow ends at the device's visible input circle. */
export function canvasOverviewRouteNodes(nodes:DiagramNode[],bindings:readonly {id:string;from:{nodeId:string};to:{nodeId:string}}[]):DiagramNode[]{
  return nodes.map(node=>{
    const incoming=bindings.filter(binding=>binding.to.nodeId===node.id);
    if(!incoming.length)return node;
    const anchor=canvasNodeAnchor(node,'input');
    return {...node,inputAnchors:{...node.inputAnchors,...Object.fromEntries(incoming.map(binding=>[binding.id,{...anchor}]))}};
  });
}

export {canvasPreviewRoutes} from './profile-route-preview';
export {bundleDiagramRoutes,type DiagramWireBundle} from './profile-wire-bundles';
export {buildDiagramWireTrunks,type DiagramWireTrunk} from './profile-wire-trunks';
export {mergeOverviewRoutes,createOverviewMerger,type OverviewMerge} from './profile-overview-merges';
