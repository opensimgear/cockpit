/** Small, bounded refinement of an already obstacle-avoiding route plan. */
type Point={x:number;y:number};
type LaneRoute={binding:{id:string};points:Point[];manual:boolean};
type Segment={route:number;index:number;a:Point;b:Point;horizontal:boolean};
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
export function refineDiagramLanes<T extends LaneRoute>(routes:T[],clear:(route:T,points:Point[])=>boolean,spacing=12):T[]{
  let all=segments(routes);
  const pairCost=(a:Point,b:Point,horizontal:boolean,peer:Segment)=>{
    if(horizontal===peer.horizontal){
      if((horizontal?a.y:a.x)!==(horizontal?peer.a.y:peer.a.x))return 0;
      const a0=horizontal?a.x:a.y,a1=horizontal?b.x:b.y,b0=horizontal?peer.a.x:peer.a.y,b1=horizontal?peer.b.x:peer.b.y;
      return Math.max(0,Math.min(Math.max(a0,a1),Math.max(b0,b1))-Math.max(Math.min(a0,a1),Math.min(b0,b1)))*3;
    }
    const x=horizontal?peer.a.x:a.x,y=horizontal?a.y:peer.a.y,left=horizontal?Math.min(a.x,b.x):Math.min(peer.a.x,peer.b.x),right=horizontal?Math.max(a.x,b.x):Math.max(peer.a.x,peer.b.x),top=horizontal?Math.min(peer.a.y,peer.b.y):Math.min(a.y,b.y),bottom=horizontal?Math.max(peer.a.y,peer.b.y):Math.max(a.y,b.y);
    return x>left&&x<right&&y>top&&y<bottom?3200:0;
  };
  const cost=(routeIndex:number,points:Point[],index:number)=>{
    let value=0;
    // Moving one lane only changes itself and its two neighbors. All other
    // route interactions cancel, so evaluating the entire path is unnecessary.
    for(let i=Math.max(1,index);i<=Math.min(points.length-1,index+2);i++){
      const a=points[i-1],b=points[i],horizontal=a.y===b.y;
      value+=(Math.abs(a.x-b.x)+Math.abs(a.y-b.y))*.025;
      for(const peer of all)if(peer.route!==routeIndex)value+=pairCost(a,b,horizontal,peer);
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
  for(const group of groups.filter(group=>group.length>1)){
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
      if(best!==route.points){routes[segment.route]={...route,points:best};all=segments(routes);}
    }
  }
  // Rip up only the most crossed lanes. A fixed budget bounds release-time
  // work even on profiles with hundreds of signals; drag previews do no search.
  const congested=all.filter(segment=>!routes[segment.route].manual&&segment.index>0&&segment.index<routes[segment.route].points.length-2).map(segment=>({segment,score:all.reduce((sum,peer)=>sum+(segment.route===peer.route?0:pairCost(segment.a,segment.b,segment.horizontal,peer)),0)})).filter(entry=>entry.score>0).sort((a,b)=>b.score-a.score||routes[a.segment.route].binding.id.localeCompare(routes[b.segment.route].binding.id)).slice(0,16);
  for(const {segment}of congested){
    const route=routes[segment.route],coordinate=segment.horizontal?route.points[segment.index].y:route.points[segment.index].x;
    let best=route.points,bestCost=cost(segment.route,best,segment.index);
    for(const offset of [-spacing,spacing,-spacing*2,spacing*2]){
      const candidate=move(route.points,segment.index,coordinate+offset);
      if(!clear(route,candidate))continue;
      const score=cost(segment.route,candidate,segment.index);
      if(score<bestCost-1e-6){best=candidate;bestCost=score;}
    }
    if(best!==route.points){routes[segment.route]={...route,points:best};all=segments(routes);}
  }
  return routes;
}
