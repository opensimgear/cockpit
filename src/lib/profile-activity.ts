/** Passive metadata from the immutable applied graph, independent of editor drafts. */
import type {AppliedProfileSnapshot,DeepReadonly,ProfileBinding} from './profiles';
import type {NativeProfileOwner} from './profile-runtime';
import {captureProfileOwner,profileOwnersEqual} from './profile-inspection';
export type ProfileTopologyNode={id:string;name:string;kind:'device'|'companion'|'game'|'processing'|'virtual'|'desktop';summary:string};
type Header={schemaVersion:1;owner:NativeProfileOwner;appliedSnapshot:AppliedProfileSnapshot;programHash:string;capturedUs:string;offset:number;total:number};
export type ProfileTopologyPage=Header&({section:'nodes';items:ProfileTopologyNode[]}|{section:'edges';items:ProfileBinding[]});
export type ProfileTopologyRequest={command:'profile_runtime_topology';owner:NativeProfileOwner;section:'nodes'|'edges';offset:number;limit:number};
export type ProfileAppliedTopology={owner:NativeProfileOwner;appliedSnapshot:AppliedProfileSnapshot;programHash:string;capturedFromUs:string;capturedThroughUs:string;nodes:ProfileTopologyNode[];edges:ProfileBinding[]};
export type ProfileActivityPath={nodes:ProfileTopologyNode[];edges:ProfileBinding[];expanded:boolean;error:string};
const object=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const integer=(value:unknown,max:number):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0&&value<=max;
const hash=(value:unknown):value is string=>typeof value==='string'&&/^sha256:[a-f0-9]{64}$/.test(value);
const counter=(value:unknown):value is string=>typeof value==='string'&&/^(0|[1-9][0-9]{0,19})$/.test(value)&&BigInt(value)<=18_446_744_073_709_551_615n;
const nodeId=(value:unknown):value is string=>typeof value==='string'&&/^[A-Za-z][A-Za-z0-9._-]{0,79}$/.test(value);
const bounded=(value:unknown,max:number):value is string=>typeof value==='string'&&value.length>0&&new TextEncoder().encode(value).length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
function endpoint(value:unknown):boolean{return exact(value,['nodeId','portId'])&&nodeId(value.nodeId)&&bounded(value.portId,80);}
function item(value:unknown,section:'nodes'|'edges'):boolean{
  if(section==='edges')return exact(value,['id','from','to'])&&nodeId(value.id)&&endpoint(value.from)&&endpoint(value.to);
  return exact(value,['id','name','kind','summary'])&&nodeId(value.id)&&typeof value.name==='string'&&value.name.trim()===value.name&&[...value.name].length>0&&[...value.name].length<=80&&!/[\u0000-\u001f\u007f-\u009f]/.test(value.name)&&['device','companion','game','processing','virtual','desktop'].includes(String(value.kind))&&bounded(value.summary,256);
}
export function profileTopologyMatches(left:Pick<DeepReadonly<ProfileAppliedTopology>,'owner'|'appliedSnapshot'|'programHash'>,right:Pick<DeepReadonly<ProfileAppliedTopology>,'owner'|'appliedSnapshot'|'programHash'>):boolean{return profileOwnersEqual(left.owner,right.owner)&&left.programHash===right.programHash&&left.appliedSnapshot.profileId===right.appliedSnapshot.profileId&&left.appliedSnapshot.revision===right.appliedSnapshot.revision&&left.appliedSnapshot.semanticHash===right.appliedSnapshot.semanticHash;}
export class ProfileTopologyClient{
  constructor(private send:(request:ProfileTopologyRequest)=>Promise<unknown>){}
  async page(run:DeepReadonly<NativeProfileOwner>,section:'nodes'|'edges',offset=0,limit=64):Promise<DeepReadonly<ProfileTopologyPage>>{
    const owner=captureProfileOwner(run),max=section==='nodes'?336:512;
    if(!['nodes','edges'].includes(section)||!integer(offset,max)||!integer(limit,64)||limit===0)throw new Error('Choose a bounded applied topology page.');
    const result=await this.send({command:'profile_runtime_topology',owner:JSON.parse(JSON.stringify(owner)) as NativeProfileOwner,section,offset,limit});
    if(!exact(result,['schemaVersion','owner','appliedSnapshot','programHash','capturedUs','section','offset','total','items'])||result.schemaVersion!==1||!object(result.owner)||!profileOwnersEqual(owner,result.owner as NativeProfileOwner)||!exact(result.appliedSnapshot,['profileId','revision','semanticHash'])||result.appliedSnapshot.profileId!==owner.profileId||result.appliedSnapshot.revision!==owner.profileRevision||!hash(result.appliedSnapshot.semanticHash)||!hash(result.programHash)||!counter(result.capturedUs)||result.section!==section||result.offset!==offset||!integer(result.total,max)||!Array.isArray(result.items)||result.items.length>limit||offset+result.items.length>result.total||offset<result.total&&result.items.length===0||result.items.some(value=>!item(value,section))||new Set(result.items.map(value=>(value as {id:string}).id)).size!==result.items.length)throw new Error('Native topology does not match this exact applied owner and bounded page.');
    return frozen(result as unknown as ProfileTopologyPage);
  }
  async load(run:DeepReadonly<NativeProfileOwner>):Promise<DeepReadonly<ProfileAppliedTopology>>{
    const owner=captureProfileOwner(run),nodes:ProfileTopologyNode[]=[],edges:ProfileBinding[]=[];let first:DeepReadonly<ProfileTopologyPage>|undefined,last:DeepReadonly<ProfileTopologyPage>|undefined;
    for(const section of ['nodes','edges'] as const){let offset=0,total:number|undefined;do{
      const page=await this.page(owner,section,offset,64);
      if(first&&!profileTopologyMatches(first,page)||last&&BigInt(page.capturedUs)<BigInt(last.capturedUs)||total!==undefined&&page.total!==total)throw new Error('The applied topology changed during its paged capture.');
      first??=page;last=page;total=page.total;
      if(page.section==='nodes')nodes.push(...JSON.parse(JSON.stringify(page.items)) as ProfileTopologyNode[]);else edges.push(...JSON.parse(JSON.stringify(page.items)) as ProfileBinding[]);
      offset+=page.items.length;
    }while(offset<total);}
    if(new Set(nodes.map(node=>node.id)).size!==nodes.length||new Set(edges.map(edge=>edge.id)).size!==edges.length||edges.some(edge=>!nodes.some(node=>node.id===edge.from.nodeId)||!nodes.some(node=>node.id===edge.to.nodeId)))throw new Error('The applied topology contains duplicate identities or missing blocks.');
    return frozen({owner:JSON.parse(JSON.stringify(owner)) as NativeProfileOwner,appliedSnapshot:JSON.parse(JSON.stringify(first!.appliedSnapshot)) as AppliedProfileSnapshot,programHash:first!.programHash,capturedFromUs:first!.capturedUs,capturedThroughUs:last!.capturedUs,nodes:nodes.sort((a,b)=>a.id.localeCompare(b.id)),edges:edges.sort((a,b)=>a.id.localeCompare(b.id))});
  }
}
/** Expand only across processing blocks. A resource's command input never implies a link to its telemetry output. */
export function profileActivityPath(topology:DeepReadonly<ProfileAppliedTopology>,edgeId:string,expand=true):DeepReadonly<ProfileActivityPath>{
  const seed=topology.edges.find(edge=>edge.id===edgeId);if(!seed)return frozen({nodes:[],edges:[],expanded:expand,error:'Choose a connection from the applied graph.'});
  const ids=new Set([seed.id]),queue=[seed],processing=new Set(topology.nodes.filter(node=>node.kind==='processing').map(node=>node.id));
  if(expand)for(let index=0;index<queue.length;index++){const selected=queue[index];for(const edge of topology.edges){if(ids.has(edge.id))continue;
    if([selected.from.nodeId,selected.to.nodeId].some(id=>processing.has(id)&&(edge.from.nodeId===id||edge.to.nodeId===id))){ids.add(edge.id);queue.push(edge);}
  }}
  const edges=topology.edges.filter(edge=>ids.has(edge.id)),nodes=topology.nodes.filter(node=>edges.some(edge=>edge.from.nodeId===node.id||edge.to.nodeId===node.id));
  if(nodes.length>32||edges.length>64)return frozen({nodes:[],edges:[],expanded:expand,error:`This connected route has ${nodes.length} blocks and ${edges.length} connections. Turn off connected-route expansion to inspect this exact connection.`});
  return frozen({nodes:JSON.parse(JSON.stringify(nodes)) as ProfileTopologyNode[],edges:JSON.parse(JSON.stringify(edges)) as ProfileBinding[],expanded:expand,error:''});
}
