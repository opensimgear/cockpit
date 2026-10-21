/** Small, bounded refinement of an already obstacle-avoiding route plan. */
type Point={x:number;y:number};
type LaneRoute={binding:{id:string};points:Point[];manual:boolean};
type Segment={route:number;index:number;a:Point;b:Point;horizontal:boolean};
const same=(a:Point,b:Point)=>a.x===b.x&&a.y===b.y;
function intersection(a:Point,b:Point,c:Point,d:Point):Point|null {
  const ah=a.y===b.y,ch=c.y===d.y;
  if(ah===ch){
    if((ah?a.y:a.x)!==(ch?c.y:c.x))return null;
    const a0=ah?a.x:a.y,a1=ah?b.x:b.y,c0=ah?c.x:c.y,c1=ah?d.x:d.y;
    if(Math.max(Math.min(a0,a1),Math.min(c0,c1))>Math.min(Math.max(a0,a1),Math.max(c0,c1)))return null;
    const at=Math.max(Math.min(a0,a1),Math.min(Math.max(a0,a1),c1));
    return ah?{x:at,y:a.y}:{x:a.x,y:at};
  }
  const horizontal=ah?[a,b]:[c,d],vertical=ah?[c,d]:[a,b],point={x:vertical[0].x,y:horizontal[0].y};
  return point.x>=Math.min(horizontal[0].x,horizontal[1].x)&&point.x<=Math.max(horizontal[0].x,horizontal[1].x)&&point.y>=Math.min(vertical[0].y,vertical[1].y)&&point.y<=Math.max(vertical[0].y,vertical[1].y)?point:null;
}
/** Loop erasure only follows existing segments, so it cannot introduce a block
 * crossing. Authored paths deliberately never pass through this cleanup. */
export function diagramAutomaticCorners(points:readonly Point[]):Point[]{
  let result:Point[]=[];
  for(const point of points){
    if(result.length&&same(result.at(-1)!,point))continue;
    result.push({...point});
    while(result.length>=3){const[a,b,c]=result.slice(-3);if(a.x===b.x&&b.x===c.x||a.y===b.y&&b.y===c.y)result.splice(result.length-2,1);else break;}
  }
  for(let attempt=0;attempt<points.length;attempt++){
    let erased=false;
    outer:for(let end=3;end<result.length;end++)for(let start=0;start<end-2;start++){
      const point=intersection(result[start],result[start+1],result[end-1],result[end]);
      if(!point)continue;
      const suffix=result.slice(end),prefix=result.slice(0,start+1);
      result=diagramAutomaticCorners([...prefix,point,...suffix]);erased=true;break outer;
    }
    if(!erased)break;
  }
  return result;
}
/** Routes must not retrace or cross themselves, even when that would reduce
 * crossings with other wires. Shape validity takes priority over routing cost. */
