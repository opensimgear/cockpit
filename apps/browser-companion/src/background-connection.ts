export interface ConnectionTransport {
  connect():Promise<void>; request(frame:Record<string,any>):Promise<Record<string,any>>; close():void;
  onreply:(reply:Record<string,any>)=>void; onclose:()=>void;
}
type Session={identity:Record<string,any>;heartbeat:bigint;input:bigint;resync:bigint;armed:boolean;panelError?:boolean;operation?:Record<string,any>;fallback:Record<string,boolean|number>;values:Record<string,boolean|number>};
/** The worker is the sole counter writer. Page suspension cannot retain held input. */
export class BackgroundConnection {
  private session:Session|undefined;
  private tail:Promise<unknown>=Promise.resolve();
  private lastWindowAt:number;
  private ticking=false;
  private closed=false;
  private transport:ConnectionTransport;
  private now:()=>number;
  private feedback:(reply:Record<string,any>,id:number)=>void;
  private deliveryId=0;
  private pendingFeedback:number|undefined;
  private feedbackDirty=false;
  private resyncing=false;
  constructor(transport:ConnectionTransport,now=()=>performance.now(),feedback:(reply:Record<string,any>,id:number)=>void=()=>{}){this.transport=transport;this.now=now;this.feedback=feedback;this.lastWindowAt=now();}
  connect(){return this.transport.connect();}
  request(frame:Record<string,any>,fromWindow=true):Promise<Record<string,any>>{
    const queuedAt=this.now();if(fromWindow)this.lastWindowAt=queuedAt;
    const work=this.tail.then(async()=>{
      if(this.closed)throw Error('Browser connection stopped');
      if(fromWindow&&this.now()-queuedAt>500)throw Error('Queued browser request expired');
      const wire={...frame},session=this.session;
      const matches=session&&wire.sessionId===session.identity.sessionId&&wire.generation===session.identity.generation;
      if(matches&&wire.type==='heartbeat')wire.sequence=String(++session.heartbeat);
      if(matches&&wire.type==='input')wire.sequence=String(++session.input);
      if(matches&&wire.type==='feedback-resync')wire.sequence=String(++session.resync);
      const reply=await this.transport.request(wire);
      if(this.closed)throw Error('Browser connection stopped');
      if(wire.type==='open'){
        const identity=Object.fromEntries(['clientId','sessionId','generation','publicationHash'].map(key=>[key,reply[key]]));
        this.resetFeedback();this.session={identity,heartbeat:0n,input:0n,resync:0n,armed:false,fallback:{},values:{}};
      }else if(matches&&wire.type==='baseline'){
        session.armed=true;session.input=0n;session.fallback={...wire.values};session.values={...wire.values};session.operation=wire.operation;
      }else if(matches&&wire.type==='input'){
        for(const change of wire.changes)session.values[change.controlId]=change.value;
      }else if(matches&&wire.type==='heartbeat'){session.panelError=wire.panelError===true;if(wire.operation)session.operation=wire.operation;}
      else if(matches&&wire.type==='close'){this.session=undefined;this.resetFeedback();}
      return reply;
    });
    this.tail=work.catch(()=>{});return work;
  }
  async tick():Promise<void>{
    if(this.closed||this.ticking||!this.session)return;
    this.ticking=true;
    try{
      // Queue this inspection behind page requests as well: only accepted input
      // can change the values used to decide whether a release is necessary.
      await this.tail;
      const session=this.session;if(!session||this.closed)return;
      if(session.armed&&this.now()-this.lastWindowAt>=1000){
        const changes=Object.entries(session.fallback).filter(([id,value])=>session.values[id]!==value).map(([controlId,value])=>({controlId,value}));
        for(let offset=0;offset<changes.length;offset+=64){const reply=await this.request({version:1,type:'input',...session.identity,changes:changes.slice(offset,offset+64)},false);this.deliverFeedback(reply);}
      }
      if(this.closed||session!==this.session)return;
      const reply=await this.request({version:1,type:'heartbeat',...session.identity,...(session.panelError?{panelError:true}:{}),...(session.operation?{operation:session.operation}:{})},false);
      this.deliverFeedback(reply);
    }finally{this.ticking=false;}
  }
  private resetFeedback(){this.pendingFeedback=undefined;this.feedbackDirty=false;this.resyncing=false;}
  private deliverFeedback(reply:Record<string,any>){
    if(!reply.feedback)return;
    if(this.pendingFeedback!==undefined||this.resyncing){this.feedbackDirty=true;return;}
    this.pendingFeedback=++this.deliveryId;this.feedback(reply,this.pendingFeedback);
  }
  async acknowledgeFeedback(id:number):Promise<void>{
    if(this.closed||this.pendingFeedback!==id)return;
    this.pendingFeedback=undefined;
    if(!this.feedbackDirty)return;
    this.feedbackDirty=false;this.resyncing=true;
    const session=this.session;if(!session){this.resetFeedback();return;}
    // Intermediate deltas have advanced the host cursor. Ask for the complete
    // current state instead of forwarding a delta with missing predecessors.
    const reply=await this.request({version:1,type:'feedback-resync',...session.identity},false);
    if(this.closed||this.session!==session)return;
    this.resyncing=false;this.deliverFeedback(reply);
  }
  close(){if(this.closed)return;this.closed=true;this.session=undefined;this.resetFeedback();this.transport.close();}
}
