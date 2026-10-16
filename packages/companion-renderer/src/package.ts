import { MAX_OBJECT_BYTES, OBJECT_ORDER } from "./abi.ts";
import { inspectBinaryObject } from "./binary.ts";
import { inspectContent } from "./content.ts";
import { assertProfile, profileHash } from "./profiles.ts";
import type { BinaryObject, DeviceProfile, Digest, Manifest, ObjectKind, PackageSource, PackedPackage, Resources, RuntimeHeadroom } from "./types.ts";
import { assert, canonical, digest, exact, freeze, hash, id, uint } from "./validation.ts";

const countKeys = ["pages", "nodes", "variables", "bindings", "hitRegions", "glyphs", "timelines"] as const;
const memoryKeys = ["internalRamBytes", "psramBytes", "sceneRamBytes", "snapshotBytes", "tileBytes", "transferBytes", "cacheBytes"] as const;
const workKeys = ["maxPageDrawOps", "maxUpdateInstructions", "maxDirtyPixels", "maxLayersPerTile", "maxBlendPixelLayers"] as const;
export type MemoryBudget = Pick<Resources, typeof memoryKeys[number]>;
export function validateManifest(input: unknown): Manifest {
  const item = exact(input, ["formatVersion", "rendererAbi", "sourceUiId", "sourcePublicationHash", "semanticFingerprint", "semanticDescriptorHash", "targetProfileId", "targetProfileHash", "compilerId", "compilerVersion", "viewport", "requires", "counts", "resources", "objects"], "manifest");
  assert(item.formatVersion === 1 && item.rendererAbi === 1, "Unsupported render format/ABI");
  for (const key of ["sourceUiId", "targetProfileId", "compilerId", "compilerVersion"]) id(item[key], key);
  assert(typeof item.sourceUiId === "string" && /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(item.sourceUiId), "sourceUiId: invalid portable identifier");
  for (const key of ["sourcePublicationHash", "semanticFingerprint", "semanticDescriptorHash", "targetProfileHash"]) hash(item[key]);
  const viewport = exact(item.viewport, ["width", "height", "content"], "viewport"); uint(viewport.width, 800); uint(viewport.height, 480);
  assert(Number(viewport.width) > 0 && Number(viewport.height) > 0 && Array.isArray(viewport.content) && viewport.content.length === 4, "Invalid viewport");
  viewport.content.forEach(value => uint(value, 800)); const [x, y, width, height] = viewport.content as number[];
  assert(width > 0 && height > 0 && x + width <= Number(viewport.width) && y + height <= Number(viewport.height), "Content rectangle outside viewport");
  assert(Array.isArray(item.requires) && item.requires.length <= 32, "Feature count exceeds budget");
  for (const [index, feature] of item.requires.entries()) assert(typeof feature === "string" && /^[a-z][a-z0-9.-]{0,63}$/.test(feature) && (!index || item.requires[index - 1] < feature), "Invalid/unsorted/duplicate required feature");
  const counts = exact(item.counts, countKeys, "counts"); Object.values(counts).forEach(value => uint(value, 65535));
  const resources = exact(item.resources, ["objectBytes", ...memoryKeys, ...workKeys], "resources"); Object.values(resources).forEach(value => uint(value));
  assert(Array.isArray(item.objects) && item.objects.length >= 2 && item.objects.length <= 4, "Invalid object count");
  let order = -1, bytes = 0;
  for (const object of item.objects) {
    const descriptor = exact(object, ["kind", "version", "bytes", "sha256"], "object descriptor");
    const next = OBJECT_ORDER.indexOf(descriptor.kind as ObjectKind); assert(next > order && descriptor.version === 1, "Unknown/duplicate/unsorted object kind/version"); order = next;
    uint(descriptor.bytes, MAX_OBJECT_BYTES); assert(descriptor.bytes >= 32, "Truncated object descriptor"); hash(descriptor.sha256); bytes += descriptor.bytes;
  }
  assert(item.objects[0].kind === "semantics" && item.objects[1].kind === "scene" && item.semanticDescriptorHash === item.objects[0].sha256, "Missing/mismatched semantic descriptor");
  assert(bytes === resources.objectBytes, "Object byte accounting mismatch");
  return freeze(JSON.parse(canonical(item)) as Manifest);
}
function validateResources(manifest: Manifest, objects: ReadonlyMap<ObjectKind, BinaryObject>, profile: DeviceProfile): void {
  const facts = inspectContent(objects, profile), resources = manifest.resources;
  assert(canonical(manifest.counts) === canonical(facts.counts), "Manifest counts mismatch binary contents");
  assert(canonical(manifest.requires) === canonical(facts.requires), "Manifest features mismatch binary contents");
  for (const key of workKeys) assert(resources[key] === facts[key], `Manifest ${key} mismatch binary-derived work`);
  assert(facts.controls <= profile.limits.controls, "Logical control budget exceeded");
  assert(resources.snapshotBytes >= facts.minimumSnapshotBytes, "Both property/variable snapshots must fit declared snapshot bytes");
  assert(resources.tileBytes >= profile.tile[0] * profile.tile[1] * 2 && resources.transferBytes === 2048, "Tile/stream buffer reservation mismatch");
  assert(resources.internalRamBytes >= resources.snapshotBytes + resources.tileBytes + resources.transferBytes, "Internal working set omits simultaneous snapshot/tile/stream buffers");
  assert(resources.internalRamBytes + resources.psramBytes >= resources.sceneRamBytes + resources.snapshotBytes + resources.tileBytes + resources.transferBytes + resources.cacheBytes, "Working-set subdivisions exceed total memory reservation");
}
/** Verifies immutable content, structure and budgets. Trusted source semantics must additionally be admitted by the native authority. */
export async function verifyPackage(manifestInput: Uint8Array, payloads: ReadonlyMap<ObjectKind, Uint8Array>, profileInput: DeviceProfile, expectedHash?: Digest, headroomInput?: RuntimeHeadroom): Promise<PackedPackage> {
  const profile = structuredClone(profileInput), headroom = headroomInput ? structuredClone(headroomInput) : undefined;
  assert(manifestInput instanceof Uint8Array && manifestInput.length <= profile.limits.manifestBytes, "Manifest byte budget exceeded");
  const manifestBytes = Uint8Array.from(manifestInput);
  const text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(manifestBytes), manifest = validateManifest(JSON.parse(text));
  assert(canonical(manifest) === text, "Noncanonical manifest encoding");
  assertProfile(manifest, profile, manifestBytes.length, headroom);
  assert(payloads.size === manifest.objects.length, "Object set mismatch");
  const copies = new Map<ObjectKind, Uint8Array>(), inspected = new Map<ObjectKind, BinaryObject>();
  // Copy all supplied byte buffers before awaiting hashing, preventing caller mutation races.
  for (const descriptor of manifest.objects) {
    const payload = payloads.get(descriptor.kind); assert(payload instanceof Uint8Array && payload.length === descriptor.bytes, "Object byte size mismatch"); copies.set(descriptor.kind, Uint8Array.from(payload));
  }
  const packageHash = await digest(manifestBytes); if (expectedHash !== undefined) { hash(expectedHash); assert(packageHash === expectedHash, "Package hash mismatch"); }
  assert(await profileHash(profile) === manifest.targetProfileHash, "Target profile hash mismatch");
  for (const descriptor of manifest.objects) {
    const bytes = copies.get(descriptor.kind)!; assert(await digest(bytes) === descriptor.sha256, `Object hash mismatch: ${descriptor.kind}`);
    inspected.set(descriptor.kind, inspectBinaryObject(descriptor.kind, bytes));
  }
  validateResources(manifest, inspected, profile); return { manifest, manifestBytes, packageHash, objects: copies };
}
export async function packPackage(sourceInput: PackageSource, payloads: ReadonlyMap<ObjectKind, Uint8Array>, profileInput: DeviceProfile, memoryInput: MemoryBudget): Promise<PackedPackage> {
  const source = structuredClone(sourceInput), profile = structuredClone(profileInput), memory = structuredClone(memoryInput);
  const objects = new Map<ObjectKind, Uint8Array>(), inspected = new Map<ObjectKind, BinaryObject>();
  assert(payloads.size >= 2 && payloads.size <= 4, "Object set mismatch");
  for (const kind of OBJECT_ORDER) {
    const payload = payloads.get(kind); if (!payload) continue;
    const bytes = Uint8Array.from(payload); objects.set(kind, bytes); inspected.set(kind, inspectBinaryObject(kind, bytes));
  }
  assert(objects.size === payloads.size, "Unknown object kind"); const facts = inspectContent(inspected, profile);
  const descriptors = [];
  for (const [kind, bytes] of objects) descriptors.push({ kind, version: 1 as const, bytes: bytes.length, sha256: await digest(bytes) });
  const manifest = validateManifest({
    formatVersion: 1, rendererAbi: 1, ...source, semanticDescriptorHash: descriptors[0].sha256,
    targetProfileId: profile.id, targetProfileHash: await profileHash(profile), viewport: { width: profile.width, height: profile.height, content: [...profile.content] },
    requires: facts.requires, counts: facts.counts,
    resources: { objectBytes: descriptors.reduce((sum, object) => sum + object.bytes, 0), ...memory, ...Object.fromEntries(workKeys.map(key => [key, facts[key]])) }, objects: descriptors,
  });
  validateResources(manifest, inspected, profile);
  const manifestBytes = new TextEncoder().encode(canonical(manifest)); assertProfile(manifest, profile, manifestBytes.length);
  return { manifest, manifestBytes, packageHash: await digest(manifestBytes), objects };
}
