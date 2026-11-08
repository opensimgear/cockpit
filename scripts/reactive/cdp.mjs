import {createRequire} from 'node:module';

export async function browserPage(url, initScript) {
  const require = createRequire(import.meta.url);
  const {chromium} = require(process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright');
  const browser = await chromium.launch({headless:true, channel:process.env.OSG_BROWSER_CHANNEL});
  try {
    const context = await browser.newContext();
    if (initScript) await context.addInitScript({content:initScript});
    const page = await context.newPage();
    await page.goto(url);
    await page.getByRole('navigation', {name:'Main navigation'}).waitFor();
    const session = await context.newCDPSession(page);
    return {
      call: (method, args) => session.send(method, args),
      evaluate: expression => page.evaluate(expression),
      async openPeer() {
        const peer = await context.newPage();
        await peer.goto(url);
        await page.bringToFront();
        return {evaluate: expression => peer.evaluate(expression)};
      },
      listen(listener) {
        const methods = ['Network.requestWillBeSent','Network.responseReceived','Network.loadingFinished','Network.loadingFailed','Fetch.requestPaused','Runtime.bindingCalled'];
        const handlers = methods.map(method => [method, params => listener({method,params})]);
        for (const [method, handler] of handlers) session.on(method, handler);
        return () => handlers.forEach(([method, handler]) => session.off(method, handler));
      },
      close: () => browser.close(),
    };
  } catch (error) {
    await browser.close();
    throw error;
  }
}

// Activation progress has its own restricted WebView at the same origin.
// Always select the main entry, independently of DevTools target ordering.
export function nativeMainTarget(pages) {
  return pages.find(value=>{
    try {
      const url=new URL(value.url);
      const local=(['127.0.0.1','localhost'].includes(url.hostname)&&url.port==='1420')||url.hostname==='tauri.localhost';
      return local&&['/','/index.html'].includes(url.pathname)&&!url.searchParams.has('profile-activation-window');
    } catch { return false; }
  });
}

// Test-only connection to the actual running native WebView, with bounded calls.
export async function nativePage(endpoint=process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222', socketUrl) {
  if (!socketUrl) {
    const pages=await(await fetch(`${endpoint}/json`)).json();
    socketUrl=nativeMainTarget(pages)?.webSocketDebuggerUrl;
  }
  if(!socketUrl)throw new Error('Start the native dev Cockpit before running acceptance.');
  const socket=new WebSocket(socketUrl);
  await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
  const pending=new Map(),listeners=new Set();let next=0;
  socket.onmessage=event=>{
    const value=JSON.parse(event.data),task=pending.get(value.id);
    if(task){pending.delete(value.id);clearTimeout(task.timer);if(value.error)task.reject(Error(JSON.stringify(value.error)));else task.resolve(value.result);}
    else for(const listener of listeners)listener(value);
  };
  const call=(method,params={},timeout=15000)=>new Promise((resolve,reject)=>{
    const id=++next,timer=setTimeout(()=>{pending.delete(id);reject(Error(`${method} timed out.`));},timeout);
    pending.set(id,{resolve,reject,timer});socket.send(JSON.stringify({id,method,params}));
  });
  return {
    call,
    async evaluate(expression,timeout){
      const result=await call('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true},timeout);
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
