// Test-only connection to the actual running native WebView, with bounded calls.
export async function nativePage() {
  const endpoint=process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222';
  const pages=await(await fetch(`${endpoint}/json`)).json();
  const page=pages.find(value=>/127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(value.url));
  if(!page)throw new Error('Start the native dev Cockpit before running acceptance.');
  const socket=new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
  const pending=new Map(),listeners=new Set();let next=0;
  socket.onmessage=event=>{
    const value=JSON.parse(event.data),task=pending.get(value.id);
    if(task){pending.delete(value.id);clearTimeout(task.timer);if(value.error)task.reject(Error(JSON.stringify(value.error)));else task.resolve(value.result);}
    else for(const listener of listeners)listener(value);
  };
  const call=(method,params={})=>new Promise((resolve,reject)=>{
    const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timed out.`));},15000);
    pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));
  });
  return {
    call,
    async evaluate(expression){
      const result=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});
      if(result.exceptionDetails)throw Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    },
    listen(listener){listeners.add(listener);return()=>listeners.delete(listener);},
    close(){for(const task of pending.values()){clearTimeout(task.timer);task.reject(Error('CDP closed.'));}pending.clear();socket.close();},
  };
}

export async function until(read,predicate=Boolean,timeout=15000){
  const deadline=Date.now()+timeout;
  while(Date.now()<deadline){const value=await read();if(predicate(value))return value;await new Promise(resolve=>setTimeout(resolve,50));}
  throw Error('Running Cockpit did not reach the required state.');
}
