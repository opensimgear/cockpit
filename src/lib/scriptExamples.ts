import shift from './script-examples/shift-layer.rhai?raw';
import two from './script-examples/two-controls.rhai?raw';
import threshold from './script-examples/threshold.rhai?raw';
import hold from './script-examples/short-long.rhai?raw';
import pulse from './script-examples/pulse.rhai?raw';
import type {MappingProfile,MappingScript,RuleInput} from './mappings';
export const scriptExamples={'Shift layer':shift,'Two controls':two,'Axis hysteresis':threshold,'Short / long press':hold,'Timed pulse':pulse};
export function scriptExample(profile:MappingProfile,name:keyof typeof scriptExamples):MappingScript {
  const kind=name==='Axis hysteresis'?'axis':'button';
  const available=profile.roles.flatMap(role=>role.controls.filter(c=>c.kind.kind===kind).map(c=>({role:role.id,control:c.id})));
  const count=['Shift layer','Two controls'].includes(name)?2:1;
  if(available.length<count)throw new Error(`This example needs ${count} declared ${kind} input${count===1?'':'s'}. Add the roles and controls to the profile first.`);
  const output=profile.targets.find(t=>t.layout.buttons>0);
  if(!output)throw new Error('Add a target button for this example.');
  const read=(i:number)=>`ctx.inputs[${JSON.stringify(available[i].role)}][${JSON.stringify(available[i].control)}].value`;
  const source=scriptExamples[name].replaceAll('ctx.inputs.left.trigger.value',read(0)).replaceAll('ctx.inputs.left.x.value',read(0)).replaceAll('ctx.inputs.right.trigger.value',read(count-1));
  return {id:'script-'+crypto.randomUUID().slice(0,8),source,inputs:available.slice(0,count).map(i=>({kind:'source',...i}) as RuleInput),outputs:[{id:'result',output:{target:output.id,control:{kind:'button',index:0}}}],parameters:{}};
}
