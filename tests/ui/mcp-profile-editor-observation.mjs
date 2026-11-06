import assert from 'node:assert/strict';

export async function profileEditorObservation({page, writer, ok, mutation, until, library, saved, mcpName, nextObservation, observationUri}) {
  const clickProfileButton = label => page.evaluate(label => {
    const button = [...document.querySelectorAll('main button')].find(button =>
      button.getAttribute('aria-label') === label || button.textContent.trim() === label);
    if (!button || button.disabled) return false;
    button.click();
    return true;
  }, label);
  const editorName = () => page.evaluate(() => document.querySelector('button[aria-label="Edit profile name"] span')?.textContent);
  await until(() => page.evaluate(() => {
    const button = document.querySelector('nav[aria-label="Main navigation"] button[title="Profiles"]');
    button?.click();
    return !!button;
  }), 'Profiles navigation did not open');
  await until(() => clickProfileButton(`Edit ${mcpName}`), 'Temporary profile did not appear in the list');
  await until(async () => await editorName() === mcpName, 'Clean profile editor did not open');
  let revision = saved.document.revision;
  let latestName;
  for (const suffix of ['first edit', 'latest edit']) {
    latestName = `${mcpName} ${suffix}`;
    const draft = await ok(writer, 'profile_prepare_changes', {
      profileId: saved.document.id, expectedRevision: revision, edits: [{kind: 'set_name', name: latestName}],
    });
    await ok(writer, 'profile_save_prepared', mutation({draftId: draft.draftId}));
    revision = (await ok(writer, 'profile_get', {id: saved.document.id})).document.revision;
    await until(async () => await editorName() === latestName, 'The open editor missed a committed MCP rename');
  }
  await until(() => clickProfileButton('Back to Profiles'), 'Clean editor did not close');
  await until(() => clickProfileButton(`Edit ${latestName}`), 'Latest profile was absent from the list');
  await until(async () => await editorName() === latestName, 'Reopened editor restored a stale name');
  const editName = async name => {
    await until(() => clickProfileButton('Edit profile name'), 'Profile rename control was unavailable');
    await page.evaluate(name => {
      const input = document.querySelector('input[aria-label="Profile name"]');
      input.value = name;
      input.dispatchEvent(new Event('input', {bubbles: true}));
      input.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', bubbles: true}));
    }, name);
    await until(async () => await editorName() === name, 'The local draft rename did not apply');
  };
  const localDraft = `${mcpName} local draft`;
  const remoteName = `${mcpName} remote edit`;
  await editName(localDraft);
  const remoteDraft = await ok(writer, 'profile_prepare_changes', {
    profileId: saved.document.id, expectedRevision: revision, edits: [{kind: 'set_name', name: remoteName}],
  });
  await ok(writer, 'profile_save_prepared', mutation({draftId: remoteDraft.draftId}));
  await until(async () => (await library())?.value?.profiles.some(profile => profile.name === remoteName), 'Library missed the remote rename');
  assert.equal(await editorName(), localDraft, 'A remote commit overwrote the dirty editor');
  await until(() => clickProfileButton('Save'), 'Dirty editor save was unavailable');
  await until(() => page.evaluate(() => /revision|changed/i.test(document.querySelector('.workspace-error[role="alert"]')?.textContent ?? '')), 'Stale save did not show a revision conflict');
  assert.equal((await ok(writer, 'profile_get', {id: saved.document.id})).document.name, remoteName);
  await until(() => clickProfileButton('Back to Profiles'), 'Reopened editor did not close');
  await until(() => clickProfileButton('Discard and continue'), 'Dirty draft discard was unavailable');
  await until(() => clickProfileButton(`Edit ${remoteName}`), 'Committed MCP profile was absent after discarding');
  await until(async () => await editorName() === remoteName, 'Discard did not restore the committed name');
  const previous = (await ok(writer, 'profile_get', {id: saved.document.id})).document;
  const uiName = `${mcpName} UI save`;
  await editName(uiName);
  await until(() => clickProfileButton('Save'), 'The recovered editor could not save');
  for (;;) {
    const notification = await nextObservation();
    if (notification.params?.uri !== observationUri) continue;
    const frame = notification.params._meta['opensimgear.com/observation'];
    const row = frame.payload?.profiles?.find(row => row.id === previous.id && row.name === uiName);
    if (row) {
      assert.ok(row.revision > previous.revision);
      assert.equal((await ok(writer, 'profile_get', {id: previous.id})).document.revision, row.revision);
      break;
    }
  }
  await until(() => clickProfileButton('Back to Profiles'), 'Recovered editor did not close');
}
