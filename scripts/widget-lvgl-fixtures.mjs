import { readFile, writeFile } from 'node:fs/promises';
import { importBundle, resolveWidgetLayers } from '../packages/companion-designer/src/index.ts';
import { componentDefinition, componentSamples, widgetLayerText, widgetIndicatorActive, widgetGaugeFraction } from '../packages/companion-ui/src/widget-presentation.ts';

const { document } = await importBundle(await readFile('examples/companion/shared-systems.bundle.json', 'utf8'));
const states = ['normal', 'pressed', 'active', 'warning', 'disabled', 'unknown', 'stale', 'disconnected'];
const frames = states.map(state => {
  const nodes = [];
  for (const widget of document.presentation.widgets) {
    const definition = componentDefinition(document, widget);
    const samples = Object.fromEntries(definition.outputs.map((p, index) => {
      const output = document.ui.feedbackWidgets.find(f => f.id === widget.feedbackIds[index]);
      return [output.channelId, { outputId: output.id, channelId: output.channelId, value: p.kind === 'indicator' ? state === 'active' : p.kind === 'gauge' ? 50 : 'Sample', unit: p.unit ?? 'ratio', ...(p.range ? { range: p.range } : {}), status: ['stale', 'unknown', 'disconnected'].includes(state) ? state : 'fresh', provenance: 'game-confirmed', sourceKind: 'simulator', sourceAgeMs: 0, receivedAtMs: 100 }];
    }));
    const outputs = componentSamples(document, widget, definition, samples);
    for (const layer of resolveWidgetLayers(definition, widget.config.properties, state)) {
      if (layer.visible === false) continue;
      const b = layer.bounds, w = widget.bounds;
      const area = [Math.round((w.x + b.x * w.width) * 800), Math.round((w.y + b.y * w.height) * 480), Math.round(b.width * w.width * 800), Math.round(b.height * w.height * 480)];
      nodes.push({ id: widget.id + '/' + layer.id, kind: layer.kind, area, fill: layer.fill ?? null, stroke: layer.stroke ?? null, strokeWidth: Number(layer.strokeWidth ?? 0), opacity: Number(layer.opacity ?? 1),
        ...(layer.kind === 'rectangle' ? { radius: Number(layer.radius ?? 0) } : {}),
        ...(layer.kind === 'text' ? { text: widgetLayerText(layer, outputs, 100), align: layer.align, font: layer.font, fontSize: layer.fontSize } : {}),
        ...(layer.kind === 'indicator' ? { active: widgetIndicatorActive(layer, outputs, 100) } : {}),
        ...(layer.kind === 'gauge' ? { fraction: widgetGaugeFraction(layer, outputs, 100), orientation: layer.orientation } : {}),
      });
    }
  }
  return { state, nodes };
});
const path = 'apps/companion-esp32/test/widget-lvgl.json', text = JSON.stringify({ formatVersion: 1, width: 800, height: 480, frames }, null, 2) + '\n';
if (process.argv.includes('--check')) { if ((await readFile(path, 'utf8')).replaceAll('\r\n', '\n') !== text) throw Error('LVGL fixture does not match shared widget source'); }
else await writeFile(path, text);
