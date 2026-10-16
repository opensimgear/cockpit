import test from 'node:test';
import assert from 'node:assert/strict';
import { SUNTON_PROFILE, canonical, packPackage, profileHash, verifyPackage } from '../src/index.ts';
import { fixturePackage } from './build-fixtures.mjs';

const copyObjects = objects => new Map([...objects].map(([kind, bytes]) => [kind, Uint8Array.from(bytes)]));
const memoryKeys = ['internalRamBytes','psramBytes','sceneRamBytes','snapshotBytes','tileBytes','transferBytes','cacheBytes'];

test('packer snapshots source, profile, memory and objects before its first await', async () => {
  const fixture = await fixturePackage(), profile = structuredClone(SUNTON_PROFILE);
  const source = Object.fromEntries(['sourceUiId','sourcePublicationHash','semanticFingerprint','compilerId','compilerVersion'].map(key => [key, fixture.manifest[key]]));
  const memory = Object.fromEntries(memoryKeys.map(key => [key, fixture.manifest.resources[key]])), objects = copyObjects(fixture.objects);
  const pending = packPackage(source, objects, profile, memory);
  source.sourceUiId = 'changed-source'; source.compilerVersion = 'changed-compiler';
  profile.width = 1; profile.features.length = 0; profile.limits.nodes = 0;
  memory.internalRamBytes = 0;
  for (const bytes of objects.values()) bytes[0] = 0;
  objects.clear();
  const packed = await pending;
  assert.equal(packed.packageHash, fixture.packageHash);
  assert.equal(packed.manifest.sourceUiId, fixture.manifest.sourceUiId);
  assert.equal(packed.manifest.resources.internalRamBytes, fixture.manifest.resources.internalRamBytes);
  await verifyPackage(packed.manifestBytes, packed.objects, SUNTON_PROFILE, packed.packageHash);
});

test('verifier snapshots profile, headroom, manifest and payload buffers before awaiting hashes', async () => {
  const fixture = await fixturePackage(), profile = structuredClone(SUNTON_PROFILE);
  const headroom = { internalRamBytes: 8192, psramBytes: 0 }, manifestBytes = Uint8Array.from(fixture.manifestBytes), objects = copyObjects(fixture.objects);
  const pending = verifyPackage(manifestBytes, objects, profile, fixture.packageHash, headroom);
  profile.features.length = 0; profile.limits.nodes = 0; profile.width = 1; headroom.internalRamBytes = 0;
  manifestBytes[0] = 0;
  for (const bytes of objects.values()) bytes[0] = 0;
  objects.clear();
  const verified = await pending;
  assert.equal(verified.packageHash, fixture.packageHash);
  assert.equal(verified.manifest.targetProfileHash, await profileHash(SUNTON_PROFILE));
});

test('profile hash and feature admission cannot observe different caller-owned profile revisions', async () => {
  const fixture = await fixturePackage(), profile = structuredClone(SUNTON_PROFILE);
  profile.features = profile.features.filter(feature => feature !== 'scene.mask4');
  const manifest = structuredClone(fixture.manifest);
  manifest.targetProfileHash = await profileHash(profile);
  const originalProfile = canonical(profile), originalDigest = crypto.subtle.digest;
  // Forward real SHA-256 unchanged; only make the previous post-hash mutation deterministic.
  crypto.subtle.digest = function (algorithm, bytes) {
    const result = originalDigest.call(this, algorithm, bytes);
    if (new TextDecoder().decode(bytes) === originalProfile) queueMicrotask(() => profile.features.push('scene.mask4'));
    return result;
  };
  try {
    await assert.rejects(() => verifyPackage(new TextEncoder().encode(canonical(manifest)), fixture.objects, profile), /scene.mask4/);
  } finally { crypto.subtle.digest = originalDigest; }
});
