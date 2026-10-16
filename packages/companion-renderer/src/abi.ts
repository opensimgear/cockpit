import type { ObjectKind } from "./types.ts";
import { freeze } from "./validation.ts";

export const HEADER_BYTES = 32;
export const DIRECTORY_BYTES = 16;
export const NONE = 0xffff;
export const MAX_OBJECT_BYTES = 2560 * 1024;
export const OBJECT_ORDER: readonly ObjectKind[] = ["semantics", "scene", "images", "fonts"];
export const MAGIC: Record<ObjectKind, string> = { semantics: "OSGC", scene: "OSGS", images: "OSGI", fonts: "OSGF" };
/** Each record layout is specified in ABI.md; numbers are stable on-wire section IDs. */
export const SECTION = freeze({
  semantics: { controls: 1, outputs: 2, interactions: 3, strings: 4 },
  scene: { pages: 1, nodes: 2, variables: 3, bindings: 4, expressions: 5, timelines: 6, keyframes: 7, hits: 8, points: 9, strings: 10, tiles: 11, tileNodes: 12, lookup: 13 },
  images: { images: 1, pixels: 2 },
  fonts: { fonts: 1, glyphs: 2, pixels: 3 },
});
export const RECORD_BYTES: Record<ObjectKind, Readonly<Record<number, number>>> = freeze({
  semantics: { 1: 32, 2: 32, 3: 24, 4: 1 },
  scene: { 1: 16, 2: 64, 3: 24, 4: 16, 5: 16, 6: 24, 7: 8, 8: 24, 9: 4, 10: 1, 11: 12, 12: 2, 13: 8 },
  images: { 1: 24, 2: 1 }, fonts: { 1: 16, 2: 24, 3: 1 },
});
export const OP = freeze({ rect: 1, image565: 2, mask4: 3, image565Mask4: 4, text: 5, line: 6, polyline: 7 });
export const OP_FEATURE: Record<number, string> = freeze({ 1: "scene.rect", 2: "scene.image565", 3: "scene.mask4", 4: "scene.image565-mask4", 5: "scene.text", 6: "scene.line", 7: "scene.polyline" });
export const INTERACTION = freeze({ momentary: 1, navigation: 2, toggle: 3, guardedMomentary: 4, xy: 5, barycentric3: 6 });
export const INTERACTION_FEATURE: Record<number, string> = freeze({ 1: "input.momentary", 2: "input.navigation", 3: "input.toggle", 4: "input.guarded-momentary", 5: "input.xy", 6: "input.barycentric3" });
/** Stack machine. All scalar arithmetic is signed Q16.16, checked after every instruction. */
export const EXPR = freeze({ constant: 1, variable: 2, add: 3, subtract: 4, multiply: 5, divide: 6, minimum: 7, maximum: 8, less: 9, equal: 10, and: 11, or: 12, not: 13, select: 14, clamp: 15, lookup: 16 });
export const PROPERTY = freeze({ visible: 1, translateX: 2, translateY: 3, color: 4, image: 5, number: 6 });
