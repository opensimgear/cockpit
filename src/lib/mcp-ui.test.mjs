import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { compile } from 'svelte/compiler';

test('AI settings compile with labeled access controls, honest native status, and one-time token actions', async () => {
  const source = await readFile(new URL('./McpSettings.svelte', import.meta.url), 'utf8');
  const result = compile(source, { filename: 'McpSettings.svelte', generate: 'client' });
  assert.deepEqual(result.warnings, []);
  assert.match(source, /aria-label="Enable MCP server"/);
  assert.match(source, /value="read">Read only/);
  assert.match(source, /value="write">Configure/);
  assert.match(source, /mcpStatusLabel\(preferences\)/);
  assert.match(source, /preferences\.status\?\.enabled && !preferences\.status\.running/);
  assert.match(source, /Open the Cockpit desktop app/);
  assert.match(source, /aria-label="New MCP token"/);
  assert.match(source, /Copy token/);
  assert.match(source, /Hide token/);
  assert.match(source, /Copy client configuration/);
  assert.match(source, /preferences\.secret\.token/);
  assert.match(source, /settings\.dispose\(\)/);
  assert.doesNotMatch(source, /localStorage|sessionStorage|console\./);
});

test('leaving AI settings unmounts the component so an outstanding token is forgotten', async () => {
  const source = await readFile(new URL('./SettingsPanel.svelte', import.meta.url), 'utf8');
  assert.deepEqual(compile(source, { filename: 'SettingsPanel.svelte', generate: 'client' }).warnings, []);
  assert.match(source, /id: 'ai', label: 'AI integration'/);
  assert.match(source, /role="tabpanel" aria-labelledby="settings-tab-ai"/);
  assert.match(source, /\{#if selected === 'ai'\}<McpSettings \/>\{\/if\}/);
});
