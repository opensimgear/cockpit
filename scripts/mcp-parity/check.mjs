import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { discover } from './actions.mjs';
import { originActions } from './baseline.mjs';

export const key = action => JSON.stringify([action.path, action.kind, action.anchor]);
export function tree(root) {
  const files = new Map();
  function visit(dir) {
    for (const item of readdirSync(dir, { withFileTypes: true })) {
      if (item.name.startsWith('.') || ['node_modules', 'target', 'dist', 'third_party'].includes(item.name)) continue;
      const path = join(dir, item.name);
      if (item.isDirectory()) visit(path);
      else if (/\.(?:svelte|rs|ts|mjs)$/.test(path)) files.set(relative(root, path).replaceAll('\\', '/'), readFileSync(path, 'utf8'));
    }
  }
  for (const name of ['src','src-tauri/src','tests','scripts','packages','apps','crates']) {
    if (existsSync(join(root, name))) visit(join(root, name));
  }
  return files;
}
export function catalogue(files) {
  const names = new Set();
  for (const [path, text] of files) {
    if (!/^src-tauri\/src\/mcp_tools(?:\/|\.rs)/.test(path)) continue;
    for (const match of text.matchAll(/\btool\(\s*"([a-z][a-z0-9_]*)"/g)) names.add(match[1]);
    for (const loop of text.matchAll(/for\s*\(\s*name\s*,\s*description\s*\)\s+in\s*\[([\s\S]*?)\]\s*\{\s*tools\.push\(tool\(name\s*,/g)) {
      for (const tuple of loop[1].matchAll(/\(\s*"([a-z][a-z0-9_]*)"\s*,/g)) names.add(tuple[1]);
    }
  }
  return names;
}
export function check(files, rows, baseline, contracts, indexedTools = [], requiredWorkflows = []) {
  const errors = [], actions = discover(files), actual = new Set(actions.map(key));
  const old = new Set(baseline.map(key)), tools = catalogue(files), workflows = new Set();
  const fields = ['workflow','ui','native','mcp','status','input','output','effect','guard','lifecycle','scenario'];
  for (const row of rows) {
    for (const field of fields) if (typeof row[field] !== 'string' || !row[field].trim()) errors.push(`${row.workflow || '<unnamed>'}: missing ${field}`);
    if (workflows.has(row.workflow)) errors.push(`Duplicate workflow: ${row.workflow}`);
    workflows.add(row.workflow);
    if (!['supported','partial','missing'].includes(row.status)) errors.push(`${row.workflow}: invalid status ${row.status}; use supported, partial, or missing`);
    for (const field of ['ui','native','mcp','scenario']) {
      for (const match of (row[field] || '').matchAll(/(?:src(?:-tauri)?|tests|scripts|packages|crates|apps)\/[\w./-]+\.(?:svelte|rs|ts|mjs)/g)) {
        if (!files.has(match[0])) errors.push(`${row.workflow}: stale ${field} path ${match[0]}`);
      }
    }
  }
  const mapped = new Set();
  for (const workflow of requiredWorkflows) if (!workflows.has(workflow)) errors.push(`Missing reviewed inventory workflow: ${workflow}; restore its row or explicitly review retirement in workflows.json`);
  for (const workflow of workflows) if (!requiredWorkflows.includes(workflow) && !contracts.some(contract => contract.workflow === workflow)) errors.push(`New inventory workflow ${workflow} requires a linked MCP/proposed contract and behavioral test`);
  for (const contract of contracts) {
    const before = errors.length;
    if (!workflows.has(contract.workflow)) errors.push(`Stale contract workflow: ${contract.workflow}`);
    if (!contract.tools?.length && !contract.proposedTools?.length) errors.push(`${contract.workflow}: missing MCP contract`);
    for (const tool of contract.tools || []) if (!tools.has(tool)) errors.push(`${contract.workflow}: absent MCP tool ${tool}`);
    for (const proposed of contract.proposedTools || []) {
      for (const field of ['name','input','output','effect','guard','lifecycle']) {
        if (typeof proposed[field] !== 'string' || !proposed[field].trim()) errors.push(`${contract.workflow}: proposed MCP contract missing ${field}`);
      }
      if (tools.has(proposed.name)) errors.push(`${contract.workflow}: proposed tool ${proposed.name} now exists; register its actual contract`);
      if (!rows.some(row => row.workflow === contract.workflow && ['partial','missing'].includes(row.status))) errors.push(`${contract.workflow}: proposed contract requires partial or missing status`);
    }
    if (!contract.tests?.length) errors.push(`${contract.workflow}: missing behavioral test`);
    for (const test of contract.tests || []) {
      const source = files.get(test.path);
      if (!source || !test.anchor || !source.includes(test.anchor)) errors.push(`${contract.workflow}: stale behavioral test ${test.path}#${test.anchor || ''}`);
      else if (!/(?:assert[.(]|expect\(|assert[!_])/.test(source)) errors.push(`${contract.workflow}: test has no behavioral assertion ${test.path}`);
    }
    for (const action of contract.actions || []) {
      if (!actual.has(key(action))) errors.push(`${contract.workflow}: stale action ${key(action)}`);
    }
    if (errors.length === before) for (const action of contract.actions || []) mapped.add(key(action));
  }
  for (const action of actions) if (!old.has(key(action)) && !mapped.has(key(action))) errors.push(`Unmapped UI/native action: ${key(action)}; add inventory, MCP contract, and behavioral test`);
  for (const tool of indexedTools) if (!tools.has(tool)) errors.push(`Stale MCP catalogue index: ${tool}`);
  for (const tool of tools) if (!indexedTools.includes(tool) && !contracts.some(contract => contract.tools?.includes(tool))) errors.push(`Unmapped MCP catalogue tool: ${tool}`);
  return errors;
}
export function main(args = process.argv.slice(2), dependencies = {}) {
  const root = resolve(args[0] || '.');
  const files = tree(root), doc = join(root, 'docs/mcp-parity');
  const read = path => JSON.parse(readFileSync(path, 'utf8'));
  const inventoryFiles = readdirSync(doc).filter(name => /^inventory-.*\.json$/.test(name)).sort();
  if (!inventoryFiles.length) throw Error('No parity inventories found');
  const rows = inventoryFiles.flatMap(name => {
    const inventory = read(join(doc, name));
    if (!Array.isArray(inventory) || !inventory.length) throw Error(`Empty parity inventory ${name}; restore the reviewed rows`);
    return inventory;
  });
  const baseline = dependencies.originActions || originActions(root);
  const contracts = read(join(doc, 'contracts.json'));
  const errors = check(files, rows, baseline, contracts, read(join(doc, 'catalogue.json')), read(join(doc, 'workflows.json')));
  if (errors.length) { console.error(errors.join('\n')); return 1; }
  console.log(`MCP parity: ${rows.length} workflows, ${baseline.length} action anchors, ${contracts.length} behavioral contracts checked.`);
  return 0;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) process.exitCode = main();
