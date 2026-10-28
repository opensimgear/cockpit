import type {CanvasNode} from './profile-canvas';
import {gameIntegrationSourceStatus} from './profile-game-source';

type StatusContext={vehicleId?:string;activeVehicleId?:string;inventoryKnown?:boolean;discovering?:boolean;error?:string;issue?:string;needsPanel?:boolean};
export type ProfileBlockStatus={tone:'accent'|'muted'|'warning';label:string};

/** Keep the compact block indicator consistent with availability and validation. */
export function canvasBlockStatus(node:CanvasNode,context:StatusContext={}):ProfileBlockStatus {
  const warning=context.error||context.issue;
  if(warning)return {tone:'warning',label:warning};
  if(node.processing?.draft)return {tone:'warning',label:'Needs input'};
  if(context.discovering)return {tone:'muted',label:'Discovering inputs and outputs…'};
  if(context.needsPanel)return {tone:'warning',label:'Choose a panel'};
  if(!node.resource)return {tone:'accent',label:'Ready'};
  if(node.resource.kind==='debug-controller')return {tone:'accent',label:'Available'};
  if(!node.assignment)return {tone:'warning',label:'Unassigned'};
  if(!node.observation)return context.inventoryKnown===false?{tone:'muted',label:'Checking availability…'}:{tone:'warning',label:'Source removed from inventory'};
  const expected=node.resource.kind==='device'?['host-controller','osg-module']:node.resource.kind==='game'?['game-integration']:['companion'];
  if(!expected.includes(node.observation.resourceType))return {tone:'warning',label:'Source removed from inventory'};
  if(node.resource.kind==='game'&&node.observation){
    const status=gameIntegrationSourceStatus(undefined,[node.observation]);
    if(status==='Running'&&context.vehicleId){
      const activeVehicle=context.activeVehicleId??node.observation.attributes.activeVehicleId??node.observation.attributes.currentVehicleId;
      if(activeVehicle!==context.vehicleId)return {tone:'muted',label:'Vehicle inactive'};
      if(node.observation.availability!=='online')return {tone:'muted',label:'Waiting for vehicle integration'};
    }
    return {tone:status==='Running'?'accent':status==='Configuration needed'?'warning':'muted',label:status};
  }
  if(node.observation.availability==='offline')return {tone:'muted',label:'Offline'};
  if(node.observation.availability==='unavailable')return {tone:'warning',label:'Unavailable'};
  return {tone:'accent',label:'Online'};
}
