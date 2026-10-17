import test from 'node:test';
import assert from 'node:assert/strict';
import { readCredential, saveCredential, clearCredential, TransferDecoder, BrowserTransport, MAX_TRANSFER } from './transport.ts';
test('pairing survives reload; invalid credentials and failed storage fail closed',()=>{
  const values=new Map();const storage={getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v)};
  const credential={token:'a'.repeat(43),companionId:'browser-one',hostId:'host-one'};
  assert.equal(readCredential(storage),null);saveCredential(storage,credential);
  assert.deepEqual(readCredential(storage),credential);
  assert.throws(()=>saveCredential({setItem(){},getItem(){return null}},credential));
  values.set('osg-browser-credential-v2',JSON.stringify({...credential,token:'bad'}));
  assert.throws(()=>readCredential(storage));
});
test('bounded publication transfer validates ordered chunks and digest',async()=>{
  const payload=JSON.stringify({ok:true,type:'session-open',content:'x'.repeat(20000)});
  const bytes=new TextEncoder().encode(payload),hash='sha256:'+Buffer.from(await crypto.subtle.digest('SHA-256',bytes)).toString('hex');
  const decoder=new TransferDecoder();
  assert.equal(await decoder.accept({type:'transfer-start',id:'test',bytes:bytes.length,sha256:hash}),null);
  for(let i=0;i<bytes.length;i+=8192)assert.equal(await decoder.accept({type:'transfer-chunk',id:'test',index:i/8192,base64:Buffer.from(bytes.subarray(i,i+8192)).toString('base64')}),null);
  assert.deepEqual(await decoder.accept({type:'transfer-end',id:'test'}),JSON.parse(payload));
  for(const frame of [{type:'transfer-start',id:'test',bytes:MAX_TRANSFER+1,sha256:hash},{type:'transfer-chunk',id:'test',index:0,base64:'e30='},{type:'transfer-end',id:'test'}])await assert.rejects(new TransferDecoder().accept(frame));
  const missing=new TransferDecoder();await missing.accept({type:'transfer-start',id:'test',bytes:2,sha256:hash});
  await assert.rejects(missing.accept({type:'transfer-end',id:'test'}));
  const outOfOrder=new TransferDecoder();await outOfOrder.accept({type:'transfer-start',id:'test',bytes:2,sha256:hash});
  await assert.rejects(outOfOrder.accept({type:'transfer-chunk',id:'test',index:1,base64:'e30='}));
  const corrupt=new TransferDecoder();await corrupt.accept({type:'transfer-start',id:'test',bytes:2,sha256:hash});
  await corrupt.accept({type:'transfer-chunk',id:'test',index:0,base64:'e30='});await assert.rejects(corrupt.accept({type:'transfer-end',id:'test'}));
});

test('browser transport uses same-origin WSS and serializes requests without input replay',async()=>{
  const previousSocket=globalThis.WebSocket,previousLocation=globalThis.location;
  const sent=[];let instance;
  class Socket{
    static OPEN=1;readyState=1;
    constructor(url,protocol){assert.equal(url,'wss://10.0.0.2:54437/socket');assert.equal(protocol,'opensimgear-companion-v1');instance=this;queueMicrotask(()=>this.onopen());}
    send(raw){sent.push(JSON.parse(raw));}
    close(){this.readyState=3;this.onclose?.();}
    reply(value){this.onmessage({data:JSON.stringify(value)});}
  }
  globalThis.WebSocket=Socket;globalThis.location={origin:'https://10.0.0.2:54437'};
  try{
    const transport=new BrowserTransport();await transport.connect();
    const first=transport.request({type:'heartbeat',sequence:'1'});
    const second=transport.request({type:'input',sequence:'1',changes:[{controlId:'press',value:true}]});
    await new Promise(resolve=>setImmediate(resolve));
    assert.equal(sent.length,1);instance.reply({ok:true,type:'ack'});
    await first;await new Promise(resolve=>setImmediate(resolve));
    assert.equal(sent.length,2);instance.reply({ok:true,type:'ack'});await second;
    const held=transport.request({type:'heartbeat',sequence:'2'});
    const stale=transport.request({type:'input',sequence:'2',changes:[{controlId:'press',value:false}]});
    await new Promise(resolve=>setImmediate(resolve));
    transport.close();await assert.rejects(held);await assert.rejects(stale);
    assert.equal(sent.filter(v=>v.type==='input').length,1);
    assert.equal(sent[1].changes[0].value,true);
  }finally{globalThis.WebSocket=previousSocket;globalThis.location=previousLocation;}
});

test('an obsolete browser tab cannot erase replacement credentials',()=>{
 const values=new Map();const storage={getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v),removeItem:k=>values.delete(k)};
 const old={token:'a'.repeat(43),companionId:'old',hostId:'host'};
 const fresh={token:'b'.repeat(43),companionId:'new',hostId:'host'};
 saveCredential(storage,old);saveCredential(storage,fresh);
 clearCredential(storage,old.token);assert.deepEqual(readCredential(storage),fresh);
 clearCredential(storage,fresh.token);assert.equal(readCredential(storage),null);
});

test('accepted replies feed visuals before resolving; invalid feedback closes once and cannot replay queued input',async()=>{
 const savedSocket=globalThis.WebSocket,savedLocation=globalThis.location;let socket;const sent=[],visuals=[];
 class Socket{static OPEN=1;readyState=1;constructor(){socket=this;queueMicrotask(()=>this.onopen());}send(raw){sent.push(JSON.parse(raw));}close(){this.readyState=3;this.onclose?.();}reply(reply){this.onmessage({data:JSON.stringify(reply)});}}
 globalThis.WebSocket=Socket;globalThis.location={origin:'https://localhost:54437'};
 try{
  const transport=new BrowserTransport();transport.onreply=reply=>{if(reply.feedback==='invalid')throw Error('Wrong panel feedback');visuals.push(reply.feedback);};await transport.connect();
  const baseline=transport.request({type:'baseline'});await new Promise(resolve=>setImmediate(resolve));socket.reply({ok:true,type:'baseline',feedback:'current'});await baseline;assert.deepEqual(visuals,['current']);
  const bad=transport.request({type:'heartbeat'}),queued=transport.request({type:'input',changes:[{controlId:'primary',value:true}]});const rejected=Promise.all([assert.rejects(bad,/Wrong panel/),assert.rejects(queued,/expired|stopped|Connection/)]);await new Promise(resolve=>setImmediate(resolve));socket.reply({ok:true,type:'ack',feedback:'invalid'});await rejected;
  assert.equal(socket.readyState,3);assert.equal(sent.some(frame=>frame.type==='input'),false);assert.deepEqual(visuals,['current']);
 }finally{globalThis.WebSocket=savedSocket;globalThis.location=savedLocation;}
});

test('legacy tabs cannot delete migrated browser credentials',()=>{
 const values=new Map();const storage={getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v),removeItem:k=>values.delete(k)};
 const old={token:'a'.repeat(43),companionId:'legacy',hostId:'host'};
 values.set('osg-browser-credential',JSON.stringify(old));assert.deepEqual(readCredential(storage),old);
 const fresh={...old,token:'b'.repeat(43),companionId:'replacement'};
 saveCredential(storage,fresh);storage.removeItem('osg-browser-credential');
 assert.deepEqual(readCredential(storage),fresh);
});
