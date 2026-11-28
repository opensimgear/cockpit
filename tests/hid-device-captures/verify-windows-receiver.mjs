// Opt-in test against the installed VKB pair and the native development Cockpit.
import assert from 'node:assert/strict';
import {mkdtemp,readFile,rm} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const page = await nativePage();
async function request(command, args = {}) {
  const payload = JSON.stringify({command:'controller_hid_' + command, ...args});
  return page.evaluate(`(async () => {
    const {gatewayRequest} = await import('/src/lib/gateway.ts');
    return (await gatewayRequest(${payload})).controllers;
  })()`);
}
try {
  // Position can change between archival and current captures. Use the archived
  // capabilities for layout and an independent Raw Input delivery for wire equality.
  const dir = await mkdtemp(join(tmpdir(),'cockpit-hid-probe-'));
  let independent;
  try {
    execFileSync('powershell.exe',['-NoProfile','-File',fileURLToPath(new URL('capture-raw.ps1',import.meta.url)),
      '-OutputDirectory',dir]);
    independent = JSON.parse((await readFile(join(dir,'raw-input.json'),'utf8')).replace(/^\uFEFF/,''));
  } finally { await rm(dir,{recursive:true,force:true}); }
  assert.equal(independent.physicalOverflow,false);
  const inventory = await request('list');
  const restricted = inventory.interfaces.filter(row => row.access === 'metadata-only');
  const other = inventory.interfaces.filter(row => row.usagePage !== null &&
    !(row.usagePage === 1 && [4,5,8].includes(row.usage)));
  console.log(JSON.stringify({interfaces:inventory.interfaces.length, metadataOnly:restricted.length,
    nonController:other.length, truncated:inventory.truncated}));
  assert.ok(restricted.length && other.length);
  const policy = await request('inspect',{interfaceId:restricted[0].interfaceId});
  assert.equal(policy.access,'metadata-only');
  const vendor = other.find(row => row.usagePage >= 0xff00);
  if (vendor) {
    const detail = await request('inspect',{interfaceId:vendor.interfaceId});
    assert.equal(detail.capabilities.usagePage,vendor.usagePage);
    console.log(JSON.stringify({vendorUsagePage:vendor.usagePage,usage:vendor.usage,access:detail.access}));
  }
  for (const row of inventory.interfaces.filter(r => r.metadataOpened && r.interfaceId)) {
    const detail = await request('inspect',{interfaceId:row.interfaceId});
    const multi = detail.capabilities?.reports.find(r => r.reportIds.length > 1);
    if (multi) {
      console.log(JSON.stringify({multipleReportIds:multi.reportIds,kind:multi.kind,usagePage:row.usagePage,usage:row.usage}));
      break;
    }
  }
  for (const pid of ['011f','0200']) {
    const fixture = JSON.parse(await readFile(new URL(`231d-${pid}-idle.json`,import.meta.url),'utf8'));
    const row = inventory.interfaces.find(row => row.attributes?.vendorId === fixture.device.vendorId &&
      row.attributes.productId === fixture.device.productId && row.usagePage === fixture.device.usagePage &&
      row.usage === fixture.device.usage);
    assert.ok(row, 'Fixture top-level collection is present');
    const detail = await request('inspect', {interfaceId:row.interfaceId});
    const caps = detail.capabilities.reports.find(r => r.kind === 'input');
    for (const button of caps.buttonCaps) {
      if (detail.capabilities.apiVersion.number >= 2) assert.ok(Number.isInteger(button.reportCount));
      else assert.equal(button.reportCount,null);
    }
    const expected = fixture.rawInput.states[0].reportHex;
    const matched = independent.devices.filter(d => d.vendor === fixture.device.vendorId &&
      d.product === fixture.device.productId && !d.virtualVjoy);
    assert.equal(matched.length,1);
    const current = new Set(independent.physicalReports.filter(r => r.device === matched[0].id)
      .map(r => r.reportHex.toLowerCase()));
    assert.equal(caps.byteLength, expected.length / 2);
    assert.ok(caps.reportIds.includes(parseInt(expected.slice(0,2),16)));
    for (const field of fixture.descriptor.osParsedCapabilities) {
      const candidates = field.kind === 'button' ? caps.buttonCaps : caps.valueCaps;
      assert.ok(candidates.some(c => c.reportId === field.reportId && c.linkCollection === field.collection &&
        c.usagePage === field.usagePage && field.dataIndex >= c.dataIndices.minimum &&
        field.dataIndex <= c.dataIndices.maximum && field.usage >= c.usages.minimum &&
        field.usage <= c.usages.maximum), 'Archived data index resolves within the current HID capabilities');
    }
    const opened = await request('open', {interfaceId:row.interfaceId});
    try {
      const result = await until(() => request('reports', opened), p => p.result.reports.length >= 2);
      const reports = result.result.reports;
      assert.equal(result.result.gap, false);
      for (const [i,record] of reports.entries()) {
        assert.equal(record.wireHex.length, caps.byteLength * 2);
        assert.equal(record.sequence, String(i + 1));
        assert.equal(record.windowsReportId, parseInt(record.wireHex.slice(0,2),16));
        assert.equal(record.samples.raw,record.wireHex);
        assert.ok(record.samples.data.some(entry => entry.matchingValueCaps.length));
      }
      const matches = reports.filter(r => current.has(r.wireHex)).length;
      assert.ok(matches > 0, 'At least one report matches a current independent Raw Input delivery byte for byte');
      console.log(JSON.stringify({product:row.product, reports:reports.length, exactRawInputMatches:matches,
        archivedIdleMatches:reports.filter(r => r.wireHex === expected).length,
        bytes:caps.byteLength, reportIds:caps.reportIds}));
    } finally { await request('close', opened); }
  }
} finally { page.close(); }
