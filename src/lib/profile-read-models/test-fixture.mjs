/** Deterministic producer fixture for mounted Svelte handler tests. No polling or native I/O. */
export function readModelFixture() {
  const listeners={library:new Set(),runtime:new Set()};
  const states={library:{status:'ready',value:{readOnly:false}},runtime:{status:'ready',value:{readOnly:false,preferredProfiles:{},profileSession:{state:'stopped',appliedSnapshot:null,owner:null,degradation:[],fault:null},automatic:{schemaVersion:1,mode:'manual',manualProfileId:'00000000-0000-0000-0000-000000000000',rules:[],status:'manual',message:'',conflicts:[],lastAttempt:null,target:null,readOnly:false}}}};
  function observe(key,listener){listeners[key].add(listener);listener(states[key]);return()=>listeners[key].delete(listener);}
  return {observeProfileLibrary:listener=>observe('library',listener),observeProfileRuntime:listener=>observe('runtime',listener),
    emit(key,state=states[key]){states[key]=state;for(const listener of [...listeners[key]])listener(state);},
    count(key){return listeners[key].size;}};
}
