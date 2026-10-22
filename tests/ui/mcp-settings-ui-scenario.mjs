import assert from 'node:assert/strict';

export async function desktopUiConflict({page,writer,ok,mutation}) {
  const until=async(operation,message)=>{
    for(let attempt=0;attempt<150;attempt++){
      const result=await operation();if(result)return result;
      await new Promise(resolve=>setTimeout(resolve,100));
    }
    throw Error(message);
  };
  const selector={scope:'desktop'},initial=await ok(writer,'local_settings_get',selector);
  assert.equal(initial.profileSession.owner,null,'The UI settings race must not interrupt an applied owner');
  const original={enabled:initial.settings.enabled,scope:initial.settings.scope};
  const candidate={enabled:!original.enabled,scope:{kind:original.scope.kind==='application'?'desktop-wide':'application'}};
  let changed=false;
  try {
    await page.evaluate(()=>[...document.querySelectorAll('button')].find(button=>button.textContent.trim()==='Settings').click());
    await until(()=>page.evaluate(()=>!!document.querySelector('#settings-tab-outputs')),'Outputs settings tab is missing');
    await page.evaluate(()=>document.querySelector('#settings-tab-outputs').click());
    await until(()=>page.evaluate(()=>[...document.querySelectorAll('.output-settings button')].some(button=>button.textContent.trim()==='Refresh output setup'&&!button.disabled)),'Output setup did not finish reading');
    await page.evaluate(()=>[...document.querySelectorAll('.output-settings button')].find(button=>button.textContent.trim()==='Refresh output setup').click());
    await until(()=>page.evaluate(enabled=>{
      const label=[...document.querySelectorAll('.output-settings label')].find(label=>label.textContent.includes('Enable desktop output on this computer'));
      const input=label?.querySelector('input');return input&&!input.disabled&&input.checked===enabled;
    },original.enabled),'The real desktop output control did not capture its current settings');
    const committed=await ok(writer,'local_settings_save',mutation({...selector,expectedSettingsRevision:initial.revision,expectedActiveOwner:null,settings:candidate}));
    changed=true;assert.deepEqual(committed.settings.scope,candidate.scope);
    await page.evaluate(()=>[...document.querySelectorAll('.output-settings label')].find(label=>label.textContent.includes('Enable desktop output on this computer')).querySelector('input').click());
    const error=await until(()=>page.evaluate(()=>[...document.querySelectorAll('.output-settings [role="alert"]')].map(item=>item.textContent).find(text=>/revision conflict/i.test(text))),'A stale real UI settings edit did not report a revision conflict');
    const after=await ok(writer,'local_settings_get',selector);
    assert.equal(after.revision,committed.revision);assert.equal(after.settings.enabled,candidate.enabled);assert.deepEqual(after.settings.scope,candidate.scope);assert.equal(after.profileSession.owner,null);
    return{staleUiRejected:true,mcpConfigurationPreserved:true,ownerUnchanged:true,error};
  } finally {
    if(changed){
      const current=await ok(writer,'local_settings_get',selector);
      assert.equal(current.settings.enabled,candidate.enabled);assert.deepEqual(current.settings.scope,candidate.scope);
      await ok(writer,'local_settings_save',mutation({...selector,expectedSettingsRevision:current.revision,expectedActiveOwner:null,settings:original}));
      const restored=await ok(writer,'local_settings_get',selector);assert.equal(restored.settings.enabled,original.enabled);assert.deepEqual(restored.settings.scope,original.scope);
    }
  }
}
