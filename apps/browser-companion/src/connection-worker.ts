import { SocketTransport } from './socket-transport.ts';
import { BackgroundConnection } from './background-connection.ts';

const worker=globalThis as typeof globalThis&{onconnect:(event:MessageEvent)=>void};
worker.onconnect=event=>{
  const port=event.ports[0],socket=new SocketTransport();
  let closed=false,timer:ReturnType<typeof setInterval>|undefined;
  const send=(message:Record<string,unknown>)=>{if(!closed)port.postMessage(message);};
  const connection=new BackgroundConnection(socket,()=>performance.now(),(reply,id)=>send({type:'feedback',id,reply}));
  const stop=()=>{if(closed)return;send({type:'closed'});closed=true;if(timer)clearInterval(timer);connection.close();port.close();};
  socket.onclose=()=>{if(timer)stop();};
  port.onmessage=async({data})=>{
    if(data.type==='close'){stop();return;}
    if(data.type==='feedback-ack'){try{await connection.acknowledgeFeedback(data.id);}catch{stop();}return;}
    if(data.type==='connect'){
      // The worker's retry timer continues when the document is unfocused.
      while(!closed){
        try{await connection.connect();if(closed)return;timer=setInterval(()=>void connection.tick().catch(stop),250);send({type:'connected'});return;}
        catch{if(!closed)await new Promise(resolve=>setTimeout(resolve,1000));}
      }
    }else if(data.type==='request'){
      try{send({type:'result',id:data.id,reply:await connection.request(data.frame)});}
      catch(error){send({type:'result',id:data.id,error:String(error instanceof Error?error.message:error)});}
    }
  };
  port.start();
};
