import {nativeProfileActivation,type NativeProfileActivation} from './profile-runtime';
import type {DeepReadonly} from './profiles';
import {SharedObservations} from './streams/shared';

const observations=new SharedObservations();

export function observeProfileActivation(
  listener:(progress:DeepReadonly<NativeProfileActivation>|null,error?:string)=>void,
):()=>void {
  return observations.subscribe('profiles.activation',state=>{
    if(state.status==='connecting')return;
    if(state.status!=='ready'||state.frame?.source!=='profile-activation'){
      listener(null,state.error??'Waiting for fresh activation progress.');
      return;
    }
    const value=state.frame.payload;
    if(value===null){listener(null);return;}
    if(!value||typeof value!=='object'||!('activationId' in value)||typeof value.activationId!=='string')return;
    let progress:DeepReadonly<NativeProfileActivation>;
    try{progress=nativeProfileActivation(value,value.activationId);}catch{return;}
    listener(progress);
  });
}
