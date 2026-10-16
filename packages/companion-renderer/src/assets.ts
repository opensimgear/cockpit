import type { BinarySection } from "./types.ts";
import { packBinaryObject, view } from "./binary.ts";
import { assert, digest, id, uint } from "./validation.ts";

export interface PreparedImage { id: string; width: number; height: number; format: "rgb565" | "a4"; pixels: Uint8Array }
export interface PreparedGlyph { codepoint: number; width: number; height: number; xOffset: number; yOffset: number; advance: number; coverage: Uint8Array }
export interface PreparedFont { id: string; lineHeight: number; ascent: number; glyphs: PreparedGlyph[]; license?: string }
export function rgb565(red: number, green: number, blue: number): number {
  return (Math.round(red * 31 / 255) << 11) | (Math.round(green * 63 / 255) << 5) | Math.round(blue * 31 / 255);
}
function dimensions(width: number, height: number): void { uint(width, 800); uint(height, 480); assert(width > 0 && height > 0, "Invalid prepared image dimensions"); }
export function prepareA4(idValue: string, width: number, height: number, alpha: Uint8Array): PreparedImage {
  id(idValue); dimensions(width, height); assert(alpha.length === width * height, "Alpha dimensions mismatch");
  const stride = Math.ceil(width / 2), pixels = new Uint8Array(stride * height);
  for (let y = 0; y < height; ++y) for (let x = 0; x < width; ++x) pixels[y * stride + (x >> 1)] |= Math.round(alpha[y * width + x] * 15 / 255) << (x % 2 ? 0 : 4);
  return { id: idValue, width, height, format: "a4", pixels };
}
export function prepareRgba(idValue: string, width: number, height: number, rgba: Uint8Array): { image: PreparedImage; mask: PreparedImage } {
  id(idValue); dimensions(width, height); assert(rgba.length === width * height * 4, "RGBA dimensions mismatch");
  const pixels = new Uint8Array(width * height * 2), data = view(pixels), alpha = new Uint8Array(width * height);
  for (let i = 0; i < width * height; ++i) { data.setUint16(i * 2, rgb565(rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]), true); alpha[i] = rgba[i * 4 + 3]; }
  return { image: { id: idValue, width, height, format: "rgb565", pixels }, mask: prepareA4(idValue + "-alpha", width, height, alpha) };
}
export function prepareGlyph(input: Omit<PreparedGlyph, "coverage"> & { alpha: Uint8Array }): PreparedGlyph {
  uint(input.codepoint, 0x10ffff); uint(input.width, 256); uint(input.height, 256);
  assert(input.alpha.length === input.width * input.height, "Glyph alpha dimensions mismatch");
  if (!input.width || !input.height) return { codepoint: input.codepoint, width: input.width, height: input.height, xOffset: input.xOffset, yOffset: input.yOffset, advance: input.advance, coverage: new Uint8Array() };
  const { alpha, ...metrics } = input;
  return { ...metrics, coverage: prepareA4("glyph", input.width, input.height, alpha).pixels };
}
function concat(inputs: Uint8Array[]): Uint8Array { const output = new Uint8Array(inputs.reduce((sum, bytes) => sum + bytes.length, 0)); let offset = 0; for (const bytes of inputs) { output.set(bytes, offset); offset += bytes.length; } return output; }
export async function packImages(inputValues: PreparedImage[]): Promise<{ bytes?: Uint8Array; indexes: Map<string, number> }> {
  const inputs = structuredClone(inputValues);
  const indexes = new Map<string, number>(), unique: PreparedImage[] = [], seen = new Map<string, number>();
  for (const input of inputs) {
    id(input.id); assert(!indexes.has(input.id), "Duplicate prepared image ID"); dimensions(input.width, input.height);
    assert(input.format === "rgb565" || input.format === "a4", "Unsupported prepared image format");
    const stride = input.format === "rgb565" ? input.width * 2 : Math.ceil(input.width / 2);
    assert(input.pixels.length === stride * input.height, "Prepared image byte count mismatch");
    const key = [input.width, input.height, input.format, await digest(input.pixels)].join(":");
    let index = seen.get(key); if (index === undefined) { index = unique.length; seen.set(key, index); unique.push(input); } indexes.set(input.id, index);
  }
  if (!unique.length) return { indexes };
  const table = new Uint8Array(unique.length * 24), data = view(table); let offset = 0;
  for (const [index, input] of unique.entries()) {
    const base = index * 24; data.setUint16(base, input.width, true); data.setUint16(base + 2, input.height, true);
    data.setUint32(base + 4, input.format === "rgb565" ? input.width * 2 : Math.ceil(input.width / 2), true); data.setUint32(base + 8, offset, true);
    data.setUint32(base + 12, input.pixels.length, true); data.setUint8(base + 16, input.format === "rgb565" ? 1 : 2); offset += input.pixels.length;
  }
  return { indexes, bytes: packBinaryObject("images", [{ kind: 1, data: table }, { kind: 2, data: concat(unique.map(image => image.pixels)) }]) };
}
export function packFonts(inputs: PreparedFont[]): { bytes?: Uint8Array; indexes: Map<string, number> } {
  const indexes = new Map<string, number>(); if (!inputs.length) return { indexes };
  const table = new Uint8Array(inputs.length * 16), fontData = view(table), glyphTables: Uint8Array[] = [], coverage: Uint8Array[] = [];
  let glyphIndex = 0, pixelOffset = 0;
  for (const [fontIndex, font] of inputs.entries()) {
    id(font.id); assert(!indexes.has(font.id), "Duplicate font ID"); indexes.set(font.id, fontIndex);
    uint(font.lineHeight, 65535); assert(font.lineHeight > 0 && Number.isInteger(font.ascent) && font.ascent >= 0 && font.ascent <= font.lineHeight, "Invalid prepared font metrics");
    const glyphs = [...font.glyphs].sort((a, b) => a.codepoint - b.codepoint), first = glyphIndex;
    assert(glyphs.length > 0 && glyphs.some(glyph => glyph.codepoint === 63), "Font subset requires '?' fallback");
    fontData.setUint16(fontIndex * 16, first, true); fontData.setUint16(fontIndex * 16 + 2, glyphs.length, true);
    fontData.setUint16(fontIndex * 16 + 4, first + glyphs.findIndex(glyph => glyph.codepoint === 63), true); fontData.setUint16(fontIndex * 16 + 6, font.lineHeight, true); fontData.setInt16(fontIndex * 16 + 8, font.ascent, true);
    for (const [index, glyph] of glyphs.entries()) {
      uint(glyph.codepoint, 0x10ffff); assert(index === 0 || glyph.codepoint > glyphs[index - 1].codepoint, "Duplicate prepared glyph");
      uint(glyph.width, 256); uint(glyph.height, 256);
      assert(Number.isInteger(glyph.xOffset) && glyph.xOffset >= 0 && Number.isInteger(glyph.yOffset) && glyph.yOffset >= -32768 && glyph.yOffset <= 32767 && Number.isInteger(glyph.advance) && glyph.advance >= glyph.xOffset + glyph.width && glyph.advance <= 32767, "Invalid prepared glyph metrics");
      assert(glyph.coverage.length === Math.ceil(glyph.width / 2) * glyph.height, "Prepared glyph coverage byte mismatch");
      const record = new Uint8Array(24), data = view(record);
      data.setUint32(0, glyph.codepoint, true); data.setUint16(4, glyph.width, true); data.setUint16(6, glyph.height, true); data.setInt16(8, glyph.xOffset, true); data.setInt16(10, glyph.yOffset, true); data.setInt16(12, glyph.advance, true); data.setUint32(16, pixelOffset, true); data.setUint32(20, glyph.coverage.length, true);
      glyphTables.push(record); coverage.push(glyph.coverage); pixelOffset += glyph.coverage.length; ++glyphIndex;
    }
  }
  const notices = [...new Set(inputs.flatMap(font => font.license ? [font.license.replace(/\r\n/g, "\n")] : []))];
  if (notices.length) coverage.push(new TextEncoder().encode("\nOSG FONT LICENSE UTF-8\n" + notices.join("\n")));
  const sections: BinarySection[] = [{ kind: 1, data: table }, { kind: 2, data: concat(glyphTables) }, { kind: 3, data: concat(coverage) }];
  return { indexes, bytes: packBinaryObject("fonts", sections) };
}
