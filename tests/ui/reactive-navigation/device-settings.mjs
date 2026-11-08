import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';
import {until} from '../../../scripts/reactive/cdp.mjs';

export async function checkDeviceSettingsVisibility({page, request, clickTab, deviceTopics, settled, retired, reads}) {
  const setHidden = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  // Windows reports effective HidHide availability rather than the saved
  // preference. The disposable database starts with the default flag off.
  const canSave = process.env.OSG_NAVIGATION_ISOLATED_DATABASE === '1';
  const save = enabled => request({command:'controller_visibility_enable', enabled});
  const prior = reads();
  await clickTab('Devices');
  await settled(['settings.desktop']);
  await until(() => reads().visibility > prior.visibility && reads().inventory > prior.inventory);
  const status = () => page.evaluate(`document.querySelector('[aria-label="Device hiding"] [role="status"]')?.textContent.trim()`);
  const original = await until(status, label => label && label !== 'Not inspected');
  let changed = false;
  try {
    await setHidden(true);
    await until(retired).catch(cause => { throw new Error('Hidden Devices settings retained observation readers', {cause}); });
    const hidden = reads();
    if (canSave) {
      await save(true);
      changed = true;
    }
    await delay(300);
    assert.deepEqual(reads(), hidden, 'A hidden view must not request inventory or visibility on commits');
    assert.equal(await status(), 'Not inspected');
    await setHidden(false);
    await settled(deviceTopics);
    await until(() => reads().inventory > hidden.inventory && reads().visibility > hidden.visibility);
    await delay(300);
    assert.deepEqual(reads(), {inventory:hidden.inventory + 1, visibility:hidden.visibility + 1});
    if (canSave) {
      const visible = reads();
      await save(false);
      changed = false;
      await until(() => reads().visibility > visible.visibility);
      await until(status, label => label === original);
      await delay(300);
      assert.deepEqual(reads(), {inventory:visible.inventory, visibility:visible.visibility + 1},
        'A visibility commit must cause one scoped read and leave inventory quiet');
    }
    console.log(JSON.stringify({deviceSettingsDocumentHideAndReopen:'passed', hiddenReads:0,
      reopenReads:{inventory:1, visibility:1}, visibilityCommitReads:canSave ? 1 : null}));
  } finally {
    await setHidden(false);
    try {
      if (changed) await save(false);
    } finally {
      await clickTab('General');
      await until(retired);
    }
  }
}
