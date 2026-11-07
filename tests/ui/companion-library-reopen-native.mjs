import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_COMPANION_LIBRARY_BROWSER_URL;
test('Companion library reopens with the latest committed drafts', {
  skip: !browserUrl && process.env.OSG_COMPANION_LIBRARY_NATIVE !== '1', timeout: 60_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const panels = 'document.querySelector(\'[aria-label="Panels"]\')';
  const navigate = label => page.evaluate(`[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
    .find(button => button.title === ${JSON.stringify(label)})?.click()`);
  const card = name => page.evaluate(`[...${panels}?.querySelectorAll('button') ?? []]
    .some(button => button.getAttribute('aria-label') === ${JSON.stringify('Edit ' + name)} && button.checkVisibility())`);
  const name = `Reopened panel ${randomUUID()}`;
  const writeDraft = operation => page.evaluate(`import('/src/lib/compiled-companion-library.ts').then(async library => {
    const store = new library.CompiledWorkspaceStore();
    const workspace = await store.load();
    const name = ${JSON.stringify(name)};
    const drafts = workspace.drafts.filter(draft => draft.title !== name);
    if (${operation === 'add'}) drafts.push(library.newCompiledDraft(name));
    await store.save({...workspace, drafts});
  })`);
  try {
    await until(() => page.evaluate('!!document.querySelector(\'nav[aria-label="Main navigation"]\')'));
    await navigate('Companions');
    await until(() => page.evaluate(`[...${panels}?.querySelectorAll('button') ?? []]
      .some(button => button.textContent.trim() === 'New' && !button.disabled && button.checkVisibility())`));
    assert.equal(await card(name), false);
    await navigate('Overview');
    await until(() => page.evaluate(`!${panels}?.checkVisibility()`));
    await writeDraft('add');
    await navigate('Companions');
    await until(() => card(name));
    await navigate('Overview');
    await until(() => page.evaluate(`!${panels}?.checkVisibility()`));
    await writeDraft('remove');
    await navigate('Companions');
    await until(() => page.evaluate(`${panels}?.checkVisibility()`));
    await until(() => card(name), visible => !visible);
  } finally {
    try { await writeDraft('remove'); } finally { await page.close(); }
  }
});
