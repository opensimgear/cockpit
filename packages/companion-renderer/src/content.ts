import { EXPR, INTERACTION, INTERACTION_FEATURE, NONE, OP, OP_FEATURE, PROPERTY, SECTION } from "./abi.ts";
import { boundedRange, records, section, zero } from "./binary.ts";
import { expressionKind, expressionRange, expressionStackDepth, formatFixed, Q } from "./fixed.ts";
import type { Instruction } from "./fixed.ts";
import type { BinaryObject, Counts, DeviceProfile, Rect } from "./types.ts";
import { assert } from "./validation.ts";

const decoder = new TextDecoder("utf-8", { fatal: true });
function text(object: BinaryObject, sectionId: number, offset: number, length: number, max = 120): string {
  const bytes = section(object, sectionId).data; assert(length <= max, "String byte budget exceeded"); boundedRange(offset, length, bytes.length, "string");
  const value = decoder.decode(bytes.subarray(offset, offset + length)); assert(!/[\u0000-\u001f\u007f]/.test(value), "Invalid string control character"); return value;
}
function named(object: BinaryObject, sectionId: number, data: DataView, used: Set<string>): void {
  const value = text(object, sectionId, data.getUint32(0, true), data.getUint16(4, true), 96);
  assert(/^[A-Za-z0-9][A-Za-z0-9_.-]{0,95}$/.test(value) && !used.has(value), "Invalid/duplicate stable ID"); used.add(value);
}
function range(first: number, count: number, total: number): void { boundedRange(first, count, total, "record range"); }
function rect(data: DataView, offset: number): Rect { return [data.getInt16(offset, true), data.getInt16(offset + 2, true), data.getUint16(offset + 4, true), data.getUint16(offset + 6, true)]; }
function contains(outer: Rect, inner: Rect): boolean { return inner[2] > 0 && inner[3] > 0 && inner[0] >= outer[0] && inner[1] >= outer[1] && inner[0] + inner[2] <= outer[0] + outer[2] && inner[1] + inner[3] <= outer[1] + outer[3]; }
function intersection(a: Rect, b: Rect): Rect { const x = Math.max(a[0], b[0]), y = Math.max(a[1], b[1]); return [x, y, Math.max(0, Math.min(a[0] + a[2], b[0] + b[2]) - x), Math.max(0, Math.min(a[1] + a[3], b[1] + b[3]) - y)]; }

export interface ContentFacts {
  counts: Counts; controls: number; requires: string[]; maxPageDrawOps: number;
  maxUpdateInstructions: number; maxDirtyPixels: number; maxLayersPerTile: number;
  maxBlendPixelLayers: number; minimumSnapshotBytes: number;
}

