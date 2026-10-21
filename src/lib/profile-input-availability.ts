import type {DeepReadonly, ProfileDocument} from './profiles';

export type ProfileInputAvailability = {full:boolean;total:number;available:number;reason:string};
type InputChoice = {readonly id:string;readonly direction:'input'|'output'};

/** Count exact input identities from the active canvas catalogue. A discovered
 * @observed choice has no writer until it is saved as a logical profile port. */
export function inputAvailability(
  document:Pick<DeepReadonly<ProfileDocument>,'bindings'>,
  nodeId:string,
  ports:readonly InputChoice[],
  portId?:string,
  replacingId?:string,
):ProfileInputAvailability {
  const inputs=new Set(ports.filter(port=>port.direction==='input').map(port=>port.id));
  const occupied=new Set(document.bindings
    .filter(binding=>binding.id!==replacingId&&binding.to.nodeId===nodeId&&inputs.has(binding.to.portId))
    .map(binding=>binding.to.portId));
  const total=inputs.size,available=total-occupied.size;
  if(portId!==undefined){
    if(!inputs.has(portId))return {full:true,total,available,reason:'This input is no longer available.'};
    const full=occupied.has(portId);
    return {full,total,available,reason:full?'This input is already connected.':''};
  }
  const full=total>0&&available===0;
  return {full,total,available,reason:full?'All inputs are connected.':''};
}
