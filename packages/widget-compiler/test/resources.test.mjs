import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFileSync} from 'node:fs';
import {loadCompilerResource} from '../src/resources.ts';

test('browser resource loading keeps exact bytes and fails clearly on missing assets',async()=>{
  const local=new URL('../node_modules/@jsquash/png/LICENSE',import.meta.url);assert.deepEqual(await loadCompilerResource(local),new Uint8Array(readFileSync(local)));
  const server=createServer((request,response)=>{if(request.url==='/compiler.wasm'){response.writeHead(200,{'content-type':'application/wasm'});response.end(Uint8Array.from([0,97,115,109]));}else{response.writeHead(404);response.end();}});
  await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve);});
  try{const base='http://127.0.0.1:'+server.address().port;assert.deepEqual(await loadCompilerResource(new URL('/compiler.wasm',base)),Uint8Array.from([0,97,115,109]));await assert.rejects(loadCompilerResource(new URL('/missing.wasm',base)),/resource could not be loaded/);}
  finally{await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
});

test('distributed codec notices retain the exact installed upstream texts',()=>{
  for(const codec of ['png','jpeg','webp'])for(const [installed,published] of [['LICENSE','LICENSE.txt'],['codec/LICENSE.codec.md','LICENSE.codec.md']]){
    const upstream=readFileSync(new URL('../node_modules/@jsquash/'+codec+'/'+installed,import.meta.url));
    const bundled=readFileSync(new URL('../../../public/licenses/jsquash/'+codec+'/'+published,import.meta.url));assert.deepEqual(bundled,upstream);
  }
});
