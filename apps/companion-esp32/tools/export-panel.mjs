import { readFile, writeFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';
import { importBundle, publishDocument } from '../../../packages/companion-designer/src/assets.ts';

export const LIMITS = Object.freeze({ pages: 4, widgets: 32, documentBytes: 65536 });
const text = (s, name, max = 120) => {
  if (typeof s !== 'string' || s.length > max || /[^A-Za-z0-9 .,:;_+=|/%!()?\-]/.test(s)) throw new Error(name + ': initial firmware requires supported ASCII glyphs within its text budget');
  return JSON.stringify(s);
};
const color = (s) => {
  const n = Number.parseInt(s.slice(1), 16);
  return '0x' + (((n >> 19 & 31) << 11) | ((n >> 10 & 63) << 5) | (n >> 3 & 31)).toString(16);
};
const number = n => Number.isInteger(n) ? String(n) + '.0' : String(n);
/** Export a validated designer bundle to an immutable C image. This is an offline
 * build tool, not a replacement wire protocol or an authenticated download. */
export async function exportPanel(serialized, target = { width: 800, height: 480 }) {
  if (!Number.isInteger(target.width) || !Number.isInteger(target.height) || target.width < 64 || target.height < 64 || target.width > 4096 || target.height > 4096) throw new Error('Invalid target dimensions');
  if (Buffer.byteLength(serialized) > LIMITS.documentBytes) throw new Error('Embedded bundle exceeds 64 KiB');
  const bundle = await importBundle(serialized);
  const publication = await publishDocument(bundle.document, bundle.assets);
  const d = publication.document;
  if (d.ui.pages.length > LIMITS.pages || d.presentation.widgets.length > LIMITS.widgets) throw new Error('Embedded page/widget budget exceeded');
  if (d.ui.assets.length) throw new Error('Image assets are not implemented by this firmware profile');
  if (d.presentation.groups.length) throw new Error('Visual groups are not implemented by this firmware profile');
  const kind = { button: 'OSG_BUTTON', indicator: 'OSG_INDICATOR', text: 'OSG_TEXT', numeric: 'OSG_NUMERIC', gauge: 'OSG_GAUGE' };
  const widgets = d.presentation.widgets.map(w => {
    if (!kind[w.kind]) throw new Error(w.id + ': unsupported widget ' + w.kind);
    if (w.kind === 'button' && w.config.buttonMode !== 'momentary') throw new Error(w.id + ': only momentary buttons are supported');
    const page = d.ui.pages.findIndex(p => p.id === w.pageId);
    const viewport = d.presentation.viewports.find(p => p.pageId === w.pageId);
    const inset = viewport.safeInsets ?? { left: 0, top: 0, right: 0, bottom: 0 };
    const b = { x: inset.left + w.bounds.x * (1 - inset.left - inset.right), y: inset.top + w.bounds.y * (1 - inset.top - inset.bottom), width: w.bounds.width * (1 - inset.left - inset.right), height: w.bounds.height * (1 - inset.top - inset.bottom) };
    const scale = Math.min(target.width / viewport.width, target.height / viewport.height);
    if (w.kind === 'button' && (Math.floor(b.width * viewport.width * scale) < 44 || Math.floor(b.height * viewport.height * scale) < 44)) throw new Error(w.id + ': touch target below 44 pixels at requested screen dimensions');
    const output = d.ui.feedbackWidgets.find(f => f.id === w.feedbackIds[0]);
    const c = w.config;
    const style = { foreground: '#dce7ef', background: '#152532', accent: '#83e3c2', ...w.style };
    return { w, b, page, line: '{ ' + [text(w.id, 'widget ID', 64), text(w.controlIds[0] ?? output.channelId, 'reference', 64), text(w.feedbackIds[0] ?? '', 'output ID', 64), text(w.label, 'label'), text(c.prefix ?? '', 'prefix'), text(c.suffix ?? '', 'suffix'), text(c.unit ?? '', 'unit', 32), kind[w.kind], page, c.precision ?? 0, '{ ' + Object.values(b).map(number).join(', ') + ' }', color(style.foreground), color(style.background), color(style.accent), number(c.min ?? 0), number(c.max ?? 1), "NULL", 0, 0].join(', ') + ' }' };
  });
  for (let i = 0; i < widgets.length; i++) for (let j = 0; j < i; j++) {
    const a = widgets[i], b = widgets[j];
    if (a.page === b.page && (a.w.kind === 'button' || b.w.kind === 'button') && a.b.x < b.b.x + b.b.width && b.b.x < a.b.x + a.b.width && a.b.y < b.b.y + b.b.height && b.b.y < a.b.y + a.b.height) throw new Error('Overlapping touch target: ' + a.w.id);
  }
  const pages = d.ui.pages.map(p => {
    const v = d.presentation.viewports.find(v => v.pageId === p.id);
    return '{ ' + [text(p.id, 'page ID', 64), text(p.title, 'page title'), v.width, v.height].join(', ') + ' }';
  });
  const header = ['/* Generated from a validated immutable Cockpit publication. Do not edit. */', '#ifndef OSG_GENERATED_PANEL_H', '#define OSG_GENERATED_PANEL_H', '#include "companion.h"', 'static const osg_page osg_demo_pages[] = {', pages.join(',\n'), '};', 'static const osg_widget osg_demo_widgets[] = {', widgets.map(w => w.line).join(',\n'), '};', 'static const osg_panel osg_demo_panel = { ' + [text(d.ui.id, 'UI ID', 64), text(publication.publicationHash, 'publication hash'), 'osg_demo_pages', 'osg_demo_widgets', pages.length, widgets.length].join(', ') + ' };', '#endif', ''].join('\n');
  return { header, publicationHash: publication.publicationHash, panel: d };
}
export async function main(args) {
  if (args.length !== 4) throw new Error('Usage: node tools/export-panel.mjs panel.bundle.json output.h width height');
  const [input, output, width, height] = args;
  const stat = await import('node:fs/promises').then(fs => fs.stat(input));
  if (stat.size > LIMITS.documentBytes) throw new Error('Embedded bundle exceeds 64 KiB');
  const result = await exportPanel(await readFile(input, 'utf8'), { width: Number(width), height: Number(height) });
  await writeFile(output, result.header);
  console.log(result.publicationHash);
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
