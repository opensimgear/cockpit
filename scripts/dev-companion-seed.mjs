// Compile the editable DCS WIN Companion example into its development workspace seed.
import { readFile, writeFile } from 'node:fs/promises';
import { compilePanelDraft } from '../src/lib/compiled-companion-library.ts';

const toggle = JSON.parse(await readFile(new URL('../examples/companion/generic-toggle.source.json', import.meta.url), 'utf8'));
const { sources: panelSources, draft } = await import('../examples/dcs-bios/win-companion/panel.mjs');
const sources = [toggle, ...panelSources];
const publication = await compilePanelDraft(draft, sources, { motion: false, audio: false });
const output = `${JSON.stringify({ sources, drafts: [draft], publications: { [draft.id]: [publication] } })}\n`;
const path = new URL('../examples/dcs-bios/win-companion/workspace.json', import.meta.url);

if (process.argv.includes('--check')) {
  const actual = (await readFile(path, 'utf8')).replaceAll('\r\n', '\n');
  if (actual !== output) throw new Error('DCS Companion workspace seed differs from the current source model.');
} else {
  await writeFile(path, output);
}
