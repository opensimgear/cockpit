import test from 'node:test';
import assert from 'node:assert/strict';
import { widgetTextLayout, widgetUnsupportedGlyphs } from './widget-fonts.ts';

test('portable text keeps deterministic glyph positions and baselines across alignments', () => {
  for (const face of ['sans', 'mono']) for (const size of [8, 14, 22, 36, 128]) {
    const left = widgetTextLayout('ENGINE 50.0 % —', face, size, 'left', 200);
    for (const align of ['center', 'right']) {
      const shifted = widgetTextLayout('ENGINE 50.0 % —', face, size, align, 200);
      const offset = (200 - left.width) / (align === 'center' ? 2 : 1);
      assert.equal(shifted.width, left.width); assert.equal(shifted.baseline, left.baseline);
      assert.deepEqual(shifted.glyphs.map(g => g.x), left.glyphs.map(g => g.x + offset));
    }
    assert.equal(left.glyphs.map(g => g.character).join(''), 'ENGINE 50.0 % —');
    assert.ok(left.glyphs.every(g => g.supported));
    assert.ok(left.width > 0 && Number.isFinite(left.baseline));
  }
  const mono = widgetTextLayout('Wmi1.', 'mono', 22, 'left', 100).glyphs;
  assert.ok(mono.slice(1).every((g, i) => g.x - mono[i].x === mono[1].x));
});

test('missing glyph diagnostics preserve the original browser text and remain independent', () => {
  assert.deepEqual(widgetUnsupportedGlyphs('ENGINE', 'sans'), []);
  assert.deepEqual(widgetUnsupportedGlyphs('A🛩🛩B', 'sans'), ['🛩']);
  const layout = widgetTextLayout('A🛩B', 'sans', 22, 'left', 100);
  assert.equal(layout.glyphs.map(g => g.character).join(''), 'A🛩B');
  assert.equal(layout.glyphs[1].supported, false);
  assert.equal(widgetTextLayout('', 'mono', 22, 'right', 100).width, 0);
  layout.glyphs[0].x = 999;
  assert.equal(widgetTextLayout('A🛩B', 'sans', 22, 'left', 100).glyphs[0].x, 0);
});
