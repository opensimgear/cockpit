import type {DeepReadonly,ProfileEndpoint} from './profiles';
import type {NativeProfileOwner} from './profile-runtime';
import {captureProfileOwner,profileOwnersEqual,type ProfileInspectedPort,type ProfileInspectorClient} from './profile-inspection';
import {profileActivityPath,profileTopologyMatches,type ProfileActivityPath,type ProfileAppliedTopology,type ProfileTopologyClient} from './profile-activity';
export type ActivityCapture={nodeId:string;capturedFromUs:string;capturedThroughUs:string;ports:ProfileInspectedPort[]};
export type ProfileActivityState={owner:DeepReadonly<NativeProfileOwner>|null;topology:DeepReadonly<ProfileAppliedTopology>|null;path:DeepReadonly<ProfileActivityPath>|null;edgeId:string;selectedNodeId:string;expanded:boolean;captures:readonly DeepReadonly<ActivityCapture>[];loading:boolean;stale:boolean;error:string};
const initial=():ProfileActivityState=>({owner:null,topology:null,path:null,edgeId:'',selectedNodeId:'',expanded:true,captures:[],loading:false,stale:true,error:''});
const key=(endpoint:DeepReadonly<ProfileEndpoint>)=>JSON.stringify([endpoint.nodeId,endpoint.portId]);
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
export function profileActivityPort(state:DeepReadonly<ProfileActivityState>,endpoint:DeepReadonly<ProfileEndpoint>):DeepReadonly<ProfileInspectedPort>|undefined{return state.captures.find(capture=>capture.nodeId===endpoint.nodeId)?.ports.find(port=>key(port.endpoint)===key(endpoint));}
/** Polling only captures cached samples. Topology, node pages and late replies remain fenced to one actual owner. */
export class ProfileActivityController{
  private value=initial();private epoch=0;private disposed=false;
  private listeners=new Set<(state:DeepReadonly<ProfileActivityState>)=>void>();
  constructor(private topologyClient:Pick<ProfileTopologyClient,'load'>,private inspector:Pick<ProfileInspectorClient,'inspect'>){}
  get state():DeepReadonly<ProfileActivityState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileActivityState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(patch:Partial<ProfileActivityState>):void{if(this.disposed)return;this.value={...this.value,...patch};for(const listener of this.listeners)listener(this.state);}
  clear():void{this.epoch++;this.set(initial());}
  async open(run:DeepReadonly<NativeProfileOwner>,selectedNodeId=''):Promise<boolean>{
    if(this.disposed)return false;
    if(this.value.owner&&profileOwnersEqual(run,this.value.owner)&&this.value.loading&&!this.value.topology)return false;
    if(this.value.owner&&profileOwnersEqual(run,this.value.owner)&&this.value.topology)return this.selectBlock(selectedNodeId);
    const owner=captureProfileOwner(run),epoch=++this.epoch;this.set({...initial(),owner,selectedNodeId,loading:true});
    try{const topology=await this.topologyClient.load(owner);if(epoch!==this.epoch||this.disposed)return false;if(!profileOwnersEqual(owner,topology.owner))throw new Error('The applied graph owner changed.');this.set({topology,loading:false});return this.selectBlock(selectedNodeId);}
    catch(error){if(epoch===this.epoch)this.set({loading:false,stale:true,error:error instanceof Error?error.message:String(error)});return false;}
  }
  selectBlock(selectedNodeId=''):Promise<boolean>{
    const topology=this.value.topology;if(!topology||this.disposed)return Promise.resolve(false);
    const edge=topology.edges.find(edge=>edge.from.nodeId===selectedNodeId||edge.to.nodeId===selectedNodeId)??(!selectedNodeId?topology.edges[0]:undefined);
    this.set({selectedNodeId});
    return this.selectEdge(edge?.id??'',this.value.expanded);
  }
  selectEdge(edgeId:string,expanded=this.value.expanded):Promise<boolean>{
    const topology=this.value.topology;if(!topology||this.disposed)return Promise.resolve(false);this.epoch++;
    const selected=topology.nodes.find(node=>node.id===this.value.selectedNodeId);
    const path=edgeId?profileActivityPath(topology,edgeId,expanded):frozen({nodes:selected?[selected]:[],edges:[],expanded,error:selected?'':this.value.selectedNodeId?'The selected editor block is absent from this applied revision. Choose an applied connection.':'This applied graph has no connections.'});
    this.set({edgeId,expanded,path,captures:[],loading:false,stale:true,error:path.error});
    return path.error?Promise.resolve(false):this.refresh();
  }
  async refresh():Promise<boolean>{
    const {owner,topology,path}=this.value;if(this.disposed||this.value.loading||!owner||!topology||!path||path.error)return false;
    const epoch=++this.epoch,previous=this.value.captures;this.set({loading:true,stale:true,error:''});
    try{
      const captures:DeepReadonly<ActivityCapture>[]=[];let capturedThrough=topology.capturedThroughUs;
      for(const node of path.nodes){let offset=0,total:number|undefined,first:string|undefined,last:string|undefined;const ports:DeepReadonly<ProfileInspectedPort>[]=[];
        do{const page=await this.inspector.inspect(owner,node.id,offset,64);if(epoch!==this.epoch||this.disposed)return false;
          if(!profileTopologyMatches(topology,page)||total!==undefined&&page.total!==total||BigInt(page.capturedUs)<BigInt(last??capturedThrough))throw new Error('The applied samples no longer match this path.');
          first??=page.capturedUs;last=page.capturedUs;total=page.total;ports.push(...page.ports);offset+=page.ports.length;
        }while(offset<total);
        const old=previous.find(capture=>capture.nodeId===node.id);
        if(old&&BigInt(last!)<BigInt(old.capturedThroughUs)||new Set(ports.map(port=>key(port.endpoint))).size!==ports.length)throw new Error('The applied path capture moved backwards or repeated ports.');
        captures.push(frozen({nodeId:node.id,capturedFromUs:first!,capturedThroughUs:last!,ports:JSON.parse(JSON.stringify(ports)) as ProfileInspectedPort[]}));capturedThrough=last!;
      }
      if(epoch!==this.epoch||this.disposed)return false;this.set({captures:Object.freeze(captures),stale:false});return true;
    }catch(error){if(epoch===this.epoch)this.set({stale:true,error:error instanceof Error?error.message:String(error)});return false;}
    finally{if(epoch===this.epoch)this.set({loading:false});}
  }
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
