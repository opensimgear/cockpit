// Exercise the actual running native Cockpit. No provider, IPC or storage mocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const modulePath = process.env.OSG_PLAYWRIGHT_MODULE ?? 'playwright';
const { chromium } = require(modulePath), { expect } = require(modulePath + '/test');
const artifacts = resolve('.research/panel-list-native');
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64');

test('native panel list duplicates independently and initializes custom or companion screen sizes', { timeout: 120_000 }, async () => {
  const browser = await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL ?? 'http://127.0.0.1:9222', { timeout: 15_000 });
  const page = browser.contexts().flatMap(context => context.pages()).find(page => /127\.0\.0\.1:1420|localhost:1420|tauri\.localhost/.test(page.url()));
  assert.ok(page, 'Start the native dev Cockpit before running this test.');
  await page.reload();
  await mkdir(artifacts, { recursive: true });
  const errors = [], created = new Map(), checks = [], name = `Panel list acceptance ${Date.now()}`;
  page.on('pageerror', error => errors.push(error.message));
  const snapshot = () => page.evaluate(async () => {
    const { CompanionLibraryStore, indexedDbStorage } = await import('/src/lib/companion-library.ts');
    return new CompanionLibraryStore(indexedDbStorage()).load();
  });
  const native = async command => {
    const response = await page.evaluate(command => window.__TAURI_INTERNALS__.invoke('gateway_request', { request: { command, clientId: 'panel-list-native' } }), command);
    assert.equal(response.ok, true, response.error); return response.state.companions;
  };
  const editor = page.getByRole('region', { name: 'Panel editor', exact: true });
  const panels = page.getByRole('region', { name: 'Panels', exact: true });
  const before = await snapshot();
  const previousIds = new Set(before.library.drafts.map(entry => entry.id));

  async function list() {
    const discard = page.getByRole('button', { name: 'Discard and continue', exact: true });
    if (await discard.isVisible().catch(() => false)) await discard.click();
    const newDialog = page.getByRole('dialog', { name: 'New panel', exact: true });
    if (await newDialog.isVisible().catch(() => false)) await newDialog.getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('button', { name: 'Companions', exact: true }).click();
    if (await discard.isVisible().catch(() => false)) await discard.click();
    const back = page.getByRole('button', { name: 'Back to Panels', exact: true });
    if (await back.isVisible().catch(() => false)) await back.click();
    if (await discard.isVisible().catch(() => false)) await discard.click();
    await expect(panels.getByRole('heading', { name: 'Panels', exact: true })).toBeVisible();
  }
  async function rename(value) {
    await editor.getByRole('button', { name: 'Edit panel name', exact: true }).click();
    await editor.getByLabel('Panel name', { exact: true }).fill(value);
    await editor.getByLabel('Panel name', { exact: true }).press('Enter');
  }
  async function save(value) {
    await editor.getByRole('button', { name: 'Save', exact: true }).click();
    await expect(editor.getByRole('button', { name: 'Save', exact: true })).toBeEnabled({ timeout: 30_000 });
    await expect(page.getByText(/^Panel saved locally\. Companion update failed:/)).toHaveCount(0);
    await expect.poll(async () => (await snapshot()).library.drafts.some(entry => entry.name === value)).toBe(true);
    const entry = (await snapshot()).library.drafts.find(entry => entry.name === value);
    assert.ok(!previousIds.has(entry.id)); created.set(entry.id, entry.name); return entry;
  }
  async function createDialog() {
    await panels.getByRole('button', { name: 'New panel', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'New panel', exact: true });
    await expect(dialog).toBeVisible(); return dialog;
  }
  try {
    assert.equal(await page.evaluate(() => typeof window.__TAURI_INTERNALS__?.invoke), 'function');
    await list();
    const dialog = await createDialog();
    await dialog.getByLabel('Initial screen size', { exact: true }).selectOption('custom');
    await dialog.getByLabel('Panel width', { exact: true }).fill('900');
    await dialog.getByLabel('Panel height', { exact: true }).fill('600');
    await dialog.getByRole('button', { name: 'Create panel', exact: true }).click();
    await expect(editor).toBeVisible();
    await expect(editor.getByLabel('Screen width', { exact: true })).toHaveValue('900');
    await expect(editor.getByLabel('Screen height', { exact: true })).toHaveValue('600');
    await rename(name);
    await editor.getByText('Widgets', { exact: true }).click();
    await editor.locator('.palette').getByRole('button', { name: 'Button', exact: true }).click();
    await editor.getByLabel('Add panel image', { exact: true }).setInputFiles({ name: 'acceptance.png', mimeType: 'image/png', buffer: png });
    const iconImage = editor.getByRole('combobox', { name: /^Icon image/ });
    await expect(iconImage.locator('option')).toHaveCount(2);
    const assetId = await iconImage.locator('option').last().getAttribute('value');
    await iconImage.selectOption(assetId);
    const source = await save(name);
    checks.push('New custom panel initializes the editor and full-screen viewport to 900 × 600');
    assert.deepEqual(source.document.presentation.viewports.map(viewport => [viewport.width, viewport.height]), [[900, 600]]);
    assert.equal(source.assets.length, 1);
    await list();

    const tile = panels.locator('.panel-tile-container').filter({ has: page.getByRole('heading', { name, exact: true }) });
    await expect(tile).toHaveCount(1);
    const positions = await tile.evaluate(element => {
      const title = element.querySelector('h3'), preview = element.querySelector('.panel-thumbnail'), details = element.querySelector('.tile-copy');
      return { title: title.getBoundingClientRect().bottom, previewTop: preview.getBoundingClientRect().top,
        previewBottom: preview.getBoundingClientRect().bottom, detailsTop: details.getBoundingClientRect().top };
    });
    assert.ok(positions.title <= positions.previewTop && positions.previewBottom <= positions.detailsTop,
      `Panel tiles must show title, preview, then details: ${JSON.stringify(positions)}`);
    while (await page.getByRole('button', { name: 'Dismiss notification', exact: true }).count()) await page.getByRole('button', { name: 'Dismiss notification', exact: true }).first().click();
    await panels.screenshot({ path: resolve(artifacts, 'panel-list.png') });
    const assignments = (await native('companion_status')).host?.instances ?? [];
    await tile.getByRole('button', { name: `Duplicate ${name}`, exact: true }).click();
    await expect.poll(async () => (await snapshot()).library.drafts.filter(entry => !previousIds.has(entry.id) && entry.id !== source.id).length).toBe(1);
    const afterDuplicate = await snapshot(), copy = afterDuplicate.library.drafts.find(entry => !previousIds.has(entry.id) && entry.id !== source.id);
    created.set(copy.id, copy.name);
    assert.deepEqual(afterDuplicate.library.drafts.find(entry => entry.id === source.id), source, 'Duplicating must preserve the original.');
    assert.equal(copy.name, `${name} copy`);
    assert.equal(copy.document.ui.title, copy.name);
    assert.notEqual(copy.document.ui.id, source.document.ui.id);
    assert.notEqual(copy.document.ui.pages[0].id, source.document.ui.pages[0].id);
    assert.notEqual(copy.document.presentation.widgets[0].id, source.document.presentation.widgets[0].id);
    assert.notEqual(copy.document.ui.controls[0].id, source.document.ui.controls[0].id);
    assert.notEqual(copy.assets[0].assetId, source.assets[0].assetId);
    assert.equal(copy.assets[0].base64, source.assets[0].base64);
    assert.equal(copy.document.presentation.widgets.at(-1).config.iconAssetId, copy.assets[0].assetId);
    assert.deepEqual(copy.document.presentation.widgets.at(-1).bounds, source.document.presentation.widgets.at(-1).bounds);
    const copyPublications = afterDuplicate.publications[copy.id] ?? [];
    assert.equal(copyPublications.length, 1, 'A saved copy must have its own current publication so it can be assigned immediately.');
    assert.equal(copyPublications[0].document.ui.id, copy.document.ui.id);
    assert.equal(copyPublications[0].document.ui.title, copy.name);
    assert.ok(!(afterDuplicate.publications[source.id] ?? []).some(publication => publication.publicationHash === copyPublications[0].publicationHash),
      'A copy must have an independent publication hash.');
    const host = await native('companion_status');
    assert.deepEqual(host.host?.instances ?? [], assignments, 'Duplicating must not change or inherit companion assignments.');
    const pairedCompanion = host.host?.clients?.find(client => typeof client.name === 'string' && client.name);
    assert.ok(pairedCompanion, 'A paired companion is required to check the actual assignment selector.');
    await page.getByRole('region', { name: 'Companions', exact: true }).getByRole('button', { name: `Edit ${pairedCompanion.name}`, exact: true }).click();
    const assignmentOption = page.getByRole('combobox', { name: /^Assigned panel/ }).locator('option').filter({ hasText: copy.name });
    await expect(assignmentOption).toHaveCount(1);
    await expect(assignmentOption).toHaveAttribute('value', copyPublications[0].publicationHash);
    await page.getByRole('button', { name: 'Back to Companions', exact: true }).click();
    checks.push('Duplicate copies layout and image bytes with fresh identities and its own assignable publication, preserves its source, and inherits no companion assignment');

    await list();
    const companionDialog = await createDialog(), screenSource = companionDialog.getByLabel('Initial screen size', { exact: true });
    const choices = await screenSource.locator('option').evaluateAll(options => options.filter(option => option.value !== 'custom' && !option.disabled).map(option => ({ value: option.value, text: option.textContent })));
    assert.ok(choices.length, 'A paired companion with a known screen size is required for native companion-size acceptance.');
    await screenSource.selectOption(choices[0].value);
    const dimensions = { width: Number(await companionDialog.getByLabel('Panel width', { exact: true }).inputValue()), height: Number(await companionDialog.getByLabel('Panel height', { exact: true }).inputValue()) };
    assert.ok(dimensions.width >= 320 && dimensions.height >= 320);
    await companionDialog.getByRole('button', { name: 'Create panel', exact: true }).click();
    await expect(editor.getByLabel('Screen width', { exact: true })).toHaveValue(String(dimensions.width));
    await expect(editor.getByLabel('Screen height', { exact: true })).toHaveValue(String(dimensions.height));
    const companionName = `${name} companion`; await rename(companionName);
    await editor.getByText('Widgets', { exact: true }).click();
    await editor.locator('.palette').getByRole('button', { name: 'Button', exact: true }).click();
    const companionPanel = await save(companionName);
    assert.deepEqual(companionPanel.document.presentation.viewports.map(viewport => [viewport.width, viewport.height]), [[dimensions.width, dimensions.height]]);
    checks.push('Choosing a real companion initializes the saved panel screen and viewport to its dimensions without assigning it');
    await list();
    assert.deepEqual(errors, []);
    await writeFile(resolve(artifacts, 'evidence.json'), JSON.stringify({ actualNativeCockpit: true, checks, sourceId: source.id, copyId: copy.id, copyPublicationHash: copyPublications[0].publicationHash, companionChoice: choices[0], dimensions, errors }, null, 2) + '\n');
  } catch (error) {
    await writeFile(resolve(artifacts, 'failure.txt'), String(error.stack ?? error));
    await page.screenshot({ path: resolve(artifacts, 'failure.png'), fullPage: true }); throw error;
  } finally {
    try {
      await list();
      // Only exact identities created in this run are eligible for UI deletion.
      // Never infer cleanup eligibility from a user panel's display name alone.
      const current = await snapshot();
      for (const [id, title] of created) {
        assert.ok(!previousIds.has(id));
        if (!current.library.drafts.some(entry => entry.id === id && entry.name === title)) continue;
        const matches = current.library.drafts.filter(entry => entry.name === title);
        assert.equal(matches.length, 1, 'Ambiguous panel name; preserve it instead of risking unrelated data.');
        await panels.getByRole('button', { name: `Delete ${title}`, exact: true }).click();
        await page.getByRole('alertdialog', { name: 'Confirm deletion', exact: true }).getByRole('button', { name: 'Delete', exact: true }).click();
        await expect.poll(async () => (await snapshot()).library.drafts.some(entry => entry.id === id)).toBe(false);
      }
      const final = await snapshot();
      for (const entry of before.library.drafts) assert.deepEqual(final.library.drafts.find(candidate => candidate.id === entry.id), entry, 'Acceptance must preserve existing panels.');
    } finally { await browser.close(); }
  }
});
