import { SocketTransport } from './socket-transport.ts';
import { BackgroundConnection } from './background-connection.ts';

const worker=globalThis as typeof globalThis&{onconnect:(event:MessageEvent)=>void};
worker.onconnect=event=>{
  const port=event.ports[0],socket=new SocketTransport();
  let closed=false,timer:ReturnType<typeof setInterval>|undefined;
  let visuals:SocketTransport|undefined;
  const send=(message:Record<string,unknown>)=>{if(!closed)port.postMessage(message);};
  const connection=new BackgroundConnection(socket,()=>performance.now(),(reply,id)=>send({type:'feedback',id,reply}));
  const stop=()=>{if(closed)return;send({type:'closed'});closed=true;if(timer)clearInterval(timer);visuals?.close();connection.close();port.close();};
  socket.onclose=()=>{if(timer)stop();};
  port.onmessage=async({data})=>{
    if(data.type==='close'){stop();return;}
    if(data.type==='page-activity'){
      const now=performance.now(), at=data.sentAt-performance.timeOrigin;
      if(Number.isFinite(at))connection.notePageActivity(Math.min(at,now));
      send({type:'page-activity-ack'});
      return;
    }
    if(data.type==='feedback-ack'){try{await connection.acknowledgeFeedback(data.id);}catch{stop();}return;}
    if(data.type==='connect'){
      // The worker's retry timer continues when the document is unfocused.
      while(!closed){
        try{await connection.connect();if(closed)return;timer=setInterval(()=>void connection.tick().catch(stop),250);send({type:'connected'});return;}
        catch{if(!closed)await new Promise(resolve=>setTimeout(resolve,1000));}
      }
    }else if(data.type==='subscribe-feedback'){
      try{
        const previous=visuals;
        const socket=visuals=new SocketTransport();
        previous?.close();
        socket.onfeedback=reply=>{
          if(visuals!==socket)return;
          if(reply.type==='assignments-event')send({type:'assignments',reply});
          else connection.deliverFeedback(reply);
        };
        socket.onclose=()=>{if(visuals===socket){visuals=undefined;send({type:'feedback-closed'});}};
        await socket.connect();
        send({type:'result',id:data.id,reply:await socket.request(data.frame)});
      }catch(error){send({type:'result',id:data.id,error:String(error instanceof Error?error.message:error)});stop();}
    }else if(data.type==='request'){
      try{send({type:'result',id:data.id,reply:await connection.request(data.frame)});}
      catch(error){send({type:'result',id:data.id,error:String(error instanceof Error?error.message:error)});}
    }
  };
  port.start();
};