function validateImages(object: BinaryObject | undefined): DataView[] {
  if (!object) return [];
  const images = records(object, SECTION.images.images), pixels = section(object, SECTION.images.pixels).data;
  assert(images.length > 0, "Empty image object");
  for (const image of images) {
    const width = image.getUint16(0, true), height = image.getUint16(2, true), stride = image.getUint32(4, true);
    const offset = image.getUint32(8, true), bytes = image.getUint32(12, true), format = image.getUint8(16);
    assert(width > 0 && width <= 800 && height > 0 && height <= 480 && (format === 1 || format === 2), "Invalid image dimensions/format");
    assert(stride === (format === 1 ? width * 2 : Math.ceil(width / 2)) && bytes === stride * height, "Image stride/decoded byte mismatch");
    boundedRange(offset, bytes, pixels.length, "image pixels"); zero(image, 17, 24);
    if (format === 2 && width % 2) for (let row = 0; row < height; ++row) assert((pixels[offset + row * stride + stride - 1] & 15) === 0, "Nonzero odd-width A4 padding");
  }
  return images;
}
function validateFonts(object: BinaryObject | undefined): { fonts: DataView[]; glyphs: DataView[] } {
  if (!object) return { fonts: [], glyphs: [] };
  const fonts = records(object, SECTION.fonts.fonts), glyphs = records(object, SECTION.fonts.glyphs), pixels = section(object, SECTION.fonts.pixels).data;
  assert(fonts.length > 0 && glyphs.length > 0, "Empty font object");
  const owned = new Set<number>();
  for (const font of fonts) {
    const first = font.getUint16(0, true), count = font.getUint16(2, true), fallback = font.getUint16(4, true);
    assert(count > 0 && font.getUint16(6, true) > 0, "Invalid font metrics"); range(first, count, glyphs.length);
    assert(fallback >= first && fallback < first + count && glyphs[fallback].getUint32(0, true) === 63, "Font requires '?' fallback glyph"); zero(font, 10, 16);
    let previous = -1;
    for (let index = first; index < first + count; ++index) {
      assert(!owned.has(index), "Overlapping font glyph ownership"); owned.add(index);
      const glyph = glyphs[index], codepoint = glyph.getUint32(0, true), width = glyph.getUint16(4, true), height = glyph.getUint16(6, true), stride = Math.ceil(width / 2);
      assert(codepoint > previous && codepoint <= 0x10ffff && !(codepoint >= 0xd800 && codepoint <= 0xdfff), "Invalid/unsorted glyph codepoint"); previous = codepoint;
      const xOffset = glyph.getInt16(8, true), advance = glyph.getInt16(12, true);
      assert(width <= 256 && height <= 256 && xOffset >= 0 && advance >= xOffset + width && glyph.getUint32(20, true) === stride * height, "Invalid or overlapping glyph metrics/coverage");
      const offset = glyph.getUint32(16, true); boundedRange(offset, stride * height, pixels.length, "glyph pixels"); zero(glyph, 14, 16);
      if (width % 2) for (let row = 0; row < height; ++row) assert((pixels[offset + row * stride + stride - 1] & 15) === 0, "Nonzero glyph A4 padding");
    }
  }
  assert(owned.size === glyphs.length, "Unowned glyphs"); return { fonts, glyphs };
}
function validateSemantics(object: BinaryObject, pageCount: number): { controls: DataView[]; outputs: DataView[]; interactions: DataView[]; requires: Set<string> } {
  const controls = records(object, SECTION.semantics.controls), outputs = records(object, SECTION.semantics.outputs), interactions = records(object, SECTION.semantics.interactions), used = new Set<string>(), requires = new Set<string>();
  assert(controls.length <= 32 && outputs.length <= 64 && interactions.length <= 64, "Semantic table count exceeds ABI cap");
  for (const control of controls) {
    named(object, SECTION.semantics.strings, control, used); assert(control.getUint8(7) === 0, "Unknown control flags");
    const kind = control.getUint8(6), min = control.getInt32(16, true), max = control.getInt32(20, true), fallback = control.getInt32(24, true);
    assert(kind >= 1 && kind <= 3 && min === (kind === 3 ? -Q : 0) && max === Q && fallback >= min && fallback <= max && (kind !== 1 || fallback === 0), "Invalid control range/fallback");
    const target = text(object, SECTION.semantics.strings, control.getUint32(8, true), control.getUint16(12, true), 96);
    assert(/^[A-Za-z0-9][A-Za-z0-9_.-]{0,95}$/.test(target), "Invalid target ID");
  }
  used.clear();
  for (const output of outputs) {
    named(object, SECTION.semantics.strings, output, used); assert(output.getUint8(7) === 0, "Unknown output flags"); zero(output, 30, 32);
    const kind = output.getUint8(6), maxText = output.getUint16(14, true), min = output.getInt32(16, true), max = output.getInt32(20, true);
    assert(kind >= 1 && kind <= 3 && min <= max && maxText <= 120 && (kind === 3 ? maxText > 0 && min === 0 && max === 0 : maxText === 0), "Invalid output descriptor");
    if (kind === 1) assert(min === 0 && max === Q, "Invalid boolean output range");
    assert(text(object, SECTION.semantics.strings, output.getUint32(8, true), output.getUint16(12, true), 96).length > 0, "Missing feedback channel");
    text(object, SECTION.semantics.strings, output.getUint32(24, true), output.getUint16(28, true), 24);
  }
  used.clear(); const owned = new Set<number>();
  for (const [index, interaction] of interactions.entries()) {
    named(object, SECTION.semantics.strings, interaction, used); assert(interaction.getUint8(7) === 0, "Unknown interaction flags"); zero(interaction, 18, 24);
    const kind = interaction.getUint8(6); assert(Object.hasOwn(INTERACTION_FEATURE, kind), "Unknown interaction kind"); requires.add(INTERACTION_FEATURE[kind]);
    const members = [8, 10, 12].map(offset => interaction.getUint16(offset, true));
    const count = kind === INTERACTION.navigation ? 0 : kind === INTERACTION.xy ? 2 : kind === INTERACTION.barycentric3 ? 3 : 1;
    for (const [memberIndex, member] of members.entries()) {
      if (memberIndex >= count) { assert(member === NONE, "Unexpected interaction member"); continue; }
      assert(member < controls.length && !owned.has(member), "Missing/duplicate interaction ownership"); owned.add(member);
      const control = controls[member]; assert(control.getUint16(28, true) === index, "Control owner mismatch");
      assert(control.getUint8(6) === (kind === INTERACTION.xy ? 3 : kind === INTERACTION.barycentric3 ? 2 : 1), "Interaction control kind mismatch");
      assert(control.getUint16(30, true) === (count > 1 ? index : NONE), "Atomic group mismatch");
    }
    const page = interaction.getUint16(14, true), guardMs = interaction.getUint16(16, true);
    assert(kind === INTERACTION.navigation ? page < pageCount : page === NONE, "Invalid navigation destination");
    assert(kind === INTERACTION.guardedMomentary ? guardMs >= 50 && guardMs <= 10000 : guardMs === 0, "Invalid guard duration");
    if (kind === INTERACTION.barycentric3) assert(members.reduce((sum, member) => sum + controls[member].getInt32(24, true), 0) === Q, "Triangle fallback must sum to one");
  }
  assert(owned.size === controls.length, "Unowned logical control"); return { controls, outputs, interactions, requires };
}

