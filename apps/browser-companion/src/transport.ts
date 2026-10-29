import { SocketTransport } from "./socket-transport.ts";
const WORKER_STARTUP_TIMEOUT_MS=4000;
export { MAX_TRANSFER, TransferDecoder, SocketTransport } from "./socket-transport.ts";
export interface Credential { token:string; companionId:string; hostId:string }
export function readCredential(storage: Pick<Storage,'getItem'>): Credential | null {
  const raw=storage.getItem('osg-browser-credential-v2')??storage.getItem('osg-browser-credential'); if(!raw)return null;
  const value=JSON.parse(raw);
  if(!value || !/^[A-Za-z0-9_-]{43}$/.test(value.token) || !/^[A-Za-z0-9_-]{1,64}$/.test(value.companionId) || typeof value.hostId!=='string' || value.hostId.length>64)throw new Error('Saved pairing is invalid. Remove this browser companion in Cockpit and pair again.');
  return value;
}
export function saveCredential(storage: Pick<Storage,'setItem'|'getItem'>, value:Credential) {
  const raw=JSON.stringify(value); storage.setItem('osg-browser-credential-v2',raw);
  if(storage.getItem('osg-browser-credential-v2')!==raw)throw new Error('The browser could not save pairing. Allow site storage and pair again.');
}
/** An obsolete tab must never remove a newer pairing saved by another tab. */
export function clearCredential(storage:Pick<Storage,'getItem'|'removeItem'>,token:string){
  for(const key of ['osg-browser-credential-v2','osg-browser-credential']){
    const raw=storage.getItem(key);if(!raw)continue;
    try{if(JSON.parse(raw)?.token===token)storage.removeItem(key);}catch{}
  }
}

/** A shared worker owns the socket and leases while this document stays open. */
export class BrowserTransport {
  private direct:SocketTransport|undefined;
  private worker:SharedWorker|undefined;
  private pending=new Map<number,{resolve:(reply:any)=>void;reject:(error:Error)=>void}>();
  private counter=0;
  private closed=false;
  private connected:{resolve:()=>void;reject:(error:Error)=>void}|undefined;
  private startupTimer:ReturnType<typeof setTimeout>|undefined;
  private workerConnected=false;
  onclose:()=>void=()=>{};
  onreply:(reply:Record<string,any>)=>void=()=>{};
  private connectDirect(){const direct=this.direct=new SocketTransport();direct.onreply=reply=>this.onreply(reply);direct.onclose=()=>{if(!this.closed)this.onclose();};return direct.connect();}
  async connect(){
    if(typeof SharedWorker==='undefined')return this.connectDirect();
    let worker:SharedWorker;
    try{worker=this.worker=new SharedWorker(new URL('./connection-worker.ts',import.meta.url),{type:'module'});}
    catch{return this.connectDirect();}
    worker.port.onmessage=({data})=>{
      if(this.closed||this.worker!==worker)return;
      if(data.type==='connected'){this.workerConnected=true;if(this.startupTimer)clearTimeout(this.startupTimer);this.startupTimer=undefined;this.connected?.resolve();this.connected=undefined;}
      else if(data.type==='closed'){if(!this.workerConnected)this.fallbackDirect(worker);else{this.fail(Error('Cockpit connection closed'));this.onclose();}}
      else if(data.type==='feedback'){try{this.onreply(data.reply);worker.port.postMessage({type:'feedback-ack',id:data.id});}catch(error){this.fail(error instanceof Error?error:Error(String(error)));this.onclose();}}
      else if(data.type==='result'){
        const pending=this.pending.get(data.id);if(!pending)return;this.pending.delete(data.id);
        if(data.error)pending.reject(Error(data.error));
        else{try{this.onreply(data.reply);pending.resolve(data.reply);}catch(error){pending.reject(error instanceof Error?error:Error(String(error)));this.fail(Error('Invalid Cockpit reply'));this.onclose();}}
      }
    };
    worker.onerror=()=>{if(this.closed||this.worker!==worker)return;if(!this.workerConnected)this.fallbackDirect(worker);else{this.fail(Error('Background Cockpit connection failed'));this.onclose();}};
    worker.port.start();
    return new Promise<void>((resolve,reject)=>{
      this.connected={resolve,reject};
      // A stalled worker download may never emit an error. Fall back before any RPC.
      this.startupTimer=setTimeout(()=>this.fallbackDirect(worker),WORKER_STARTUP_TIMEOUT_MS);
      worker.port.postMessage({type:'connect'});
    });
  }
  private fallbackDirect(worker:SharedWorker){
    if(this.closed||this.worker!==worker||this.workerConnected)return;
    if(this.startupTimer)clearTimeout(this.startupTimer);this.startupTimer=undefined;
    this.worker=undefined;
    worker.onerror=null;
    try{worker.port.postMessage({type:'close'});}catch{}
    try{worker.port.close();}catch{}
    const pending=this.connected;
    if(!pending)return;
    this.connectDirect().then(()=>{
      if(this.connected!==pending)return;
      this.connected=undefined;pending.resolve();
    },error=>{
      if(this.connected!==pending)return;
      this.connected=undefined;pending.reject(error instanceof Error?error:Error(String(error)));
    });
  }
  request(frame:Record<string,unknown>):Promise<any>{
    if(this.direct)return this.direct.request(frame);
    if(this.closed||!this.worker)return Promise.reject(Error('Browser connection stopped'));
    if(this.pending.size>=32){this.close();return Promise.reject(Error('Browser request queue exhausted'));}
    return new Promise((resolve,reject)=>{const id=++this.counter;this.pending.set(id,{resolve,reject});this.worker!.port.postMessage({type:'request',id,frame});});
  }
  private fail(error:Error){
    this.connected?.reject(error);this.connected=undefined;
    for(const pending of this.pending.values())pending.reject(error);this.pending.clear();
    this.close();
  }
  close(){
    if(this.closed)return;this.closed=true;if(this.startupTimer)clearTimeout(this.startupTimer);this.startupTimer=undefined;this.direct?.close();
    try{this.worker?.port.postMessage({type:'close'});}catch{}try{this.worker?.port.close();}catch{}this.worker=undefined;
    const error=Error('Browser connection stopped');this.connected?.reject(error);this.connected=undefined;
    for(const pending of this.pending.values())pending.reject(error);this.pending.clear();
  }
}
