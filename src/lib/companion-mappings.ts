import type { CompanionMappingSource, CompanionRoute, MappingAssignment, MappingProfile, MappingResult, VirtualBinding, VirtualSlot } from './mappings.ts';
export type TargetLayout = MappingProfile['targets'][number]['layout'];
export type CompanionRoleState = { role: string; sourceId: string; label: string; state: 'unassigned' | 'offline' | 'waiting-baseline' | 'descriptor-changed' | 'fault' | 'ready'; reason: string };
const integer = (value: number, low: number, high: number) => Number.isSafeInteger(value) && value >= low && value <= high;
export function companionInstanceLabel(source: Pick<CompanionMappingSource, 'name' | 'id'>): string { return `${source.name} · ${source.id}`; }
export function targetLayoutLabel(layout: TargetLayout): string {
  return `${layout.buttons} buttons · ${layout.axes.length} axes · ${layout.hats} hats${layout.axes.length ? ' · ' + layout.axes.map((axis,index) => `Axis ${index+1}: ${axis.range}, fallback ${axis.fallback}`).join('; ') : ''}`;
}
function validLayout(layout: TargetLayout): boolean {
  return integer(layout.buttons,0,128) && integer(layout.hats,0,4) && Array.isArray(layout.axes) && layout.axes.length <= 16 && layout.buttons + layout.hats + layout.axes.length > 0 && layout.axes.every(axis => ['bipolar','unipolar'].includes(axis.range) && integer(axis.fallback,axis.range==='bipolar'?-32768:0,32767));
}
export function equalTargetLayout(first: TargetLayout, second: TargetLayout): boolean {
  return validLayout(first) && validLayout(second) && first.buttons === second.buttons && first.hats === second.hats && first.axes.length === second.axes.length && first.axes.every((axis,index) => axis.range === second.axes[index].range && axis.fallback === second.axes[index].fallback);
}
/** Capability checks are advisory; the native start transaction rechecks ownership and readiness. */
export function virtualBindingIssues(target: MappingProfile['targets'][number], binding: VirtualBinding, slot?: VirtualSlot): string[] {
  const issues: string[] = [];
  if (target.id !== binding.target) issues.push('Binding belongs to another logical target.');
  if (!equalTargetLayout(target.layout,binding.layout)) issues.push('The saved target shape or axis fallback differs from the profile.');
  if (!integer(binding.slot,1,16)) issues.push('Choose an output identity from 1 to 16.');
  if (!Array.isArray(binding.axes) || binding.axes.length !== target.layout.axes.length || new Set(binding.axes).size !== binding.axes.length || binding.axes.some(usage=>!integer(usage,1,65535))) issues.push('Each logical axis needs one distinct, explicit native axis usage.');
  if (!slot) { issues.push('Inspect the selected native output before saving.'); return issues; }
  if (slot.id !== binding.slot) issues.push('The inspected output is another identity.');
  if (slot.buttons !== target.layout.buttons || slot.axes.length !== target.layout.axes.length || slot.continuousHats !== target.layout.hats || slot.discreteHats !== 0) issues.push('Native output counts must match exactly; controls are never truncated.');
  if (slot.forceFeedback) issues.push('Force-feedback layouts are unsupported for this output.');
  if (slot.axes.some(axis=>!Number.isSafeInteger(axis.min)||!Number.isSafeInteger(axis.max)||axis.min>=axis.max) || new Set(slot.axes.map(axis=>axis.usage)).size !== slot.axes.length || binding.axes.some(usage=>!slot.axes.some(axis=>axis.usage===usage))) issues.push('Native axis usages or ranges do not match the explicit binding.');
  return issues;
}
export function exactTargetBindings(profile: MappingProfile, choices: Record<string,number>, slots: VirtualSlot[], axisChoices: Record<string,number[]> = {}): VirtualBinding[] {
  const bindings=profile.targets.map(target=>{
    const slot=slots.find(slot=>slot.id===choices[target.id]);
    const binding:VirtualBinding={target:target.id,slot:choices[target.id],axes:axisChoices[target.id]??slot?.axes.map(axis=>axis.usage)??[],layout:structuredClone(target.layout)};
    const issues=virtualBindingIssues(target,binding,slot);
    if(issues.length)throw new Error(`${target.name}: ${issues.join(' ')}`);
    return binding;
  });
  if(new Set(bindings.map(binding=>binding.slot)).size!==bindings.length)throw new Error('Each logical target requires a distinct native output identity.');
  return bindings;
}
export function companionRoleReadiness(profile: MappingProfile, assignments: MappingAssignment[], sources: CompanionMappingSource[], pinnedHashes: Record<string,string> = {}): CompanionRoleState[] {
  return profile.roles.filter(role=>role.origin==='companion').map(role=>{
    const assignment=assignments.find(assignment=>assignment.role===role.id), sourceId=assignment?.source.origin==='companion'?assignment.source.id:'';
    const source=sources.find(source=>source.id===sourceId), base={role:role.id,sourceId,label:source?companionInstanceLabel(source):sourceId||role.name};
    if(!sourceId)return{...base,state:'unassigned',reason:'Assign this role to a retained companion instance.'};
    if(!source)return{...base,state:'offline',reason:'The assigned instance is unavailable; saved assignments do not activate it.'};
    if(pinnedHashes[sourceId]&&pinnedHashes[sourceId]!==source.publicationHash)return{...base,state:'descriptor-changed',reason:'The immutable publication changed. Inspect and explicitly reconcile before Start.'};
    if(source.fault)return{...base,state:'fault',reason:source.fault};
    if(!source.online)return{...base,state:'offline',reason:'Reconnect this instance; other companions keep their own sessions.'};
    if(!source.baselineReady)return{...base,state:'waiting-baseline',reason:'Waiting for a fresh complete source baseline.'};
    return{...base,state:'ready',reason:'Source baseline is available. Target/backend preflight is still required.'};
  });
}

