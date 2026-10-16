export type Digest = `sha256:${string}`;
export type ObjectKind = "semantics" | "scene" | "images" | "fonts";
export type Rect = readonly [number, number, number, number];
export interface ObjectDescriptor { kind: ObjectKind; version: 1; bytes: number; sha256: Digest }
export interface Counts {
  pages: number; nodes: number; variables: number; bindings: number;
  hitRegions: number; glyphs: number; timelines: number;
}
export interface Resources {
  objectBytes: number; internalRamBytes: number; psramBytes: number; sceneRamBytes: number;
  snapshotBytes: number; tileBytes: number; transferBytes: number; cacheBytes: number;
  maxPageDrawOps: number; maxUpdateInstructions: number; maxDirtyPixels: number;
  maxLayersPerTile: number; maxBlendPixelLayers: number;
}
export interface Manifest {
  formatVersion: 1; rendererAbi: 1; sourceUiId: string; sourcePublicationHash: Digest;
  semanticFingerprint: Digest; semanticDescriptorHash: Digest;
  targetProfileId: string; targetProfileHash: Digest; compilerId: string; compilerVersion: string;
  viewport: { width: number; height: number; content: Rect };
  requires: string[]; counts: Counts; resources: Resources; objects: ObjectDescriptor[];
}
export interface ProfileLimits extends Counts {
  packageBytes: number; manifestBytes: number; sceneBytes: number; semanticBytes: number;
  controls: number; dynamicStringBytes: number; internalRamBytes: number; psramBytes: number;
  transferBytes: number; maxUpdateInstructions: number; maxDirtyPixels: number;
  maxLayersPerTile: number; maxBlendPixelLayers: number;
}
/** Qualification is explicit: these profiles describe proposed admission caps, not measured performance. */
export interface DeviceProfile {
  profileVersion: 1; id: string; rendererAbi: 1; width: number; height: number; content: Rect;
  tile: readonly [number, number]; presentationHz: number; features: readonly string[]; limits: ProfileLimits;
}
export interface RuntimeHeadroom { internalRamBytes: number; psramBytes: number }
export interface Diagnostic { path: string; required: number | string; available: number | string; message: string }
export interface BinarySection { kind: number; data: Uint8Array }
export interface InspectedSection extends BinarySection { recordBytes: number; count: number; offset: number }
export interface BinaryObject {
  kind: ObjectKind; bytes: Uint8Array; sections: ReadonlyMap<number, InspectedSection>;
}
export interface PackedPackage { manifest: Manifest; manifestBytes: Uint8Array; packageHash: Digest; objects: ReadonlyMap<ObjectKind, Uint8Array> }
export interface PackageSource {
  sourceUiId: string; sourcePublicationHash: Digest; semanticFingerprint: Digest;
  compilerId: string; compilerVersion: string;
}
