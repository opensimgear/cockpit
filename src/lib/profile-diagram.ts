import type { DeepReadonly, ProfileBinding } from './profiles';

type Edge = DeepReadonly<Pick<ProfileBinding, 'from' | 'to'>>;
export type DiagramPoint = { x:number; y:number };
type Node = {id:string;position:DiagramPoint};
type Box = {left:number;right:number;top:number;bottom:number};

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
export function diagramRoutes(nodes:readonly Node[], bindings:readonly DeepReadonly<ProfileBinding>[],width:number,height:number){
  const clearance=20, byId=new Map(nodes.map(node=>[node.id,node]));
  const boxes=nodes.map(node=>({left:node.position.x-clearance,right:node.position.x+width+clearance,top:node.position.y-clearance,bottom:node.position.y+height+clearance}));
  const xs=[...new Set(boxes.flatMap(box=>[box.left,box.right]))].sort((a,b)=>a-b);
  const ys=[...new Set(nodes.flatMap((node,i)=>[boxes[i].top,boxes[i].bottom,node.position.y+height/2]))].sort((a,b)=>a-b);
  const xIndex=new Map(xs.map((x,index)=>[x,index])),yIndex=new Map(ys.map((y,index)=>[y,index]));
  const segmentCache=new Map<string,boolean>();
  const point=(index:number)=>({x:xs[index%xs.length],y:ys[Math.floor(index/xs.length)]});
  function clear(a:number,b:number){const key=a<b?`${a}:${b}`:`${b}:${a}`;let value=segmentCache.get(key);if(value===undefined){value=!boxes.some(box=>crosses(point(a),point(b),box));segmentCache.set(key,value);}return value;}
  function route(start:DiagramPoint,end:DiagramPoint):DiagramPoint[]{
    const source=yIndex.get(start.y)!*xs.length+xIndex.get(start.x)!,target=yIndex.get(end.y)!*xs.length+xIndex.get(end.x)!;
    type Step={key:number;cost:number;estimate:number;remaining:number};
    const heap:Step[]=[],costs=new Map<number,number>(),parent=new Map<number,number>();
    const before=(a:Step,b:Step)=>a.estimate<b.estimate||a.estimate===b.estimate&&a.remaining<b.remaining;
    function push(step:Step){let index=heap.length;heap.push(step);while(index){const up=(index-1)>>1;if(!before(step,heap[up]))break;heap[index]=heap[up];index=up;}heap[index]=step;}
    function pop(){const first=heap[0],last=heap.pop()!;if(heap.length){let index=0;while(index*2+1<heap.length){let child=index*2+1;if(child+1<heap.length&&before(heap[child+1],heap[child]))child++;if(!before(heap[child],last))break;heap[index]=heap[child];index=child;}heap[index]=last;}return first;}
    const distance=(id:number)=>{const p=point(id);return Math.abs(p.x-end.x)+Math.abs(p.y-end.y);};
    costs.set(source*3,0);push({key:source*3,cost:0,estimate:distance(source),remaining:distance(source)});
    while(heap.length){
      const current=pop();if(current.cost!==costs.get(current.key))continue;
      const id=Math.floor(current.key/3),direction=current.key%3;
      if(id===target){const points:DiagramPoint[]=[];let key:number|undefined=current.key;while(key!==undefined){points.push(point(Math.floor(key/3)));key=parent.get(key);}return points.reverse();}
      const x=id%xs.length,y=Math.floor(id/xs.length),p=point(id);
      for(const [nx,ny,axis]of [[x-1,y,1],[x+1,y,1],[x,y-1,2],[x,y+1,2]]){
        if(nx<0||ny<0||nx>=xs.length||ny>=ys.length)continue;
        const next=ny*xs.length+nx;if(!clear(id,next))continue;
        const q=point(next),key=next*3+axis,cost=current.cost+Math.abs(p.x-q.x)+Math.abs(p.y-q.y)+(direction&&direction!==axis?16:0);
        if(cost>=(costs.get(key)??Infinity))continue;
        costs.set(key,cost);parent.set(key,current.key);const remaining=distance(next);push({key,cost,estimate:cost+remaining,remaining});
      }
    }
    return [];
  }
  return bindings.flatMap(binding=>{
    const from=byId.get(binding.from.nodeId),to=byId.get(binding.to.nodeId);if(!from||!to)return[];
    const start={x:from.position.x+width,y:from.position.y+height/2},end={x:to.position.x,y:to.position.y+height/2};
    const points=[start,...route({x:start.x+clearance,y:start.y},{x:end.x-clearance,y:end.y}),end];
    // Overlapping manually placed blocks can enclose a port completely. Never draw
    // an invented direct line through those blocks when there is no visible route.
    if(points.length===2)return[];
    const corners=points.filter((p,index)=>!index||index===points.length-1||!((points[index-1].x===p.x&&p.x===points[index+1].x)||(points[index-1].y===p.y&&p.y===points[index+1].y)));
    let path=`M${corners[0].x},${corners[0].y}`;
    for(let i=1;i<corners.length-1;i++){
      const a=corners[i-1],b=corners[i],c=corners[i+1];
      const radius=Math.min(10,(Math.abs(a.x-b.x)+Math.abs(a.y-b.y))/2,(Math.abs(c.x-b.x)+Math.abs(c.y-b.y))/2);
      const before={x:b.x+Math.sign(a.x-b.x)*radius,y:b.y+Math.sign(a.y-b.y)*radius};
      const after={x:b.x+Math.sign(c.x-b.x)*radius,y:b.y+Math.sign(c.y-b.y)*radius};
      path+=` L${before.x},${before.y} Q${b.x},${b.y} ${after.x},${after.y}`;
    }
    const last=corners.at(-1)!;path+=` L${last.x},${last.y}`;
    return[{binding,points:corners,path}];
  });
}
