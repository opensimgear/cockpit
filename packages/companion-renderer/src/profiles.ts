import type { DeviceProfile, Diagnostic, Manifest, RuntimeHeadroom } from "./types.ts";
import { assert, canonical, digest, freeze, uint } from "./validation.ts";

const features = ["input.momentary", "input.navigation", "scene.image565", "scene.image565-mask4", "scene.line", "scene.mask4", "scene.polyline", "scene.rect", "scene.text", "scene.timeline"];
function profile(waveshare: boolean): DeviceProfile {
  return freeze({
    profileVersion: 1, id: waveshare ? "waveshare-lcd7-s3-render-v1" : "sunton-3248s035r-render-v1", rendererAbi: 1,
    width: waveshare ? 800 : 480, height: waveshare ? 480 : 320,
    content: waveshare ? [0, 24, 800, 448] : [0, 24, 480, 288], tile: [64, 8],
    presentationHz: waveshare ? 10 : 5, features,
    limits: {
      packageBytes: (waveshare ? 2560 : 768) * 1024, manifestBytes: (waveshare ? 8 : 4) * 1024,
      sceneBytes: (waveshare ? 48 : 16) * 1024, semanticBytes: (waveshare ? 16 : 8) * 1024,
      pages: 4, nodes: waveshare ? 256 : 128, variables: waveshare ? 64 : 32,
      bindings: waveshare ? 256 : 128, hitRegions: 32, controls: 32, glyphs: waveshare ? 512 : 256,
      timelines: waveshare ? 8 : 4, dynamicStringBytes: 120,
      internalRamBytes: (waveshare ? 32 : 24) * 1024, psramBytes: waveshare ? 1024 * 1024 : 0,
      transferBytes: 2048, maxUpdateInstructions: waveshare ? 1024 : 512,
      maxDirtyPixels: waveshare ? 96000 : 16384, maxLayersPerTile: waveshare ? 16 : 8,
      maxBlendPixelLayers: waveshare ? 192000 : 16384,
    },
  } as DeviceProfile);
}
export const SUNTON_PROFILE = profile(false);
export const WAVESHARE_PROFILE = profile(true);
export async function profileHash(profile: DeviceProfile): Promise<import("./types.ts").Digest> {
  return digest(new TextEncoder().encode(canonical(profile)));
}
export function checkProfile(manifest: Manifest, profile: DeviceProfile, manifestBytes: number, headroom?: RuntimeHeadroom): Diagnostic[] {
  const errors: Diagnostic[] = [];
  const equal = (path: string, required: number | string, available: number | string) => {
    if (required !== available) errors.push({ path, required, available, message: `${path}: requires ${required}, device provides ${available}` });
  };
  const limit = (path: string, required: number, available: number) => {
    uint(required); uint(available);
    if (required > available) errors.push({ path, required, available, message: `${path}: requires ${required}, budget is ${available}` });
  };
  equal("targetProfileId", manifest.targetProfileId, profile.id);
  equal("rendererAbi", manifest.rendererAbi, profile.rendererAbi);
  equal("viewport.width", manifest.viewport.width, profile.width); equal("viewport.height", manifest.viewport.height, profile.height);
  equal("viewport.content", canonical(manifest.viewport.content), canonical(profile.content));
  for (const feature of manifest.requires) if (!profile.features.includes(feature)) errors.push({ path: "requires", required: feature, available: profile.id, message: `Unsupported feature ${feature} on ${profile.id}` });
  for (const key of Object.keys(manifest.counts) as (keyof Manifest["counts"])[]) limit(`counts.${key}`, manifest.counts[key], profile.limits[key]);
  limit("manifestBytes", manifestBytes, profile.limits.manifestBytes);
  limit("packageBytes", manifestBytes + manifest.resources.objectBytes, profile.limits.packageBytes);
  for (const key of ["internalRamBytes", "psramBytes", "transferBytes", "maxUpdateInstructions", "maxDirtyPixels", "maxLayersPerTile", "maxBlendPixelLayers"] as const) limit(`resources.${key}`, manifest.resources[key], profile.limits[key]);
  for (const object of manifest.objects) {
    if (object.kind === "scene") limit("objects.scene.bytes", object.bytes, profile.limits.sceneBytes);
    if (object.kind === "semantics") limit("objects.semantics.bytes", object.bytes, profile.limits.semanticBytes);
  }
  if (headroom) for (const key of ["internalRamBytes", "psramBytes"] as const) limit(`headroom.${key}`, manifest.resources[key], headroom[key]);
  return errors;
}
export function assertProfile(manifest: Manifest, profile: DeviceProfile, manifestBytes: number, headroom?: RuntimeHeadroom): void {
  const errors = checkProfile(manifest, profile, manifestBytes, headroom);
  assert(errors.length === 0, errors.map(error => error.message).join("; "));
}
