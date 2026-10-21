type Point={x:number;y:number};
type Clear=(points:Point[])=>boolean;
const same=(a:Point,b:Point)=>a.x===b.x&&a.y===b.y;
function corners(points:readonly Point[]):Point[]{
  const result:Point[]=[];
  for(const point of points){
    if(result.length&&same(result.at(-1)!,point))continue;
    result.push({...point});
    while(result.length>=3){
      const[a,b,c]=result.slice(-3);
      if(a.x===b.x&&b.x===c.x&&(b.y-a.y)*(c.y-b.y)>=0||a.y===b.y&&b.y===c.y&&(b.x-a.x)*(c.x-b.x)>=0)result.splice(result.length-2,1);else break;
    }
  }
  return result;
}
const length=(points:readonly Point[])=>points.slice(1).reduce((sum,point,index)=>sum+Math.abs(point.x-points[index].x)+Math.abs(point.y-points[index].y),0);

/** Remove unnecessary automatic legs with bounded, whole-route shortcuts.
 * The validator owns obstacle clearance, physical port leads and simplicity;
 * every accepted candidate has passed it as a complete path. Authored paths
 * and coordinated fans must be excluded by the caller.
 */
export function simplifyDiagramRoute(points:Point[],clear:Clear):Point[];
export function simplifyDiagramRoute(points:readonly Point[],clear:Clear):readonly Point[];
export function simplifyDiagramRoute(points:readonly Point[],clear:Clear):readonly Point[]{
  if(points.length<3)return points;
  let current=points,attempts=0,checks=0;
  for(let pass=0;pass<4&&attempts<2048&&checks<256;pass++){
    let best=current,bestLength=length(current);
    // Long replacements come first, so the small budget removes whole detours
    // before spending work on neighboring corners.
    for(let span=current.length-1;span>=2&&attempts<2048&&checks<256;span--)for(let from=0;from+span<current.length&&attempts<2048&&checks<256;from++){
      const to=from+span,a=current[from],b=current[to];
      for(const bend of [{x:b.x,y:a.y},{x:a.x,y:b.y}]){
        if(attempts>=2048||checks>=256)break;
        attempts++;
        const candidate=corners([...current.slice(0,from+1),bend,...current.slice(to)]),distance=length(candidate);
        if(candidate.length>best.length||candidate.length===best.length&&distance>=bestLength-1e-8)continue;
        checks++;
        if(!clear(candidate))continue;
        best=candidate;bestLength=distance;
        if(best.length===2)return best;
      }
    }
    if(best===current)break;
    current=best;
  }
  return current.length<points.length?current:points;
}