export function diagramSimpleRoute(points:readonly Point[]):boolean {
  for(let i=1;i<points.length;i++){
    const a=points[i-1],b=points[i];if(same(a,b)||a.x!==b.x&&a.y!==b.y)return false;
    if(i>1){const before=points[i-2];if((a.x-before.x)*(b.x-a.x)+(a.y-before.y)*(b.y-a.y)<0)return false;}
    for(let j=1;j<i-1;j++)if(intersection(points[j-1],points[j],a,b))return false;
  }
  return true;
}
const segments=(routes:readonly LaneRoute[]):Segment[]=>routes.flatMap((route,index)=>route.points.slice(1).map((b,i)=>({route:index,index:i,a:route.points[i],b,horizontal:b.y===route.points[i].y})));
function interaction(a:Segment,b:Segment){
  if(a.route===b.route)return {crossings:0,overlap:0};
  if(a.horizontal===b.horizontal){
    if((a.horizontal?a.a.y:a.a.x)!==(b.horizontal?b.a.y:b.a.x))return {crossings:0,overlap:0};
    const a0=a.horizontal?a.a.x:a.a.y,a1=a.horizontal?a.b.x:a.b.y,b0=b.horizontal?b.a.x:b.a.y,b1=b.horizontal?b.b.x:b.b.y;
    return {crossings:0,overlap:Math.max(0,Math.min(Math.max(a0,a1),Math.max(b0,b1))-Math.max(Math.min(a0,a1),Math.min(b0,b1)))};
  }
  const h=a.horizontal?a:b,v=a.horizontal?b:a;
  return {crossings:Number(v.a.x>Math.min(h.a.x,h.b.x)&&v.a.x<Math.max(h.a.x,h.b.x)&&h.a.y>Math.min(v.a.y,v.b.y)&&h.a.y<Math.max(v.a.y,v.b.y)),overlap:0};
}
/** Includes shared port leads, so before/after measurements use identical rules. */
export function diagramRouteQuality(routes:readonly LaneRoute[]){
  let crossings=0,overlap=0,length=0,bends=0;const all=segments(routes);
  for(let i=0;i<all.length;i++){
    const a=all[i];length+=Math.abs(a.a.x-a.b.x)+Math.abs(a.a.y-a.b.y);
    for(let j=i+1;j<all.length;j++){const score=interaction(a,all[j]);crossings+=score.crossings;overlap+=score.overlap;}
  }
  for(const route of routes)bends+=Math.max(0,route.points.length-2);
  return {crossings,overlap,length,bends};
}

/** Shift coordinated groups together before a short, targeted crossing pass.
 * Only interior lanes move; physical port leads and authored paths are fixed.
 */
