import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';

async function call(client,name,args) {
  const reply=await client.callTool({name,arguments:args});
  const result=reply.structuredContent??JSON.parse(reply.content[0].text);
  assert.equal(result.ok,true,`${name}: ${JSON.stringify(result.error)}`);
  return result.data;
}

// Exercise the same bounded file transport available to an external authenticated client.
export async function verifyRecordingTransfer(writer,reader,identity,archive) {
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
  assert.deepEqual(JSON.parse(file.toString('utf8')).recording,archive);
  const upload=await call(writer,'controller_hid_replay_upload_start',{totalBytes,operationId:randomUUID()});
  let replayId;
  try {
    for(let start=0;start<file.length;start+=upload.maxChunkBytes) {
      const part=file.subarray(start,start+upload.maxChunkBytes);
      const args={uploadId:upload.uploadId,offset:start,chunk:part.toString('base64')};
      const accepted=await call(writer,'controller_hid_replay_upload_chunk',args);
      assert.equal(accepted.nextOffset,start+part.length);
      if(!start)assert.deepEqual(await call(writer,'controller_hid_replay_upload_chunk',args),accepted);
    }
    ({replayId}=await call(writer,'controller_hid_replay_upload_finish',{uploadId:upload.uploadId,operationId:randomUUID()}));
    let after='0';
    const records=[];
    do {
      const args={replayId,after};
      const page=await call(writer,'controller_hid_replay_read',args);
      assert.deepEqual(await call(writer,'controller_hid_replay_read',args),page);
      records.push(...page.records);
      if(page.complete)break;
      assert.ok(BigInt(page.next)>BigInt(after));
      after=page.next;
    }while(true);
    assert.deepEqual(records,archive.records);
  }finally{
    if(replayId)await call(writer,'controller_hid_replay_close',{replayId,operationId:randomUUID()});
    else await call(writer,'controller_hid_replay_upload_cancel',{uploadId:upload.uploadId,operationId:randomUUID()});
  }
}
