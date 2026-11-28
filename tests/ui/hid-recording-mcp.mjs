import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {verifyReplayQueries} from './hid-query-mcp.mjs';

export async function call(client,name,args={}) {
  const reply=await client.callTool({name,arguments:args});
  assert.ok(Buffer.byteLength(JSON.stringify(reply))<=1024*1024,`${name} exceeded the MCP reply budget`);
  const result=reply.structuredContent??JSON.parse(reply.content[0].text);
  assert.equal(result.ok,true,`${name}: ${JSON.stringify(result.error)}`);
  return result.data;
}

export async function rejected(client,name,args,code) {
  const reply=await client.callTool({name,arguments:args});
  const result=reply.structuredContent??JSON.parse(reply.content.find(part=>part.type==='text').text);
  assert.equal(result.ok,false,`${name} unexpectedly succeeded`);
  if(code)assert.equal(result.error.code,code,JSON.stringify(result));
  return result;
}

export async function verifyInspectionTransfer(client,source,identity={}) {
  const chunks=[];let offset=0,totalBytes,contentFingerprint;
  do {
    const args={source,...identity,offset,limit:8192,...(contentFingerprint?{contentFingerprint}:{})};
    const slice=await call(client,'controller_hid_inspection_slice',args);
    totalBytes??=slice.totalBytes;contentFingerprint??=slice.contentFingerprint;
    assert.equal(slice.totalBytes,totalBytes);assert.equal(slice.contentFingerprint,contentFingerprint);
    assert.equal(slice.offset,offset);assert.equal(slice.encoding,'base64');
    const bytes=Buffer.from(slice.data,'base64');
    assert.ok(bytes.length>0&&bytes.length<=args.limit);
    assert.equal(slice.nextOffset,offset+bytes.length);assert.equal(slice.done,slice.nextOffset===totalBytes);
    chunks.push(bytes);offset=slice.nextOffset;
  }while(offset<totalBytes);
  await rejected(client,'controller_hid_inspection_slice',{source,...identity,offset:0,limit:1,contentFingerprint:'0'.repeat(64)});
  await rejected(client,'controller_hid_inspection_slice',{source,...identity,offset:0,limit:32769},'invalid_arguments');
  return JSON.parse(Buffer.concat(chunks).toString('utf8'));
}

export async function verifyReportTransfer(client,identity,row) {
  let offset=0,totalBytes;const chunks=[];
  do {
    const args={...identity,sequence:row.sequence,offset,limit:8192};
    const slice=(await call(client,'controller_hid_report_slice',args)).result;
    totalBytes??=slice.totalBytes;assert.equal(slice.totalBytes,totalBytes);
    assert.equal(slice.sequence,row.sequence);assert.equal(slice.offset,offset);
    const bytes=Buffer.from(slice.data,'base64');assert.ok(bytes.length>0&&bytes.length<=args.limit);
    assert.equal(slice.nextOffset,offset+bytes.length);assert.equal(slice.done,slice.nextOffset===totalBytes);
    chunks.push(bytes);offset=slice.nextOffset;
  }while(offset<totalBytes);
  assert.deepEqual(JSON.parse(Buffer.concat(chunks).toString('utf8')),row);
}

