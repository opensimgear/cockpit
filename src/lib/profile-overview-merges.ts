import type {CanvasRoute} from './profile-canvas-render';
import {diagramOrthogonalPath,diagramSmoothPath,type DiagramNode,type DiagramPoint} from './profile-diagram';
import {routeOverview} from './profile-overview-routing';
import {profileAnchorWireRoute} from './profile-wire-layout';
import type {ProfileWireStyle} from './profiles';

export type OverviewMerge={
  id:string;targetNodeId:string;endpoint:DiagramPoint;junction:DiagramPoint;
  members:readonly CanvasRoute[];path:string;points:DiagramPoint[];
};
type Box={left:number;right:number;top:number;bottom:number};
const same=(a:DiagramPoint,b:DiagramPoint)=>a.x===b.x&&a.y===b.y;
const inside=(p:DiagramPoint,b:Box)=>p.x>b.left&&p.x<b.right&&p.y>b.top&&p.y<b.bottom;
function crosses(a:DiagramPoint,b:DiagramPoint,box:Box):boolean {
  if(a.x===b.x)return a.x>box.left&&a.x<box.right&&Math.max(a.y,b.y)>box.top&&Math.min(a.y,b.y)<box.bottom;
  if(a.y===b.y)return a.y>box.top&&a.y<box.bottom&&Math.max(a.x,b.x)>box.left&&Math.min(a.x,b.x)<box.right;
  return true;
}
function previewPathClear(path:string,boxes:Box[]):boolean {
  const values=path.match(/-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/gi)?.map(Number)??[];
  let a={x:values[0],y:values[1]};
  for(let index=2;index+5<values.length;index+=6){
    const b={x:values[index],y:values[index+1]},c={x:values[index+2],y:values[index+3]},d={x:values[index+4],y:values[index+5]};
    // Conservative control hull test: a preview may retain its unmerged guide
    // when a curve would need a full collision search to prove it safe.
    if(boxes.some(box=>Math.max(a.x,b.x,c.x,d.x)>box.left&&Math.min(a.x,b.x,c.x,d.x)<box.right&&Math.max(a.y,b.y,c.y,d.y)>box.top&&Math.min(a.y,b.y,c.y,d.y)<box.bottom))return false;
    a=d;
  }
  return true;
}

/** Presentation geometry only. Bindings and saved routing guides retain their
 * identities; each returned member ends at a shared, independently drawn stem. */
export function mergeOverviewRoutes(routes:readonly CanvasRoute[],nodes:readonly DiagramNode[],width:number,height:number,style:ProfileWireStyle,cornerRadius:number,preview=false):{routes:CanvasRoute[];merges:OverviewMerge[]} {
  const groups=new Map<string,CanvasRoute[]>(),merges:OverviewMerge[]=[],replacements=new Map<CanvasRoute,CanvasRoute>();
  const boxes=nodes.map(node=>({left:node.position.x,right:node.position.x+(node.width??width),top:node.position.y,bottom:node.position.y+(node.height??height)}));
  for(const route of routes){
    const end=route.points.at(-1);if(!end)continue;
    const key=JSON.stringify([route.binding.to.nodeId,end.x,end.y]);
    const group=groups.get(key)??[];group.push(route);groups.set(key,group);
  }
  for(const [key,members]of groups){
    if(members.length<2)continue;
    const targetNodeId=members[0].binding.to.nodeId;
    if(!nodes.some(node=>node.id===targetNodeId))continue;
    const endpoint={...members[0].points.at(-1)!};
    // Prefer a conspicuous final stem. Only a constrained card arrangement
    // justifies moving the confluence closer to its input circle.
    for(const length of [120,96,80,64,48,32,16,8]){
      const junction={x:endpoint.x-length,y:endpoint.y};
      if(boxes.some(box=>inside(junction,box)||crosses(junction,endpoint,box)))continue;
      let branches:CanvasRoute[];
      if(preview){
        branches=members.map(route=>{
          const points=profileAnchorWireRoute(route.points,route.points[0],junction);
          const path=style==='smooth'?diagramSmoothPath(points,points):diagramOrthogonalPath(points,0);
          return {...route,points,path};
        });
        if(branches.some(route=>route.points.some((p,index)=>index&&boxes.some(box=>crosses(route.points[index-1],p,box)))||(style==='smooth'&&!previewPathClear(route.path,boxes))))continue;
      }else{
        let syntheticId=`overview-merge:${key}`;
        while(nodes.some(node=>node.id===syntheticId))syntheticId+=':';
        const synthetic:DiagramNode={id:syntheticId,position:junction,width:0,height:0,inputAnchors:{input:junction}};
        const bindings=members.map(route=>({...route.binding,to:{nodeId:syntheticId,portId:'input'}}));
        const rerouted=routeOverview([...nodes,synthetic],bindings,width,height,{style,cornerRadius,clearance:24});
        if(rerouted.length!==members.length)continue;
        const byId=new Map(rerouted.map(route=>[route.binding.id,route]));
        branches=members.map(route=>({...route,...byId.get(route.binding.id)!,binding:route.binding}));
      }
      if(branches.some(route=>!same(route.points.at(-1)!,junction)))continue;
      branches.forEach((route,index)=>replacements.set(members[index],route));
      const points=[junction,endpoint];
      merges.push({id:`overview-merge:${key}`,targetNodeId,endpoint,junction,members,path:diagramOrthogonalPath(points,cornerRadius),points});
      break;
    }
  }
  return {routes:routes.map(route=>replacements.get(route)??route),merges};
}

/** One committed geometry snapshot per canvas. Inventory refreshes can replace
 * bindings without paying for another collision search or retaining old data. */
export function createOverviewMerger():typeof mergeOverviewRoutes {
  let previous:{key:string;inputs:readonly CanvasRoute[];result:ReturnType<typeof mergeOverviewRoutes>}|undefined;
  return (routes,nodes,width,height,style,cornerRadius,preview=false)=>{
    if(preview)return mergeOverviewRoutes(routes,nodes,width,height,style,cornerRadius,true);
    const key=JSON.stringify([
      width,height,style,cornerRadius,
      nodes.map(node=>[node.id,node.position.x,node.position.y,node.width??width,node.height??height,node.inputAnchors,node.outputAnchors]),
      routes.map(route=>[route.binding.id,route.binding.from.nodeId,route.binding.from.portId,route.binding.to.nodeId,route.binding.to.portId,route.path,route.points,route.cornerRadius,route.manual,route.customBlocked]),
    ]);
    if(previous?.key===key){
      if(routes.every((route,index)=>route===previous!.inputs[index]))return previous.result;
      const current=new Map(routes.map(route=>[route.binding.id,route]));
      const originals=new Map(previous.inputs.map(route=>[route.binding.id,route]));
      const result={
        routes:previous.result.routes.map(route=>{
          const input=current.get(route.binding.id)!;
          return route===originals.get(route.binding.id)?input:{...input,points:route.points,path:route.path,cornerRadius:route.cornerRadius,manual:route.manual,customBlocked:route.customBlocked};
        }),
        merges:previous.result.merges.map(merge=>({...merge,members:merge.members.map(route=>current.get(route.binding.id)!)})),
      };
      previous={key,inputs:routes,result};return result;
    }
    const result=mergeOverviewRoutes(routes,nodes,width,height,style,cornerRadius);
    previous={key,inputs:routes,result};return result;
  };
}
