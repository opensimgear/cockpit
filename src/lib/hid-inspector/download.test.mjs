import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from '../test-module-loader.mjs';
const {exportChunk}= (await loadModules(['./hid-inspector/download'])).get('./hid-inspector/download');
test('download keeps exact byte chunks across UTF-8 boundaries',()=>{
  const bytes=Buffer.from('{"recording":{"product":"🎮 ä throttle"}}');
  const parts=[];let total=null;
  for(let offset=0;offset<bytes.length;offset+=3){const end=Math.min(offset+3,bytes.length);const result=exportChunk({offset,nextOffset:end,totalBytes:bytes.length,chunk:bytes.subarray(offset,end).toString('base64')},offset,total);total=result.total;parts.push(result.bytes);}
  assert.deepEqual(Buffer.concat(parts),bytes);
});
test('malformed, inconsistent and oversized exports stop before allocating an archive',()=>{
  const good={offset:0,nextOffset:1,totalBytes:1,chunk:'AA=='};
  for(const patch of [{offset:1},{nextOffset:0},{nextOffset:2},{chunk:'?'},{chunk:'AAAA'},{totalBytes:64*1024*1024+1},{totalBytes:-1},{nextOffset:1.5}])assert.throws(()=>exportChunk({...good,...patch},0,null));
  assert.throws(()=>exportChunk(good,0,2));
});
