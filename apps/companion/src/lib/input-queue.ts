export interface QueuedChange { controlId: string; value: number | boolean }
interface Job { epoch:number; changes:QueuedChange[]; generation:number; at:number }
/** Serialize IPC acceptance so independent async native workers cannot reorder digital edges. */
export function createOrderedInputQueue(send:(epoch:number,changes:QueuedChange[])=>Promise<unknown>,onError:(reason:string)=>void,options:{clock?:()=>number;capacity?:number;maxAgeMs?:number;maxRate?:number}={}) {
  const clock=options.clock??(()=>performance.now()),capacity=options.capacity??256,maxAge=options.maxAgeMs??250,maxRate=options.maxRate??120;
  let generation=0,enabled=false,draining=false,inFlight=false,inFlightChanges=0;
  const pending:Job[]=[],recent:number[]=[];
  function stop(){generation++;enabled=false;pending.length=0;recent.length=0;}
  function reject(reason:string){stop();onError(reason);return false;}
  async function drain(){
    if(draining)return;draining=true;
    try{while(enabled&&pending.length){
      const job=pending.shift()!;if(job.generation!==generation)continue;
      const age=clock()-job.at;if(age<0||age>maxAge){reject('Input backlog expired; reconnect and Start with a fresh baseline');break;}
      inFlight=true;inFlightChanges=job.changes.length;
      try{await send(job.epoch,job.changes);}catch(error){if(job.generation===generation){reject(error instanceof Error?error.message:String(error));break;}}
      finally{inFlight=false;inFlightChanges=0;}
    }}finally{draining=false;if(enabled&&pending.length)void drain();}
  }
  function start(){stop();enabled=true;}
  function enqueue(epoch:number,changes:QueuedChange[]){
    if(!enabled)return false;
    if(!Number.isSafeInteger(epoch)||epoch<0||!changes.length||changes.length>128||changes.some(change=>!change.controlId||!(typeof change.value==='boolean'||typeof change.value==='number'&&Number.isFinite(change.value))))return reject('Invalid native input batch');
    const count=Math.ceil(changes.length/64),now=clock();while(recent.length&&now-recent[0]>=1000)recent.shift();
    if(pending.reduce((total,job)=>total+job.changes.length,0)+inFlightChanges+changes.length>capacity)return reject('Input queue capacity exceeded; session stopped');
    if(recent.length+count>maxRate)return reject('Input rate exceeds 120 batches per second; session stopped');
    for(let index=0;index<changes.length;index+=64){pending.push({epoch,changes:changes.slice(index,index+64).map(change=>({...change})),generation,at:now});recent.push(now);}
    void drain();return true;
  }
  return{start,stop,enqueue,snapshot:()=>({enabled,pending:pending.length,queuedChanges:pending.reduce((total,job)=>total+job.changes.length,0)+inFlightChanges,inFlight,generation})};
}