export function companionTargetBindings(profile: MappingProfile, choices: Record<string,number>, output: MappingResult | null, axisChoices: Record<string,number[]> = {}): { bindings: VirtualBinding[]; issues: string[] } {
  try {
    if (!output) throw new Error('Inspect native target capabilities before saving output assignments.');
    if (!['uinput','macos-hid'].includes(output.backend??'')) return {bindings:exactTargetBindings(profile,choices,output.slots??[],axisChoices),issues:[]};
    const limits=output.limits??{axes:8,buttons:32,hats:4,targets:16};
    if(profile.targets.length>limits.targets)throw new Error('This backend cannot represent every authored logical target.');
    const bindings=profile.targets.map(target=>{
      if(!validLayout(target.layout)||target.layout.axes.length>limits.axes||target.layout.buttons>limits.buttons||target.layout.hats>limits.hats)throw new Error(`${target.name}: the complete authored shape exceeds this backend; no controls will be truncated.`);
      const slot=choices[target.id],axes=axisChoices[target.id]??target.layout.axes.map((_,index)=>0x30+index);
      if(!integer(slot,1,16)||axes.length!==target.layout.axes.length||new Set(axes).size!==axes.length||axes.some(usage=>!integer(usage,0x30,0x30+limits.axes-1)))throw new Error(`${target.name}: choose an explicit identity and unique supported axis usages.`);
      return{target:target.id,slot,axes:[...axes],layout:structuredClone(target.layout)};
    });
    if(new Set(bindings.map(binding=>binding.slot)).size!==bindings.length)throw new Error('Each logical target requires its own output identity.');
    return{bindings,issues:[]};
  }catch(error){return{bindings:[],issues:[error instanceof Error?error.message:String(error)]};}
}

export function createCompanionOperationGuard() {
  const operations=new Map<string,number>(),polling=new Set<string>();let selected='',view=0;
  const capture=(instanceId=selected)=>({instanceId,operation:operations.get(instanceId)??0,view});
  return {
    capture,
    beginPoll(instanceId:string){if(polling.has(instanceId))return null;polling.add(instanceId);return capture(instanceId);},
    endPoll(token:ReturnType<typeof capture>){polling.delete(token.instanceId);},
    select(instanceId:string){selected=instanceId;view++;return capture();},
    begin(instanceId:string){operations.set(instanceId,(operations.get(instanceId)??0)+1);return capture(instanceId);},
    current(token:ReturnType<typeof capture>){return token.instanceId===selected&&token.view===view&&token.operation===(operations.get(token.instanceId)??0);},
    dispose(){selected='';view++;},
  };
}

export function companionStartAllowed(route: CompanionRoute, mode: 'recording' | 'live', dirty: boolean, source?: CompanionMappingSource): boolean {
  return !dirty && !route.session.running && !route.session.busy && !route.session.starting && route.session.cleanupComplete && !route.session.recoveryRequired && source?.id === route.instanceId && source.online && source.baselineReady && !source.fault && route.expectedPublicationHash === route.publicationHash && (mode === 'live' ? route.preflight.liveReady : route.preflight.recordingReady);
}
export function companionSessionLabel(session: CompanionRoute['session']): string {
  if(session.recoveryRequired)return 'Recovery required; output state needs native inspection';
  if(session.stopRequested&&!session.cleanupComplete)return 'Stop requested; waiting for native cleanup';
  if(session.starting)return 'Starting selected native session';
  if(session.busy)return 'Native operation pending';
  if(session.running)return `${session.mode === 'live' ? 'Native live' : 'Recording'} session · ${session.ready ? 'Ready' : 'Waiting for baseline'}`;
  if(!session.cleanupComplete)return 'Native cleanup pending';
  return 'Mapping stopped · native cleanup complete';
}
