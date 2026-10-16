import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument, publishDocument, canonical } from '../../packages/companion-designer/src/index.ts';
import { sendPublication, stableCompanionId } from './companions.ts';

test('native publication upload preserves exact canonical v2 bytes in bounded chunks without activation', async () => {
  const publication = await publishDocument(newDocument('Portable panel'), []), calls = [];
  const result = await sendPublication(publication, async (command, payload) => { calls.push({ command, payload }); return command.endsWith('_begin') ? { uploadId: 'owned-upload' } : { host: { hostId: 'host' } }; });
  const chunks = calls.filter(c => c.command.endsWith('_chunk'));
  assert.ok(chunks.length); assert.ok(chunks.every(c => c.payload.bytes.length <= 8192));
  const decoded = JSON.parse(new TextDecoder().decode(new Uint8Array(chunks.flatMap(c => c.payload.bytes))));
  assert.equal(decoded.canonicalDocument, canonical(publication.document)); assert.equal(decoded.publicationHash, publication.publicationHash);
  assert.deepEqual(decoded.assets, []); assert.equal(result.host.hostId, 'host');
  assert.deepEqual(calls.map(c => c.command), ['companion_publish_begin', ...chunks.map(() => 'companion_publish_chunk'), 'companion_publish_commit']);
});
test('failed native upload is aborted; abort failure preserves original diagnosis', async () => {
  const publication = await publishDocument(newDocument(), []), calls = [];
  await assert.rejects(sendPublication(publication, async command => { calls.push(command); if (command.endsWith('_begin')) return { uploadId: 'u' }; throw new Error(command.endsWith('_abort') ? 'abort failed' : 'original chunk error'); }), /original chunk error/);
  assert.equal(calls.at(-1), 'companion_publish_abort');
});
test('missing upload and oversized payload fail before subsequent chunks', async () => {
  const publication = await publishDocument(newDocument(), []); let count = 0;
  await assert.rejects(sendPublication(publication, async () => { count++; return {}; }), /did not start/); assert.equal(count, 1);
  await assert.rejects(sendPublication({ ...publication, assets: [{ assetId: 'large', base64: 'a'.repeat(13 * 1024 * 1024) }] }, async () => { throw new Error('should not send'); }), /budget/);
});
test('routing IDs are strict and cannot use endpoint addresses or names', () => {
  assert.equal(stableCompanionId('controller-123'), 'controller-123');
  for (const value of ['', 'a b', '192.168.1.2', 'a'.repeat(65), null]) assert.throws(() => stableCompanionId(value));
});
