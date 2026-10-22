import bank from '../assets/fonts/metrics.json' with { type: 'json' };

export type WidgetFont = 'sans' | 'mono';
const advances = Object.fromEntries(Object.entries(bank.fonts).map(([name, font]) => [name, new Map(font.advances as [number, number][])])) as Record<WidgetFont, Map<number, number>>;
/** Exactly the unkerned pixel advances used by the pinned LVGL TinyTTF backend. */
export function widgetTextLayout(text: string, face: WidgetFont, size: number, align: 'left' | 'center' | 'right', width: number) {
  const font = bank.fonts[face];
  const scale = Math.fround(size / font.unitsPerEm);
  const lineHeight = Math.trunc(Math.fround(scale * (font.ascent - font.descent + font.lineGap)));
  const baseline = Math.trunc(Math.fround(scale * (font.lineGap - font.descent)));
  let cursor = 0;
  const glyphs = [...text].map(character => {
    const advance = advances[face].get(character.codePointAt(0)!);
    const x = cursor; cursor += Math.trunc(Math.fround(scale * (advance ?? font.missingAdvance)) + .5);
    return { character, x, supported: advance !== undefined };
  });
  const offset = align === 'center' ? (width - cursor) / 2 : align === 'right' ? width - cursor : 0;
  return { glyphs: glyphs.map(glyph => ({ ...glyph, x: glyph.x + offset })), width: cursor, baseline: lineHeight / 2 - baseline };
}
export function widgetUnsupportedGlyphs(text: string, face: WidgetFont): string[] {
  return [...new Set([...text].filter(character => !advances[face].has(character.codePointAt(0)!)))];
}
