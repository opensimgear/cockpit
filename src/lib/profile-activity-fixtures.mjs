export const id='11111111-1111-4111-8111-111111111111',session='22222222-2222-4222-8222-222222222222',hash=`sha256:${'1'.repeat(64)}`;
export const owner=()=>({session,generation:3,profileId:id,profileRevision:7,gameId:'dcs'});
export const node=(id,kind='processing')=>({id,name:id,kind,summary:kind==='processing'?'Boolean NOT':'Optional resource'});
export const edge=(id,from,fromPort,to,toPort)=>({id,from:{nodeId:from,portId:fromPort},to:{nodeId:to,portId:toPort}});
export const graph=()=>({owner:owner(),appliedSnapshot:{profileId:id,revision:7,semanticHash:hash},programHash:hash,capturedFromUs:'100',capturedThroughUs:'100',nodes:[node('source','device'),node('invert'),node('simulator','game'),node('light','companion')],edges:[edge('a','source','button:0','invert','input'),edge('b','invert','output','light','indicator'),edge('telemetry','simulator','warning','light','warning'),edge('command','source','event','simulator','command')]});
export const plain=value=>JSON.parse(JSON.stringify(value));
export function topologyPage(request,topology=graph(),size=request.limit){const items=topology[request.section];return{schemaVersion:1,owner:request.owner,appliedSnapshot:{...topology.appliedSnapshot,profileId:request.owner.profileId,revision:request.owner.profileRevision},programHash:topology.programHash,capturedUs:'100',section:request.section,offset:request.offset,total:items.length,items:items.slice(request.offset,request.offset+size)};}
export const port=(nodeId,portId,direction='output',sample={status:'unknown'})=>({endpoint:{nodeId,portId},direction,contract:{delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:null},sample,observedUs:sample.status==='known'?'101':null});
export const known=value=>({status:'known',value:{kind:'scalar',value}});
export const deferred=()=>{let resolve,reject;const promise=new Promise((yes,no)=>{resolve=yes;reject=no;});return{promise,resolve,reject};};
export function pathSamples(topology){
  const active=new Map(),opened=[];
  const value=(run,nodeId,ports,at='101')=>Object.freeze({
    owner:run,nodeId,appliedSnapshot:topology.appliedSnapshot,programHash:topology.programHash,capturedUs:at,
    ports:Object.freeze(ports.map(port=>Object.freeze({...port,sample:Object.freeze(port.sample)}))),
  });
  const fixture={
    active,opened,value,
    initial(run,nodeId){
      const ports=topology.edges.flatMap(edge=>[
        edge.from.nodeId===nodeId?port(nodeId,edge.from.portId,'output',known(false)):null,
        edge.to.nodeId===nodeId?port(nodeId,edge.to.portId,'input',known(true)):null,
      ]).filter(Boolean);
      return {status:'ready',value:value(run,nodeId,[...new Map(ports.map(port=>[JSON.stringify(port.endpoint),port])).values()])};
    },
    observe(run,nodeId,listener){
      const entry={run,nodeId,listener};
      active.set(nodeId,entry);
      opened.push(entry);
      listener(fixture.initial(run,nodeId));
      return ()=>{if(active.get(nodeId)===entry)active.delete(nodeId);};
    },
    emit(nodeId,state){active.get(nodeId).listener(state);},
  };
  return fixture;
}
