import assert from 'node:assert/strict';
/** Presentation tests still carry the exact native settings revision on saves. */
export function progressSettingsInvoke(invoke){
 const value=row=>({delivery:row.delivery,position:row.position});
 return async(command,args={})=>{
  if(command==='profile_progress_settings_get')return value(await invoke(command,args));
  if(command!=='profile_progress_settings_save')return invoke(command,args);
  const current=await invoke('profile_progress_settings_get');
  const expectedRevision=args.expectedRevision??current.settingsRevision;
  const saved=await invoke(command,{settings:value(args.settings),expectedRevision});
  assert.equal(saved.settingsRevision,expectedRevision+1);assert.deepEqual(value(saved),value(args.settings));return value(saved);
 };
}
