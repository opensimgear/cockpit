export const MAX_TRANSFER = 13 * 1024 * 1024;
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
export class TransferDecoder {
  private transfer:{id:string;bytes:number;hash:string;chunks:Uint8Array[];received:number;index:number;started:number}|null=null;
  async accept(value:Record<string,any>):Promise<Record<string,any>|null>{
    if(value.type==='transfer-start'){
      if(this.transfer || !Number.isInteger(value.bytes) || value.bytes<=0 || value.bytes>MAX_TRANSFER || typeof value.id!=='string' || value.id.length>64 || !/^sha256:[a-f0-9]{64}$/.test(value.sha256))throw new Error('Invalid panel transfer');
      this.transfer={id:value.id,bytes:value.bytes,hash:value.sha256,chunks:[],received:0,index:0,started:performance.now()};return null;
    }
    const t=this.transfer;
    if(t && performance.now()-t.started>5000)throw new Error('Panel transfer expired');
    if(value.type==='transfer-chunk'){
      if(!t || value.id!==t.id || value.index!==t.index || typeof value.base64!=='string' || value.base64.length>11000)throw new Error('Invalid panel chunk');
      const b=Uint8Array.from(atob(value.base64),c=>c.charCodeAt(0));
      if(!b.length || t.received+b.length>t.bytes)throw new Error('Panel transfer exceeds budget');
      t.chunks.push(b);t.received+=b.length;t.index++;return null;
    }
    if(value.type==='transfer-end'){
      this.transfer=null;
      if(!t || value.id!==t.id || t.received!==t.bytes)throw new Error('Incomplete panel transfer');
      const bytes=new Uint8Array(t.bytes);let offset=0;for(const c of t.chunks){bytes.set(c,offset);offset+=c.length;}
      const hash='sha256:'+Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),v=>v.toString(16).padStart(2,'0')).join('');
      if(hash!==t.hash)throw new Error('Panel transfer digest differs');
      return JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes));
    }
    if(t)throw new Error('Interleaved panel transfer');return value;
  }
}
/** One bounded RPC at a time; never retry input or retain it across connections. */
export class BrowserTransport {
  private socket:WebSocket|null=null;
  private pending:{resolve:(v:any)=>void;reject:(e:Error)=>void;timer:ReturnType<typeof setTimeout>}|null=null;
  private tail:Promise<unknown>=Promise.resolve();
  private queued=0;
  private decoder=new TransferDecoder();
  onclose:()=>void=()=>{};
  async connect(){
    const socket=new WebSocket(location.origin.replace(/^http/,'ws')+'/socket','opensimgear-companion-v1'); this.socket=socket;
    await new Promise<void>((resolve,reject)=>{
      const timer=setTimeout(()=>{socket.close();reject(new Error('Cockpit connection timed out'));},3000);
      socket.onopen=()=>{clearTimeout(timer);resolve();};
      socket.onerror=()=>{clearTimeout(timer);reject(new Error('Cockpit is unavailable'));};
      socket.onclose=()=>{clearTimeout(timer);this.fail(new Error('Cockpit connection closed'));reject(new Error('Cockpit connection closed'));this.onclose();};
    });
    let messages=Promise.resolve();
    socket.onmessage=event=>{messages=messages.then(async()=>{
      if(typeof event.data!=='string' || new TextEncoder().encode(event.data).length>16384)throw new Error('Invalid Cockpit frame');
      const value=await this.decoder.accept(JSON.parse(event.data)); if(value===null)return;
      const pending=this.pending;if(!pending)throw new Error('Unexpected Cockpit reply');
      clearTimeout(pending.timer);this.pending=null;
      if(value.ok!==true)pending.reject(new Error(String(value.error??'Cockpit rejected the request')));else pending.resolve(value);
    }).catch(e=>{this.fail(e instanceof Error?e:new Error(String(e)));socket.close();});};
  }
  request(frame:Record<string,unknown>):Promise<any>{
    if(++this.queued>32){this.queued--;this.close();return Promise.reject(new Error('Browser request queue exhausted'));}
    const at=performance.now();
    const work=this.tail.then(()=>new Promise((resolve,reject)=>{
      if(!this.socket || this.socket.readyState!==WebSocket.OPEN || performance.now()-at>500){reject(new Error('Connection or queued request expired'));return;}
      const bytes=JSON.stringify(frame);if(new TextEncoder().encode(bytes).length>16384){reject(new Error('Input frame exceeds budget'));return;}
      const timer=setTimeout(()=>{this.fail(new Error('Cockpit reply timed out'));this.socket?.close();},4000);
      this.pending={resolve,reject,timer};this.socket.send(bytes);
    })).finally(()=>{this.queued--;});
    this.tail=work.catch(()=>{});return work;
  }
  private fail(error:Error){if(this.pending){clearTimeout(this.pending.timer);this.pending.reject(error);this.pending=null;}}
  close(){this.fail(new Error('Browser connection stopped'));this.socket?.close();this.socket=null;}
}
