import assert from 'node:assert/strict';
import {until} from '../../../scripts/reactive/cdp.mjs';

export async function checkActivitySettings({page, request, clickTab, activityReader, deliveries, waitForReader, waitForRetirement}) {
  const retentionInput = () => page.evaluate(`document.querySelector('[aria-label="Activity retention days"]:not(:disabled)')?.valueAsNumber`);
  const hideActivity = hidden => page.evaluate(`(() => {
    if (${hidden}) Object.defineProperty(document, 'hidden', {configurable:true, value:true});
    else delete document.hidden;
    document.dispatchEvent(new Event('visibilitychange'));
  })()`);
  const {activitySettings: retention} = await request({command:'activity_settings_get'});
  await until(retentionInput, days => days === retention.retentionDays);
  const firstActivityReader = activityReader();
  assert.ok(retention.retentionDays < 3649, 'The dev Cockpit needs two higher retention values for this fixture');
  const changedDays = retention.retentionDays + 1;
  try {
    await request({command:'activity_settings_save', retentionDays:changedDays});
    await until(retentionInput, days => days === changedDays);
    const draftDays = changedDays + 1;
    await page.evaluate(`(() => {
      const input = document.querySelector('[aria-label="Activity retention days"]');
      input.valueAsNumber = ${draftDays};
      input.dispatchEvent(new Event('input', {bubbles:true}));
    })()`);
    const beforeDraftCommit = deliveries.get(firstActivityReader);
    await request({command:'activity_settings_save', retentionDays:retention.retentionDays});
    await until(() => deliveries.get(firstActivityReader) !== beforeDraftCommit);
    assert.equal(await retentionInput(), draftDays, 'An external commit must not overwrite the draft');
    const beforeSave = deliveries.get(firstActivityReader);
    await page.evaluate(`document.querySelector('[aria-label="Activity retention"] button').click()`);
    await until(() => page.evaluate(`document.querySelector('[aria-label="Activity retention"] [role="status"]')
      ?.textContent.includes('Retention saved')`));
    await until(() => deliveries.get(firstActivityReader) !== beforeSave);
    await clickTab('Devices');
    await waitForRetirement();
    await request({command:'activity_settings_save', retentionDays:changedDays});
    assert.equal(activityReader(), undefined);
    await clickTab('General');
    await waitForReader();
    assert.notEqual(activityReader(), firstActivityReader);
    await until(retentionInput, days => days === changedDays);
    const reopenedReader = activityReader();
    await hideActivity(true);
    assert.equal(await retentionInput(), undefined, 'Hidden Activity settings must withdraw the editable snapshot');
    await waitForRetirement();
    await request({command:'activity_settings_save', retentionDays:changedDays + 1});
    assert.equal(activityReader(), undefined);
    await hideActivity(false);
    await waitForReader();
    assert.notEqual(activityReader(), reopenedReader);
    await until(retentionInput, days => days === changedDays + 1);
  } finally {
    await hideActivity(false);
    const {activitySettings: current} = await request({command:'activity_settings_get'});
    if (current.retentionDays === changedDays || current.retentionDays === changedDays + 1) {
      const reader = activityReader();
      const beforeRestore = deliveries.get(reader);
      await request({command:'activity_settings_save', retentionDays:retention.retentionDays});
      if (reader) await until(() => deliveries.get(reader) !== beforeRestore);
    }
  }
}
