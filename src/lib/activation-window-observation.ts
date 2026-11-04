import type {ActivationWindowSnapshot} from './profile-activation-window';
import {SharedObservations} from './streams/shared';

const observations=new SharedObservations();

export function observeActivationWindow(listener:(snapshot:ActivationWindowSnapshot|null,error?:string)=>void):()=>void {
  return observations.subscribe('profiles.activation-window',state=>{
    if(state.status==='connecting')return;
    if(state.status!=='ready'||!state.frame||state.frame.source!=='activation-window'){
      listener(null,state.error??'Waiting for Cockpit to reconnect…');
      return;
    }
    listener(state.frame.payload as ActivationWindowSnapshot|null);
  });
}
