import type {DeepReadonly,ProfileEndpoint} from './profiles';
import type {NativeProfileOwner} from './profile-runtime';
import {captureProfileOwner,profileOwnersEqual,type ProfileInspectedPort} from './profile-inspection';
import {profileActivityPath,profileTopologyMatches,type ProfileActivityPath,type ProfileAppliedTopology,type ProfileTopologyClient} from './profile-activity';
import type {ObserveNode} from './profile-inspection-controller';
export type ActivityCapture={nodeId:string;capturedFromUs:string;capturedThroughUs:string;ports:ProfileInspectedPort[]};
export type ProfileActivityState={owner:DeepReadonly<NativeProfileOwner>|null;topology:DeepReadonly<ProfileAppliedTopology>|null;path:DeepReadonly<ProfileActivityPath>|null;edgeId:string;selectedNodeId:string;expanded:boolean;captures:readonly DeepReadonly<ActivityCapture>[];loading:boolean;stale:boolean;error:string};
const initial=():ProfileActivityState=>({owner:null,topology:null,path:null,edgeId:'',selectedNodeId:'',expanded:true,captures:[],loading:false,stale:true,error:''});
const key=(endpoint:DeepReadonly<ProfileEndpoint>)=>JSON.stringify([endpoint.nodeId,endpoint.portId]);
function frozen<T>(value:T):DeepReadonly<T>{const copy:T=JSON.parse(JSON.stringify(value));const freeze=(entry:unknown)=>{if(entry&&typeof entry==='object'){Object.values(entry).forEach(freeze);Object.freeze(entry);}};freeze(copy);return copy as DeepReadonly<T>;}
export function profileActivityPort(state:DeepReadonly<ProfileActivityState>,endpoint:DeepReadonly<ProfileEndpoint>):DeepReadonly<ProfileInspectedPort>|undefined{return state.captures.find(capture=>capture.nodeId===endpoint.nodeId)?.ports.find(port=>key(port.endpoint)===key(endpoint));}
/** Share exact native node samples for the selected path of one applied owner. */
export class ProfileActivityController{
  private value=initial();private epoch=0;private disposed=false;
  private listeners=new Set<(state:DeepReadonly<ProfileActivityState>)=>void>();
  private stops:(()=>void)[]=[];
  constructor(private topologyClient:Pick<ProfileTopologyClient,'load'>,private observe:ObserveNode){}
  get state():DeepReadonly<ProfileActivityState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<ProfileActivityState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(patch:Partial<ProfileActivityState>):void{if(this.disposed)return;this.value={...this.value,...patch};for(const listener of this.listeners)listener(this.state);}
  private stopSamples():void{for(const stop of this.stops.splice(0))stop();}
  clear():void{this.epoch++;this.stopSamples();this.set(initial());}
  async open(run:DeepReadonly<NativeProfileOwner>,selectedNodeId=''):Promise<boolean>{
    if(this.disposed)return false;
    if(this.value.owner&&profileOwnersEqual(run,this.value.owner)&&this.value.loading&&!this.value.topology)return false;
    if(this.value.owner&&profileOwnersEqual(run,this.value.owner)&&this.value.topology)return this.selectBlock(selectedNodeId);
    const owner=captureProfileOwner(run),epoch=++this.epoch;this.stopSamples();this.set({...initial(),owner,selectedNodeId,loading:true});
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
    const topology=this.value.topology;if(!topology||this.disposed)return Promise.resolve(false);this.epoch++;this.stopSamples();
    const selected=topology.nodes.find(node=>node.id===this.value.selectedNodeId);
    const path=edgeId?profileActivityPath(topology,edgeId,expanded):frozen({nodes:selected?[selected]:[],edges:[],expanded,error:selected?'':this.value.selectedNodeId?'The selected editor block is absent from this applied revision. Choose an applied connection.':'This applied graph has no connections.'});
    this.set({edgeId,expanded,path,captures:[],loading:false,stale:true,error:path.error});
    return path.error?Promise.resolve(false):this.refresh();
  }
  refresh():Promise<boolean>{
    const {owner,topology,path}=this.value;
    if(this.disposed||this.value.loading||!owner||!topology||!path||path.error)return Promise.resolve(false);
    const epoch=++this.epoch;
    this.stopSamples();
    const captures=new Map(this.value.captures.map(capture=>[capture.nodeId,capture]));
    const pending=new Set(path.nodes.map(node=>node.id)),errors=new Map<string,string>();
    this.set({loading:pending.size>0,stale:pending.size>0,error:''});
    for(const node of path.nodes){
      if(epoch!==this.epoch)break;
      const required=new Map<string,'input'|'output'>();
      for(const edge of path.edges){
        if(edge.from.nodeId===node.id)required.set(edge.from.portId,'output');
        if(edge.to.nodeId===node.id)required.set(edge.to.portId,'input');
      }
      const stop=this.observe(owner,node.id,state=>{
        if(epoch!==this.epoch)return;
        const sample=state.value,previous=captures.get(node.id);
        let error='';
        if(state.status==='ready'&&sample){
          const declared=new Map(sample.ports.map(port=>[port.endpoint.portId,port.direction]));
          if(sample.nodeId!==node.id||!profileTopologyMatches(topology,sample)
            ||[...required].some(([port,direction])=>declared.get(port)!==direction)
            ||previous&&BigInt(sample.capturedUs)<BigInt(previous.capturedThroughUs)){
            error='The native samples no longer match this applied path.';
          }else{
            captures.set(node.id,Object.freeze({
              nodeId:node.id,capturedFromUs:sample.capturedUs,capturedThroughUs:sample.capturedUs,ports:sample.ports,
            }));
          }
        }else if(state.status!=='connecting'){
          error=state.error??'The native path samples were lost.';
        }
        if(error)errors.set(node.id,error);else errors.delete(node.id);
        if(state.status==='connecting')pending.add(node.id);else pending.delete(node.id);
        this.set({
          captures:Object.freeze(path.nodes.flatMap(node=>{const capture=captures.get(node.id);return capture?[capture]:[];})),
          loading:pending.size>0,stale:pending.size>0||errors.size>0,error:errors.values().next().value??'',
        });
      });
      if(epoch===this.epoch)this.stops.push(stop);else stop();
    }
    return Promise.resolve(epoch===this.epoch);
  }
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
