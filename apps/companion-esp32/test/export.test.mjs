import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { exportPanel, main } from '../tools/export-panel.mjs';
import { normalizeToPortableUi, canonical } from '../../../packages/companion-designer/src/model.ts';
import { publishDocument } from '../../../packages/companion-designer/src/assets.ts';
const source = await readFile(new URL('./panel.bundle.json', import.meta.url), 'utf8');
const change = mutate => { const b = JSON.parse(source); mutate(b.document, b); b.document.ui = normalizeToPortableUi(b.document); return JSON.stringify(b); };
test('shared designer publication keeps source identity, channels, pages and immutable output', async () => {
  const result = await exportPanel(source);
  const b = JSON.parse(source); const publication = await publishDocument(b.document, []);
  assert.equal(result.publicationHash, publication.publicationHash);
  const live = JSON.parse(await readFile(new URL("./live-publication.json", import.meta.url), "utf8"));
  assert.equal(live.canonicalDocument, canonical(publication.document));
  assert.equal(live.publicationHash, publication.publicationHash);
  assert.equal(live.capabilityFingerprint, publication.capabilityFingerprint);
  assert.equal(result.header, (await readFile(new URL('./panel.h', import.meta.url), 'utf8')).replaceAll('\r\n', '\n'));
  assert.match(result.header, /radio-channel/); assert.match(result.header, /master/);
  assert.match((await exportPanel(source, { width: 320, height: 240 })).header, /OSG_GAUGE/);
  assert.match((await exportPanel(source, { width: 480, height: 800 })).header, /OSG_NUMERIC/);
});
test('strict designer validation, target budgets and capabilities reject unsupported documents', async () => {
  for (const target of [{width:0,height:480},{width:800,height:63},{width:4097,height:480},{width:800,height:4097},{width:1.5,height:480}]) await assert.rejects(exportPanel(source, target), /dimensions/);
  await assert.rejects(exportPanel(' '.repeat(65537)), /64 KiB/);
  await assert.rejects(exportPanel('{}'));
  await assert.rejects(exportPanel(change(d => d.schemaVersion = 99)), /schema/);
  await assert.rejects(exportPanel(source, {width:64,height:64}), /44 pixels/);
  await assert.rejects(exportPanel(change(d => d.presentation.widgets[0].label = 'Knöpfchen')), /ASCII/);
  await assert.rejects(exportPanel(change(d => d.presentation.widgets[0].config.buttonMode = 'toggle')), /momentary/);
  await assert.rejects(exportPanel(change(d => d.presentation.groups.push({id:'group',pageId:'main-page',label:'Group'}))), /groups/);
  await assert.rejects(exportPanel(change(d => d.presentation.widgets[1].bounds = {...d.presentation.widgets[0].bounds})), /Overlapping/);
  await assert.rejects(exportPanel(change(d => {
    d.presentation.widgets[1].kind = 'icon'; d.presentation.widgets[1].config = {symbol:'power'};
  })), /unsupported widget/);
  await assert.rejects(exportPanel(change(d => {
    for (let i=2;i<5;i++) {
      const pageId='page-'+i;
      d.ui.pages.push({id:pageId,title:'Page '+i,items:[]});
      d.presentation.viewports.push({pageId,width:800,height:480});
      // Keep every page publishable so this case reaches the embedded page budget.
      d.presentation.widgets[i-1].pageId=pageId;
    }
  })), /budget/);
});
test('supported styles, safe insets and text affixes retain source publication changes', async () => {
  const next = change(d => {d.presentation.viewports[0].safeInsets={left:.02,right:.02,top:.02,bottom:.02}; d.presentation.widgets[0].style={foreground:'#ffffff',background:'#000000',accent:'#ff0000'};});
  const a = await exportPanel(source), b = await exportPanel(next);
  assert.notEqual(a.publicationHash,b.publicationHash); assert.match(b.header,/0xf800/);
});
test('CLI rejects missing files and arguments and writes only complete validated output', async () => {
  await assert.rejects(main([]), /Usage/); await assert.rejects(main(['missing-file','out.h','800','480']));
  const dir = await mkdtemp(join(tmpdir(), 'osg-esp32-'));
  try {
    const file = join(dir,'panel.h'); const input = new URL('./panel.bundle.json',import.meta.url);
    await main([(await import('node:url')).fileURLToPath(input),file,'800','480']);
    assert.equal(await readFile(file,'utf8'), (await exportPanel(source)).header);
  } finally {await rm(dir,{recursive:true,force:true});}
});