/** Re-derives content counts and conservative work from verified bytes, never manifest estimates. */
export function inspectContent(objects: ReadonlyMap<import("./types.ts").ObjectKind, BinaryObject>, profile: DeviceProfile): ContentFacts {
  const semantics = objects.get("semantics"), scene = objects.get("scene"); assert(semantics && scene, "Missing mandatory semantics/scene object");
  text(semantics, SECTION.semantics.strings, 0, section(semantics, SECTION.semantics.strings).data.length, profile.limits.semanticBytes);
  text(scene, SECTION.scene.strings, 0, section(scene, SECTION.scene.strings).data.length, profile.limits.sceneBytes);
  const pages = records(scene, SECTION.scene.pages), nodes = records(scene, SECTION.scene.nodes), variables = records(scene, SECTION.scene.variables), bindings = records(scene, SECTION.scene.bindings);
  const timelines = records(scene, SECTION.scene.timelines), keys = records(scene, SECTION.scene.keyframes), hits = records(scene, SECTION.scene.hits), points = records(scene, SECTION.scene.points);
  const exprs = records(scene, SECTION.scene.expressions), lookup = records(scene, SECTION.scene.lookup), tiles = records(scene, SECTION.scene.tiles), tileNodes = records(scene, SECTION.scene.tileNodes);
  assert(pages.length > 0 && pages.length <= profile.limits.pages && nodes.length <= profile.limits.nodes && variables.length <= profile.limits.variables && bindings.length <= profile.limits.bindings && timelines.length <= profile.limits.timelines && hits.length <= profile.limits.hitRegions, "Scene count exceeds profile");
  const { controls, outputs, interactions, requires } = validateSemantics(semantics, pages.length);
  const images = validateImages(objects.get("images")), { fonts, glyphs } = validateFonts(objects.get("fonts"));
  const dynamic = new Set<number>(), pageNames = new Set<string>(), bounds: Rect[] = [], reachable: Rect[] = [], clips: Rect[] = [];
  let expectedNode = 0, maxPageDrawOps = 0;
  for (const [index, page] of pages.entries()) {
    named(scene, SECTION.scene.strings, page, pageNames); zero(page, 12, 16);
    const first = page.getUint16(6, true), count = page.getUint16(8, true); range(first, count, nodes.length); assert(first === expectedNode, "Noncontiguous page node ownership");
    for (let node = first; node < first + count; ++node) assert(nodes[node].getUint16(0, true) === index, "Node page ownership mismatch");
    expectedNode += count; maxPageDrawOps = Math.max(maxPageDrawOps, count);
  }
  assert(expectedNode === nodes.length, "Unowned scene nodes");
  for (const [index, node] of nodes.entries()) {
    const op = node.getUint8(2); assert(Object.hasOwn(OP_FEATURE, op) && (node.getUint8(3) & ~1) === 0, "Unknown node opcode/flags"); requires.add(OP_FEATURE[op]);
    const initial = rect(node, 4), clip = rect(node, 12), reach = rect(node, 20);
    assert(contains(profile.content, reach) && contains(reach, initial) && contains(profile.content, clip), "Node/clip/reachable bounds outside content");
    bounds[index] = initial; reachable[index] = reach; clips[index] = clip;
    const imageIndex = node.getUint16(28, true), maskIndex = node.getUint16(30, true), fontIndex = node.getUint16(32, true), lineWidth = node.getUint16(34, true);
    const firstPoint = node.getUint16(38, true), pointCount = node.getUint16(40, true), textBytes = node.getUint16(48, true), maxText = node.getUint16(50, true);
    if (op === OP.text) assert(node.getUint16(42, true) <= 4, "Text precision exceeds four places"); else zero(node, 42, 44);
    zero(node, 56, 64);
    const sourceX = node.getUint16(52, true), sourceY = node.getUint16(54, true);
    const useImage = (image: number, format: number) => {
      assert(image < images.length && images[image].getUint8(16) === format, "Invalid image reference/format");
      assert(sourceX + initial[2] <= images[image].getUint16(0, true) && sourceY + initial[3] <= images[image].getUint16(2, true), "Image source region outside asset");
    };
    if (op === OP.image565 || op === OP.image565Mask4) useImage(imageIndex, 1); else assert(imageIndex === NONE, "Unexpected image reference");
    if (op === OP.mask4 || op === OP.image565Mask4) useImage(maskIndex, 2); else assert(maskIndex === NONE, "Unexpected mask reference");
    if (op === OP.text) { assert(fontIndex < fonts.length && maxText > 0 && maxText <= profile.limits.dynamicStringBytes && textBytes <= maxText, "Invalid font/string reference"); text(scene, SECTION.scene.strings, node.getUint32(44, true), textBytes, maxText); }
    else assert(fontIndex === NONE && textBytes === 0 && maxText === 0 && node.getUint32(44, true) === 0, "Unexpected text reference");
    if (op === OP.line || op === OP.polyline) {
      assert(lineWidth >= 1 && lineWidth <= 16 && pointCount >= 2 && pointCount <= 64 && (op !== OP.line || pointCount === 2), "Invalid line geometry"); range(firstPoint, pointCount, points.length);
      for (let point = firstPoint; point < firstPoint + pointCount; ++point) assert(contains(reach, [points[point].getInt16(0, true), points[point].getInt16(2, true), 1, 1]), "Point outside node reachable bounds");
    } else assert(lineWidth === 0 && firstPoint === 0 && pointCount === 0, "Unexpected line geometry");
    if (![OP.image565, OP.mask4, OP.image565Mask4].includes(op)) assert(sourceX === 0 && sourceY === 0, "Unexpected image source region");
  }
  let minimumSnapshotBytes = nodes.length * 16;
  for (const variable of variables) {
    const output = outputs[variable.getUint16(0, true)]; assert(output, "Variable references unknown semantic output");
    assert(variable.getUint8(2) === output.getUint8(6) && variable.getUint8(3) === 0, "Variable type mismatch"); zero(variable, 18, 24);
    const fallback = variable.getInt32(4, true), min = variable.getInt32(8, true), max = variable.getInt32(12, true), maxText = variable.getUint16(16, true);
    assert(min === output.getInt32(16, true) && max === output.getInt32(20, true) && maxText === output.getUint16(14, true) && fallback >= min && fallback <= max, "Variable descriptor mismatch");
    if (variable.getUint8(2) === 1) assert(fallback === 0 || fallback === Q, "Boolean fallback must be false or true");
    minimumSnapshotBytes += 16 + maxText;
  }
  const instructionValues: Instruction[] = exprs.map(instruction => {
    assert(instruction.getUint8(1) === 0, "Unknown expression flags"); zero(instruction, 8, 16);
    const op = instruction.getUint8(0), operand = instruction.getUint16(2, true), value = instruction.getInt32(4, true);
    assert(op >= EXPR.constant && op <= EXPR.lookup, "Unknown expression opcode");
    if (op === EXPR.variable) assert(operand < variables.length && variables[operand].getUint8(2) !== 3 && value === 0, "Invalid scalar variable read");
    else if (op === EXPR.lookup) {
      assert(value >= 2 && value <= 256, "Lookup count outside budget"); range(operand, value, lookup.length);
      for (let index = operand + 1; index < operand + value; ++index) assert(lookup[index].getInt32(0, true) > lookup[index - 1].getInt32(0, true), "Unsorted lookup points");
    } else assert(operand === 0 && (op === EXPR.constant || value === 0), "Unexpected expression operand");
    return { op, operand, value };
  });
  let maxUpdateInstructions = 0; const ownedInstructions = new Set<number>(), propertyOwners = new Set<string>();
  const validateProperty = (node: number, property: number, min: number, max: number) => {
    assert(node < nodes.length && property >= 1 && property <= 6 && min <= max, "Invalid binding/timeline target");
    const owner = `${node}:${property}`; assert(!propertyOwners.has(owner), "Multiple writers to node property"); propertyOwners.add(owner); dynamic.add(node);
    if (property === PROPERTY.visible) assert(min >= 0 && max <= Q, "Visibility result outside boolean range");
    if (property === PROPERTY.color) assert(min >= 0 && max <= 65535, "Color result outside raw RGB565 range");
    if (property === PROPERTY.translateX || property === PROPERTY.translateY) {
      assert(min % Q === 0 && max % Q === 0, "Translation bounds must be integer pixels");
      const b = bounds[node], r = reachable[node], coordinate = property === PROPERTY.translateX ? 0 : 1, extent = coordinate + 2;
      assert(b[coordinate] + min / Q >= r[coordinate] && b[coordinate] + max / Q + b[extent] <= r[coordinate] + r[extent], "Translation escapes reachable bounds");
    }
    if (property === PROPERTY.image) {
      assert(nodes[node].getUint8(2) === OP.image565 && min >= 0 && max < images.length, "Image selection outside image table");
      for (let image = min; image <= max; ++image) assert(images[image].getUint8(16) === 1 && images[image].getUint16(0, true) >= bounds[node][2] + nodes[node].getUint16(52, true) && images[image].getUint16(2, true) >= bounds[node][3] + nodes[node].getUint16(54, true), "Selected image incompatible with source geometry");
    }
    if (property === PROPERTY.number) {
      assert(nodes[node].getUint8(2) === OP.text, "Number formatting requires text node");
      const precision = nodes[node].getUint16(42, true), bytes = nodes[node].getUint16(50, true);
      assert(Math.max(formatFixed(min, precision).length, formatFixed(max, precision).length) <= bytes, "Formatted number exceeds text byte budget");
    }
  };
  for (const binding of bindings) {
    assert(binding.getUint8(3) === 0, "Unknown binding flags"); const first = binding.getUint16(4, true), count = binding.getUint16(6, true);
    assert(count > 0, "Empty expression"); range(first, count, exprs.length); const instructions = instructionValues.slice(first, first + count); expressionStackDepth(instructions);
    const inferred = expressionRange(instructions, variables.map(variable => [variable.getInt32(8, true), variable.getInt32(12, true)]), lookup.map(point => [point.getInt32(0, true), point.getInt32(4, true)]));
    assert(inferred && inferred[0] >= binding.getInt32(8, true) && inferred[1] <= binding.getInt32(12, true), "Binding result bounds do not contain proven expression range");
    const kind = expressionKind(instructions, variables.map(variable => variable.getUint8(2) === 1 ? "boolean" : "number"));
    assert(kind !== null && (binding.getUint8(2) !== PROPERTY.visible || kind === "boolean"), "Binding result is not a proven boolean");
    for (let index = first; index < first + count; ++index) { assert(!ownedInstructions.has(index), "Overlapping binding expressions"); ownedInstructions.add(index); }
    // Lookup validation/search is bounded work too; charge its maximum reference-evaluator visits.
    maxUpdateInstructions += instructions.reduce((sum, instruction) => sum + 1 + (instruction.op === EXPR.lookup ? instruction.value! * 2 : 0), 0);
    validateProperty(binding.getUint16(0, true), binding.getUint8(2), binding.getInt32(8, true), binding.getInt32(12, true));
  }
  assert(ownedInstructions.size === exprs.length, "Unowned expression instructions");
  const ownedKeys = new Set<number>();
  for (const timeline of timelines) {
    requires.add("scene.timeline"); zero(timeline, 10, 24); const first = timeline.getUint16(4, true), count = timeline.getUint16(6, true), duration = timeline.getUint16(8, true);
    assert(count >= 2 && count <= 8 && duration >= 50 && duration <= 2000 && timeline.getUint8(3) <= 1, "Invalid timeline bounds"); range(first, count, keys.length);
    let previous = -1, min = 2147483647, max = -2147483648;
    for (let index = first; index < first + count; ++index) {
      assert(!ownedKeys.has(index), "Overlapping timeline keyframes"); ownedKeys.add(index); zero(keys[index], 2, 4);
      const time = keys[index].getUint16(0, true), value = keys[index].getInt32(4, true); assert(time > previous && time <= duration, "Invalid keyframe time"); previous = time; min = Math.min(min, value); max = Math.max(max, value);
      if (timeline.getUint8(2) === PROPERTY.visible) assert(timeline.getUint8(3) === 0 && (value === 0 || value === Q), "Visibility timelines require step boolean keyframes");
    }
    assert(keys[first].getUint16(0, true) === 0 && previous === duration, "Timeline endpoints required");
    validateProperty(timeline.getUint16(0, true), timeline.getUint8(2), min, max);
  }
  assert(ownedKeys.size === keys.length, "Unowned timeline keys");
  const hitOwners = new Set<number>();
  for (const hit of hits) {
    zero(hit, 14, 24); const interaction = hit.getUint16(0, true), page = hit.getUint16(2, true), node = hit.getUint16(12, true);
    assert(interaction < interactions.length && !hitOwners.has(interaction) && page < pages.length && node < nodes.length && nodes[node].getUint16(0, true) === page, "Invalid/duplicate hit owner");
    assert(contains(profile.content, rect(hit, 4)) && contains(reachable[node], rect(hit, 4)), "Hit outside visible control"); hitOwners.add(interaction); dynamic.add(node);
  }
  assert(hitOwners.size === interactions.length, "Interaction missing hit region");
  // Reconstruct every required tile membership; never trust initial-only indexes for moving nodes.
  const expectedTiles = new Map<string, number[]>(), dynamicTiles = new Set<string>();
  const [tileWidth, tileHeight] = profile.tile, [originX, originY, contentWidth, contentHeight] = profile.content;
  const tileRect = (x: number, y: number): Rect => [originX + x * tileWidth, originY + y * tileHeight, Math.min(tileWidth, contentWidth - x * tileWidth), Math.min(tileHeight, contentHeight - y * tileHeight)];
  for (const [index, node] of nodes.entries()) {
    const r = intersection(reachable[index], clips[index]), page = node.getUint16(0, true); if (!r[2] || !r[3]) continue;
    for (let y = Math.floor((r[1] - originY) / tileHeight); y <= Math.floor((r[1] + r[3] - 1 - originY) / tileHeight); ++y) for (let x = Math.floor((r[0] - originX) / tileWidth); x <= Math.floor((r[0] + r[2] - 1 - originX) / tileWidth); ++x) {
      const key = `${page}:${y}:${x}`; const members = expectedTiles.get(key) ?? []; members.push(index); expectedTiles.set(key, members); if (dynamic.has(index)) dynamicTiles.add(key);
    }
  }
  assert(tiles.length === expectedTiles.size, "Spatial tile coverage mismatch");
  let previousTile = -1, tileNodeCursor = 0, maxLayersPerTile = 1;
  for (const tile of tiles) {
    zero(tile, 10, 12); const page = tile.getUint16(0, true), x = tile.getUint16(2, true), y = tile.getUint16(4, true), first = tile.getUint16(6, true), count = tile.getUint16(8, true);
    assert(page < pages.length && x * tileWidth < contentWidth && y * tileHeight < contentHeight, "Tile outside viewport");
    const order = page * 65536 + y * 256 + x; assert(order > previousTile, "Unsorted/duplicate tile index"); previousTile = order;
    const members = expectedTiles.get(`${page}:${y}:${x}`); assert(members && count === members.length && first === tileNodeCursor, "Tile membership range mismatch"); range(first, count, tileNodes.length);
    for (let index = 0; index < count; ++index) assert(tileNodes[first + index].getUint16(0, true) === members[index], "Tile draw-order membership mismatch");
    tileNodeCursor += count; maxLayersPerTile = Math.max(maxLayersPerTile, count + 1);
  }
  assert(tileNodeCursor === tileNodes.length, "Unowned tile membership records");
  const dirtyByPage = new Map<number, number>(), blendByPage = new Map<number, number>();
  for (const key of dynamicTiles) {
    const [page, y, x] = key.split(":").map(Number), tile = tileRect(x, y); dirtyByPage.set(page, (dirtyByPage.get(page) ?? 0) + tile[2] * tile[3]);
    for (const node of expectedTiles.get(key)!) if ([OP.mask4, OP.image565Mask4, OP.text].includes(nodes[node].getUint8(2))) {
      const r = intersection(tile, intersection(reachable[node], clips[node])); blendByPage.set(page, (blendByPage.get(page) ?? 0) + r[2] * r[3]);
    }
  }
  return {
    counts: { pages: pages.length, nodes: nodes.length, variables: variables.length, bindings: bindings.length, hitRegions: hits.length, glyphs: glyphs.length, timelines: timelines.length },
    controls: controls.length, requires: [...requires].sort(), maxPageDrawOps, maxUpdateInstructions,
    maxDirtyPixels: Math.max(0, ...dirtyByPage.values()), maxLayersPerTile,
    maxBlendPixelLayers: Math.max(0, ...blendByPage.values()), minimumSnapshotBytes: minimumSnapshotBytes * 2,
  };
}
