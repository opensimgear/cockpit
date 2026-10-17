import { validateFeedbackUpdate, type PortableUi } from '@opensimgear/companion-contracts';
import type { FeedbackSamples } from '@opensimgear/companion-ui';

export type FeedbackIdentity = {clientId:string;sessionId:string;generation:string;controllerInstanceId:string;publicationHash:string};
const fields = ['clientId','sessionId','generation','controllerInstanceId','publicationHash'] as const;
/** Visual-only WSS receiver. Neither feedback nor resync can enqueue controls. */
export function createBrowserFeedback() {
  let identity = '', generation = -1n, sequence = -1n, bindings = new Map<string,string>(), samples:FeedbackSamples = {}, clock = -1;
  return {
    snapshot:():FeedbackSamples=>structuredClone(samples),
    accept(expected:FeedbackIdentity, input:unknown, ui:PortableUi, now:number, resync=false):boolean {
      const checked=validateFeedbackUpdate(input,ui);
      if(!checked.ok)throw new Error(checked.error);
      const frame=checked.value;
      if(fields.some(key=>!expected[key]||frame[key]!==expected[key]))throw new Error('Feedback belongs to another panel session.');
      if(!Number.isFinite(now)||now<0||now<clock)throw new Error('Invalid feedback clock.');
      const nextIdentity=JSON.stringify(fields.map(key=>expected[key])), same=identity===nextIdentity;
      const nextGeneration=BigInt(frame.bindingGeneration), nextSequence=BigInt(frame.sequence);
      if(same&&(nextGeneration<generation||(nextGeneration===generation&&nextSequence<sequence)))throw new Error('Stale feedback generation or sequence.');
      if(frame.mode==='delta'&&(!same||nextGeneration!==generation))throw new Error('Feedback delta requires its current snapshot.');
      const nextBindings=frame.mode==='snapshot'?new Map(frame.bindings!.map(row=>[row.channelId,row.outputId])):bindings;
      if(frame.values.some(value=>nextBindings.get(value.channelId)!==value.outputId))throw new Error('Feedback output differs from the current binding.');
      if(frame.mode==='snapshot'&&frame.values.length!==nextBindings.size)throw new Error('Feedback snapshot is incomplete.');
      if(same&&nextGeneration===generation&&nextSequence===sequence&&!(resync&&frame.mode==='snapshot'))return false;
      const next:FeedbackSamples=frame.mode==='snapshot'?{}:structuredClone(samples);
      for(const value of frame.values)next[value.channelId]={...structuredClone(value),receivedAtMs:now};
      identity=nextIdentity;generation=nextGeneration;sequence=nextSequence;bindings=nextBindings;samples=next;clock=now;
      return true;
    },
    reset(){samples={};},
    disconnect(){samples=Object.fromEntries(Object.entries(samples).map(([channel,sample])=>{const{value:_value,...metadata}=sample;return[channel,{...metadata,status:'disconnected' as const}];}));},
    clear(){identity='';generation=-1n;sequence=-1n;bindings=new Map();samples={};clock=-1;},
  };
}