export function refineDiagramLanes<T extends LaneRoute>(routes:T[],clear:(route:T,points:Point[])=>boolean,spacing=12,preferParallel:(route:T)=>boolean=()=>false):T[]{
  let all=segments(routes);
  const parallelPriority=routes.map(preferParallel);
  const endpointPairs=routes.map(route=>`${route.points[0].x},${route.points[0].y}>${route.points.at(-1)!.x},${route.points.at(-1)!.y}`);
  type CostLane={route:number;horizontal:boolean;coordinate:number;low:number;high:number};
  const costLane=(segment:Segment):CostLane=>{
    const a=segment.horizontal?segment.a.x:segment.a.y,b=segment.horizontal?segment.b.x:segment.b.y;
    return {route:segment.route,horizontal:segment.horizontal,coordinate:segment.horizontal?segment.a.y:segment.a.x,low:Math.min(a,b),high:Math.max(a,b)};
  };
  let costLanes=all.map(costLane);
  const pairCost=(lane:CostLane,peer:CostLane)=>{
    if(lane.horizontal===peer.horizontal){
      if(lane.coordinate!==peer.coordinate)return 0;
      // Imported parallel corridors may need to trade a few crossings for
      // selectable tracks. Existing coordinated fans keep their original cost.
      const overlapWeight=(parallelPriority[lane.route]||parallelPriority[peer.route])&&endpointPairs[lane.route]===endpointPairs[peer.route]?12:3;
      return Math.max(0,Math.min(lane.high,peer.high)-Math.max(lane.low,peer.low))*overlapWeight;
    }
    return peer.coordinate>lane.low&&peer.coordinate<lane.high&&lane.coordinate>peer.low&&lane.coordinate<peer.high?3200:0;
  };
  const cost=(routeIndex:number,points:Point[],index:number)=>{
    let value=0;
    // Moving one lane only changes itself and its two neighbors. All other
    // route interactions cancel, so evaluating the entire path is unnecessary.
    for(let i=Math.max(1,index);i<=Math.min(points.length-1,index+2);i++){
      const a=points[i-1],b=points[i],horizontal=a.y===b.y;
      const lane=costLane({route:routeIndex,index:i-1,a,b,horizontal});
      value+=(Math.abs(a.x-b.x)+Math.abs(a.y-b.y))*.025;
      for(const peer of costLanes)if(peer.route!==routeIndex)value+=pairCost(lane,peer);
    }
    return value;
  };
  const move=(points:Point[],index:number,coordinate:number)=>{
    const next=points.map(point=>({...point})),a=next[index],b=next[index+1];
    if(a.y===b.y){a.y=coordinate;b.y=coordinate;}else{a.x=coordinate;b.x=coordinate;}
    return next;
  };
  // The order is based on the opposite ends of each channel, rather than the
  // order in which bindings happened to be stored in a profile document.
  const ordered=all.filter(segment=>!routes[segment.route].manual&&segment.index>0&&segment.index<routes[segment.route].points.length-2).sort((a,b)=>{
    const ar=routes[a.route],br=routes[b.route],axis=a.horizontal?'y':'x';
    return Number(a.horizontal)-Number(b.horizontal)||(a.a[axis]-b.a[axis])||((ar.points[0][axis]+ar.points.at(-1)![axis])-(br.points[0][axis]+br.points.at(-1)![axis]))||ar.binding.id.localeCompare(br.binding.id)||a.index-b.index;
  });
  const groups:Segment[][]=[];
  for(const segment of ordered){
    const coordinate=segment.horizontal?segment.a.y:segment.a.x;
    const group=groups.find(group=>group[0].horizontal===segment.horizontal&&Math.abs((segment.horizontal?group[0].a.y:group[0].a.x)-coordinate)<spacing*.75&&group.some(peer=>{
      const a0=segment.horizontal?segment.a.x:segment.a.y,a1=segment.horizontal?segment.b.x:segment.b.y,b0=segment.horizontal?peer.a.x:peer.a.y,b1=segment.horizontal?peer.b.x:peer.b.y;
      return Math.min(Math.max(a0,a1),Math.max(b0,b1))-Math.max(Math.min(a0,a1),Math.min(b0,b1))>spacing;
    }));
    if(group)group.push(segment);else groups.push([segment]);
  }
  let nudges=0;
  // Spend the bounded refinement budget on long shared trunks before short
  // port leads. Otherwise early bend columns exhaust the budget while several
  // long signals remain drawn directly on top of each other.
  const extent=(group:Segment[])=>group.reduce((sum,segment)=>sum+Math.abs(segment.a.x-segment.b.x)+Math.abs(segment.a.y-segment.b.y),0);
  for(const group of groups.filter(group=>group.length>1).sort((a,b)=>extent(b)-extent(a))){
    const localOrder=(segment:Segment)=>{
      const points=routes[segment.route].points,axis=segment.horizontal?'y':'x';
      return (points[Math.max(0,segment.index-1)][axis]+points[Math.min(points.length-1,segment.index+2)][axis])/2;
    };
    group.sort((a,b)=>localOrder(a)-localOrder(b)||routes[a.route].binding.id.localeCompare(routes[b.route].binding.id)||a.index-b.index);
    // Closely packed lanes share the available corridor symmetrically. Try the
    // coordinated position and then nearest alternatives if an obstacle limits it.
    const center=group.reduce((sum,segment)=>sum+(segment.horizontal?segment.a.y:segment.a.x),0)/group.length;
    for(let index=0;index<group.length;index++){
      if(nudges++>=48)break;
      const segment=group[index],route=routes[segment.route];
      const desired=center+(index-(group.length-1)/2)*spacing;
      let best=route.points,bestCost=cost(segment.route,best,segment.index);
      for(const coordinate of [desired,desired-spacing,desired+spacing]){
        const candidate=move(route.points,segment.index,coordinate);
        if(!clear(route,candidate))continue;
        const score=cost(segment.route,candidate,segment.index);
        if(score<bestCost-1e-6){best=candidate;bestCost=score;}
      }
      if(best!==route.points){routes[segment.route]={...route,points:best};all=segments(routes);costLanes=all.map(costLane);}
    }
  }
  // Rip up only the most crossed lanes. A fixed budget bounds release-time
  // work even on profiles with hundreds of signals; drag previews do no search.
  const congested=all.filter(segment=>!routes[segment.route].manual&&segment.index>0&&segment.index<routes[segment.route].points.length-2).map(segment=>{const lane=costLane(segment);return {segment,score:costLanes.reduce((sum,peer)=>sum+(segment.route===peer.route?0:pairCost(lane,peer)),0)};}).filter(entry=>entry.score>0).sort((a,b)=>b.score-a.score||routes[a.segment.route].binding.id.localeCompare(routes[b.segment.route].binding.id)).slice(0,16);
  for(const {segment}of congested){
    const route=routes[segment.route],coordinate=segment.horizontal?route.points[segment.index].y:route.points[segment.index].x;
    let best=route.points,bestCost=cost(segment.route,best,segment.index);
    for(const offset of [-spacing,spacing,-spacing*2,spacing*2]){
      const candidate=move(route.points,segment.index,coordinate+offset);
      if(!clear(route,candidate))continue;
      const score=cost(segment.route,candidate,segment.index);
      if(score<bestCost-1e-6){best=candidate;bestCost=score;}
    }
    if(best!==route.points){routes[segment.route]={...route,points:best};all=segments(routes);costLanes=all.map(costLane);}
  }
  return routes;
}