// A complete external-client import, including ownership and byte/cursor fences.
export async function verifyReplayTransfer(writer,reader,archive,chunkBytes=24576,expected) {
  const file=Buffer.from(JSON.stringify({recording:archive}));
  const upload=await call(writer,'controller_hid_replay_upload_start',{totalBytes:file.length,operationId:randomUUID()});
  assert.ok(upload.maxChunkBytes>0&&upload.maxChunkBytes<=24576);
  let replayId;
  try {
    await rejected(reader,'controller_hid_replay_read',{replayId:'not-created'});
    await rejected(writer,'controller_hid_replay_upload_chunk',{uploadId:upload.uploadId,offset:1,chunk:file.subarray(0,1).toString('base64')});
    await rejected(writer,'controller_hid_replay_upload_finish',{uploadId:upload.uploadId,operationId:randomUUID()});
    chunkBytes=Math.min(chunkBytes,upload.maxChunkBytes);
    for(let offset=0;offset<file.length;offset+=chunkBytes) {
      const part=file.subarray(offset,offset+chunkBytes);
      const args={uploadId:upload.uploadId,offset,chunk:part.toString('base64')};
      const accepted=await call(writer,'controller_hid_replay_upload_chunk',args);
      assert.equal(accepted.nextOffset,offset+part.length);
      if(!offset) {
        assert.deepEqual(await call(writer,'controller_hid_replay_upload_chunk',args),accepted);
        const conflict=Buffer.from(part);conflict[0]^=1;
        await rejected(writer,'controller_hid_replay_upload_chunk',{...args,chunk:conflict.toString('base64')});
      }
    }
    ({replayId}=await call(writer,'controller_hid_replay_upload_finish',{uploadId:upload.uploadId,operationId:randomUUID()}));
    const descriptor=await verifyInspectionTransfer(writer,'replay_read',{replayId,part:'descriptor'});
    assert.deepEqual(descriptor.descriptor,archive.descriptor);
    if(archive.interfaceId.startsWith('authored-'))await verifyReplayQueries(call,rejected,writer,replayId,archive,expected);
    await rejected(reader,'controller_hid_replay_read',{replayId});
    for(const args of [{after:'01'},{after:1},{limit:129},{part:'recording',maxBytes:24577}])
      await rejected(writer,'controller_hid_replay_read',{replayId,...args},'invalid_arguments');
    await rejected(writer,'controller_hid_replay_read',{replayId,after:String(archive.records.length+1)});
    let after='0';const records=[];
    while(true) {
      const args={replayId,after,limit:chunkBytes===257?1:128};
      const page=await call(writer,'controller_hid_replay_read',args);
      assert.deepEqual(await call(writer,'controller_hid_replay_read',args),page);
      assert.ok(page.records.length<=args.limit);
      records.push(...page.records);
      if(page.complete)break;
      assert.ok(BigInt(page.next)>BigInt(after));after=page.next;
    }
    const slices=[];let byteOffset=0,totalBytes;
    do {
      const args={replayId,part:'recording',byteOffset,maxBytes:chunkBytes};
      const slice=await call(writer,'controller_hid_replay_read',args);
      assert.deepEqual(await call(writer,'controller_hid_replay_read',args),slice);
      const bytes=Buffer.from(slice.chunk,'base64');
      totalBytes??=slice.totalBytes;
      assert.equal(slice.totalBytes,totalBytes);assert.ok(bytes.length>0&&bytes.length<=chunkBytes);
      assert.equal(slice.nextByteOffset,byteOffset+bytes.length);
      slices.push(bytes);byteOffset=slice.nextByteOffset;
      assert.equal(slice.complete,byteOffset===totalBytes);
    }while(byteOffset<totalBytes);
    const exported=Buffer.concat(slices);
    assert.deepEqual(JSON.parse(exported.toString('utf8')),archive);
    const exactRecords=records.map(row=>{
      if(!row.recordSlice)return row;
      const {byteOffset,totalBytes}=row.recordSlice;
      const exact=JSON.parse(exported.subarray(byteOffset,byteOffset+totalBytes).toString('utf8'));
      assert.equal(exact.sequence,row.sequence);
      return exact;
    });
    assert.deepEqual(exactRecords,archive.records);
    return exactRecords;
  }finally {
    if(replayId) {
      await call(writer,'controller_hid_replay_close',{replayId,operationId:randomUUID()});
      await rejected(writer,'controller_hid_replay_read',{replayId});
    }else await call(writer,'controller_hid_replay_upload_cancel',{uploadId:upload.uploadId,operationId:randomUUID()});
  }
}

// Exercise the same bounded file transport available to an external authenticated client.
export async function verifyRecordingTransfer(writer,reader,identity,archive) {
  await rejected(reader,'controller_hid_record_export',{...identity,offset:0,limit:32769},'invalid_arguments');
  let offset=0,totalBytes;
  const chunks=[];
  do {
    const result=(await call(reader,'controller_hid_record_export',{...identity,offset,limit:8192})).result;
    const bytes=Buffer.from(result.chunk,'base64');
    totalBytes??=result.totalBytes;
    assert.equal(result.totalBytes,totalBytes);
    assert.ok(bytes.length&&bytes.length<=8192);
    assert.equal(result.nextOffset,offset+bytes.length);
    offset=result.nextOffset;
    chunks.push(bytes);
  }while(offset<totalBytes);
  const file=Buffer.concat(chunks);
  const downloaded=JSON.parse(file.toString('utf8')).recording;
  if(archive)assert.deepEqual(downloaded,archive);
  await rejected(reader,'controller_hid_record_export',{...identity,offset:totalBytes+1,limit:1});
  await verifyReplayTransfer(writer,reader,downloaded);
  return downloaded;
}
