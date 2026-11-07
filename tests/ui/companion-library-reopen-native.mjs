import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import test from 'node:test';
import {browserPage, nativePage, until} from '../../scripts/reactive/cdp.mjs';

const browserUrl = process.env.OSG_COMPANION_LIBRARY_BROWSER_URL;
test('Companion library follows visible and hidden draft commits', {
  skip: !browserUrl && process.env.OSG_COMPANION_LIBRARY_NATIVE !== '1', timeout: 120_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const panels = 'document.querySelector(\'[aria-label="Panels"]\')';
  const navigate = label => page.evaluate(`[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
    .find(button => button.title === ${JSON.stringify(label)})?.click()`);
  const card = name => page.evaluate(`[...${panels}?.querySelectorAll('button') ?? []]
    .some(button => button.getAttribute('aria-label') === ${JSON.stringify('Edit ' + name)} && button.checkVisibility())`);
  const name = `Reopened panel ${randomUUID()}`;
  let writer = page;
  const writeDraft = operation => writer.evaluate(`import('/src/lib/compiled-companion-library.ts').then(async library => {
    const store = new library.CompiledWorkspaceStore();
    const workspace = await store.load();
    const name = ${JSON.stringify(name)};
    const drafts = workspace.drafts.filter(draft => draft.title !== name);
    if (${operation === 'add'}) drafts.push(library.newCompiledDraft(name));
    await store.save({...workspace, drafts});
  })`);
  try {
    if (browserUrl) writer = await page.openPeer();
    await until(() => page.evaluate('!!document.querySelector(\'nav[aria-label="Main navigation"]\')'));
    await navigate('Companions');
    await until(() => page.evaluate(`[...${panels}?.querySelectorAll('button') ?? []]
      .some(button => button.textContent.trim() === 'New' && !button.disabled && button.checkVisibility())`));
    assert.equal(await card(name), false);
    await writeDraft('add');
    await until(() => card(name));
    await writeDraft('remove');
    await until(() => card(name), visible => !visible);
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

test('Open panel and widget editors surface background changes and preserve their drafts', {
  skip: !browserUrl && process.env.OSG_COMPANION_LIBRARY_NATIVE !== '1', timeout: 120_000,
}, async () => {
  const page = browserUrl ? await browserPage(browserUrl) : await nativePage();
  const suffix = randomUUID().slice(0,8);
  const names = {panel:'Panel '+suffix, widget:'Widget '+suffix, extra:'Extra '+suffix,
    localPanel:'Local panel '+suffix, localWidget:'Local widget '+suffix,
    remotePanel:'Saved panel '+suffix, remoteWidget:'Saved widget '+suffix};
  let writer = page;
  const write = operation => writer.evaluate(`import('/src/lib/compiled-companion-library.ts').then(async library => {
    const store = new library.CompiledWorkspaceStore();
    const current = await store.load();
    const names = ${JSON.stringify(names)}, operation = ${JSON.stringify(operation)};
    if (operation === 'seed') {
      current.drafts.push(library.newCompiledDraft(names.panel));
      current.sources.push({...library.blankWidgetSource(),title:names.widget});
    } else if (operation === 'panel') {
      current.drafts.find(item => item.title === names.panel).title = names.remotePanel;
      current.sources.push({...library.blankWidgetSource(),title:names.extra});
    } else if (operation === 'widget') {
      current.sources.find(item => item.title === names.widget).title = names.remoteWidget;
    } else {
      current.drafts = current.drafts.filter(item => ![names.panel,names.remotePanel].includes(item.title));
      current.sources = current.sources.filter(item => ![names.widget,names.remoteWidget,names.extra].includes(item.title));
    }
    await store.save(current);
  })`);
  const click = selector => until(() => page.evaluate(`(() => {
    const button = [...document.querySelectorAll(${JSON.stringify(selector)})]
      .find(button => !button.disabled && button.checkVisibility());
    if (!button) return false;
    button.click(); return true;
  })()`));
  const value = name => page.evaluate(`document.querySelector('input[aria-label="${name}"]')?.value`);
  const type = (name, text) => page.evaluate(`(() => {
    const input = document.querySelector('input[aria-label="${name}"]');
    input.value = ${JSON.stringify(text)};
    input.dispatchEvent(new Event('change',{bubbles:true}));
  })()`);
  const back = () => click('.compiled-designer button.header-back');
  const editPanel = title => click(`button[aria-label="Edit ${title}"]`);
  const editWidget = title => click(`button[aria-label="Edit design ${title}"]`);
  const expectConflict = async kind => {
    await until(() => page.evaluate(`[...document.querySelectorAll('.compiled-designer [role="status"]')]
      .some(item => item.textContent.includes(${JSON.stringify('This '+kind+' was changed elsewhere.')}))`),
    Boolean, 4500).catch(error => {throw Error('No external '+kind+' conflict appeared in the open editor',{cause:error});});
    const expected = 'Reopen the ' + (kind === 'widget' ? 'design' : 'panel') + ' before saving.';
    assert.equal(await page.evaluate(`[...document.querySelectorAll('.compiled-designer button')]
      .some(button => button.disabled && button.title.endsWith(${JSON.stringify(expected)}) && button.checkVisibility())`),true);
  };
  try {
    if (browserUrl) writer = await page.openPeer();
    await write('seed');
    await click('nav[aria-label="Main navigation"] button[title="Companions"]');
    await editPanel(names.panel);
    await until(() => value('Panel name'), value => value === names.panel);
    await type('Panel name',names.localPanel);
    await page.evaluate(`import('/src/lib/compiled-companion-library.ts').then(async library => {
      const store = new library.CompiledWorkspaceStore();
      window.__staleDesigner = {store,snapshot:await store.load()};
    })`);
    await write('panel');
    const staleError = await page.evaluate(`(async () => {
      const {store,snapshot} = window.__staleDesigner;
      delete window.__staleDesigner;
      try { await store.save(snapshot); return ''; } catch(error) { return String(error); }
    })()`);
    assert.match(staleError,/changed|revision|stale/i);
    await expectConflict('panel');
    await until(() => page.evaluate(`[...document.querySelectorAll('[aria-label="Panel configuration"] button')]
      .some(button => button.textContent.trim() === ${JSON.stringify(names.extra)})`));
    assert.equal(await value('Panel name'),names.localPanel);
    await page.evaluate('window.__designerConfirm=window.confirm; window.confirm=()=>true');
    await back();
    await editPanel(names.remotePanel);
    assert.equal(await value('Panel name'),names.remotePanel);
    await back();

    await editWidget(names.widget);
    await until(() => value('Widget name'), value => value === names.widget);
    await type('Widget name',names.localWidget);
    await write('widget');
    await expectConflict('widget');
    assert.equal(await value('Widget name'),names.localWidget);
    await back();
    await editWidget(names.remoteWidget);
    assert.equal(await value('Widget name'),names.remoteWidget);
    console.log(JSON.stringify({transport:browserUrl?'browser-dev':'native',
      activePaletteUpdated:true,panelAndWidgetDraftsPreserved:true,conflictsBlockedSave:true,
      staleStorageWriteRejected:true,reopenShowsCurrent:true}));
  } finally {
    await page.evaluate('if (window.__designerConfirm) {window.confirm=window.__designerConfirm;delete window.__designerConfirm}').catch(() => {});
    try {await write('remove');} finally {await page.close();}
  }
});