/** Final parallel-signal separation after ordinary congestion refinement.
 * Keep one stable trunk and move only interior lanes; full-path validation may
 * permit a lane to pass the port's height without permitting a loop or block
 * crossing. Authored and retained routes are fixed obstacles for this pass. */
export function separateDiagramParallels<T extends LaneRoute>(routes:T[],clear:(route:T,points:Point[])=>boolean,spacing=12,groupKey?:(route:T)=>string):T[]{
  const groups=new Map<string,number[]>();
  for(let index=0;index<routes.length;index++){
    const points=routes[index].points;if(points.length<2)continue;
    const start=points[0],end=points.at(-1)!,key=groupKey?groupKey(routes[index]):`${start.x},${start.y}>${end.x},${end.y}`,group=groups.get(key)??[];
    group.push(index);groups.set(key,group);
  }
  const parallelGroups=[...groups.values()].filter(group=>group.length>1);
  if(!parallelGroups.length)return routes;
  const length=(segment:Segment)=>Math.abs(segment.a.x-segment.b.x)+Math.abs(segment.a.y-segment.b.y);
  const routeSegments=(routeIndex:number,interior=true):Segment[]=>{
    const points=routes[routeIndex].points,result:Segment[]=[];
    for(let index=interior?1:0;index<points.length-(interior?2:1);index++){
      const a=points[index],b=points[index+1],segment={route:routeIndex,index,a,b,horizontal:a.y===b.y};
      result.push(segment);
    }
    return result;
  };
  const coordinate=(segment:Segment)=>segment.horizontal?segment.a.y:segment.a.x;
  const overlap=(a:Segment,b:Segment)=>{
    if(a.horizontal!==b.horizontal)return 0;
    const a0=a.horizontal?a.a.x:a.a.y,a1=a.horizontal?a.b.x:a.b.y,b0=b.horizontal?b.a.x:b.a.y,b1=b.horizontal?b.b.x:b.b.y;
    return Math.max(0,Math.min(Math.max(a0,a1),Math.max(b0,b1))-Math.max(Math.min(a0,a1),Math.min(b0,b1)));
  };
  const pathKey=(points:Point[])=>{
    const corners:Point[]=[];
    for(const point of points){
      if(corners.length&&same(corners.at(-1)!,point))continue;
      corners.push(point);
      while(corners.length>=3){const[a,b,c]=corners.slice(-3);if(a.x===b.x&&b.x===c.x||a.y===b.y&&b.y===c.y)corners.splice(corners.length-2,1);else break;}
    }
    return corners.map(point=>`${point.x},${point.y}`).join(':');
  };
  const pathKeys=routes.map(route=>pathKey(route.points));
  let all=segments(routes),checks=0,attempts=0;
  for(const group of parallelGroups){
    group.sort((a,b)=>Number(routes[b].manual)-Number(routes[a].manual)||routes[a].binding.id.localeCompare(routes[b].binding.id));
    // A fixed manual/direct route already supplies the base. Otherwise retain
    // the first automatic route so identical input always produces stable lanes.
    const fixed=group.some(index=>routes[index].manual||!routeSegments(index).length),base=fixed?-1:group[0];
    for(const routeIndex of group){
      if(routes[routeIndex].manual||routeIndex===base)continue;
      // Peers can have their longest legs at opposite ends of a detour. Compare
      // all substantial interior legs, then repair the longest congested leg;
      // comparing only each peer's primary leg can miss parallel exit columns.
      const peerLanes=group.filter(index=>index!==routeIndex).flatMap(index=>{
        const interior=routeSegments(index);return (interior.length?interior:routeSegments(index,false)).filter(segment=>length(segment)>spacing*2);
      });
      const lane=routeSegments(routeIndex).filter(segment=>length(segment)>spacing*2&&peerLanes.some(peer=>overlap(segment,peer)>spacing&&Math.abs(coordinate(segment)-coordinate(peer))<spacing)).sort((a,b)=>length(b)-length(a)||a.index-b.index)[0];
      if(!lane)continue;
      if(attempts++>=64||checks>=256)return routes;
      const route=routes[routeIndex],current=coordinate(lane),peers=peerLanes.map(peer=>({coordinate:coordinate(peer),overlap:overlap(lane,peer)})).filter(peer=>peer.overlap>spacing);
      const deficit=(value:number)=>peers.reduce((sum,peer)=>sum+peer.overlap*Math.max(0,spacing-Math.abs(value-peer.coordinate)),0);
      const originalDeficit=deficit(current);if(originalDeficit<1e-6)continue;
      const coordinates=new Set<number>();
      for(let step=1;step<=Math.min(8,Math.max(2,group.length));step++)for(const sign of [-1,1])coordinates.add(current+sign*spacing*step);
      const peerCoordinates=[...new Set(peers.map(peer=>peer.coordinate))].sort((a,b)=>Math.abs(a-current)-Math.abs(b-current)||a-b);
      for(const value of peerCoordinates.slice(0,12))for(const sign of [-1,1])coordinates.add(value+sign*spacing);
      if(peerCoordinates.length){coordinates.add(Math.min(...peerCoordinates)-spacing);coordinates.add(Math.max(...peerCoordinates)+spacing);}
      const interactionCost=(points:Point[])=>{
        let value=0;
        for(let index=Math.max(1,lane.index);index<=Math.min(points.length-1,lane.index+2);index++){
          const a=points[index-1],b=points[index],candidate={route:routeIndex,index:index-1,a,b,horizontal:a.y===b.y};
          for(const peer of all)if(peer.route!==routeIndex){const score=interaction(candidate,peer);value+=score.crossings*3200+score.overlap*3;}
        }
        return value;
      };
      const candidates=[...coordinates].map(value=>{
        const points=route.points.map(point=>({...point}));
        if(lane.horizontal){points[lane.index].y=value;points[lane.index+1].y=value;}
        else{points[lane.index].x=value;points[lane.index+1].x=value;}
        return {points,deficit:deficit(value),distance:Math.abs(value-current),value,cost:undefined as number|undefined};
      }).filter(candidate=>candidate.deficit<originalDeficit-1e-6).sort((a,b)=>a.deficit-b.deficit||a.distance-b.distance||((a.cost??=interactionCost(a.points))-(b.cost??=interactionCost(b.points)))||a.value-b.value);
      for(const candidate of candidates.slice(0,12)){
        if(checks++>=256)return routes;
        if(!clear(route,candidate.points))continue;
        const key=pathKey(candidate.points);if(pathKeys.some((peer,index)=>index!==routeIndex&&peer===key))continue;
        routes[routeIndex]={...route,points:candidate.points};pathKeys[routeIndex]=key;all=segments(routes);break;
      }
    }
  }
  return routes;
}
