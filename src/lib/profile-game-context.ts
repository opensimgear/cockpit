import {isGameId,isRegisteredGameId,getGameCatalogue,gameHasIntegration,gameName,type GameId} from './games';
import {canvasNodes} from './profile-canvas';
import {graphAssignResource,graphRemoveNode,type ProfileGraphDraft} from './profile-graph';
import {profileDocumentError,profileDocumentAssignmentsError,type DeepReadonly,type ProfileDocument} from './profiles';
import type {ObservedResourcePort,ResourceSummary} from './profile-inventory';
import {profileChangeVehicleTab,profileRemoveVehicleTab,profileTabNodeIds} from './profile-vehicle-tabs';

export function profileGameContext(document:Pick<DeepReadonly<ProfileDocument>,'eligibility'>):GameId|'' {
  const id=document.eligibility.gameIds[0];return isGameId(id)?id:'';
}
export function profileGameName(id:string):string{return gameName(id);}

/** Aircraft aliases identify one installed DCS-BIOS module; Base always uses CommonData. */
export function profileDcsResourceMatchesVehicle(source:DeepReadonly<ResourceSummary>,vehicleId:string):boolean {
  if(source.resourceType!=='game-integration'||source.attributes.gameId!=='dcs')return false;
  if(!vehicleId)return source.attributes.aircraftModule==='CommonData';
  if(source.attributes.aircraftModule==='CommonData')return false;
  if(source.attributes.aircraftModule===vehicleId||source.attributes.vehicleId===vehicleId)return true;
  try{const aliases=JSON.parse(source.attributes.vehicleAliases??'[]');return Array.isArray(aliases)&&aliases.includes(vehicleId);}catch{return false;}
}
export function profileGameIntegrationSource(document:DeepReadonly<ProfileDocument>,resourceId:string,inventory:readonly DeepReadonly<ResourceSummary>[]):DeepReadonly<ResourceSummary>|undefined {
  const gameId=profileGameContext(document),tab=document.vehicleTabs?.find(tab=>tab.nodeIds.includes(resourceId));
  return inventory.find(source=>source.resourceType==='game-integration'&&source.attributes.gameId===gameId&&(gameId!=='dcs'||profileDcsResourceMatchesVehicle(source,tab?.vehicleId??'')));
}
/** Browsing another aircraft never exposes the loaded aircraft's controls in its canvas. */
export function profileGameTabInventory(document:DeepReadonly<ProfileDocument>,inventory:readonly DeepReadonly<ResourceSummary>[],tabId=''):readonly DeepReadonly<ResourceSummary>[] {
  if(profileGameContext(document)!=='dcs')return inventory;
  const vehicleId=document.vehicleTabs?.find(tab=>tab.id===tabId)?.vehicleId??'';
  return inventory.filter(source=>source.attributes.gameId!=='dcs'||profileDcsResourceMatchesVehicle(source,vehicleId));
}

/** The saved context and its integration block change together in one Undo edit. */
export function profileSetGameContext(value:DeepReadonly<ProfileGraphDraft>,gameId:GameId|'',inventory:readonly DeepReadonly<ResourceSummary>[]):ProfileGraphDraft {
  if(gameId&&!isRegisteredGameId(gameId))throw Error('Choose a discovered game context or None.');
  let next=JSON.parse(JSON.stringify(value)) as ProfileGraphDraft;
  if(profileGameContext(value.document)!==gameId)for(const tab of [...(next.document.vehicleTabs??[])])next=profileRemoveVehicleTab(next.document,next.assignments,tab.id);
  const before=canvasNodes(next.document,next.assignments,inventory),baseIds=profileTabNodeIds(next.document),games=next.document.resources.filter(resource=>resource.kind==='game'&&baseIds.has(resource.id));
  const integrated=gameId&&gameHasIntegration(getGameCatalogue().find(game=>game.id===gameId));
  const keep=integrated?games.find(resource=>{
    const assignment=value.assignments.find(assignment=>assignment.resourceId===resource.id),observed=inventory.find(resource=>resource.id===assignment?.sourceId);
    return observed?.attributes.gameId===gameId||profileGameContext(value.document)===gameId&&games.length===1&&!observed;
  }):undefined;
  const oldPosition=before.find(node=>node.resource?.kind==='game')?.position;
  const oldGroup=value.document.editor.groups?.find(group=>games[0]&&group.members.includes(games[0].id))?.id;
  for(const resource of games)if(resource.id!==keep?.id)next=graphRemoveNode(next,resource.id);
  next.document.eligibility.gameIds=gameId?[gameId]:[];
  if(integrated){
    let resource=next.document.resources.find(resource=>resource.id===keep?.id);
    if(!resource){
      if(next.document.resources.length>=64)throw Error('Remove a component before selecting a game context; a profile supports 64 resources.');
      const used=new Set([...next.document.resources,...next.document.processingNodes,...next.document.virtualOutputs,...(next.document.desktopOutputs??[])].map(node=>node.id));
      let number=1;while(used.has(`game-context-${gameId}-${number}`))number++;
      resource={id:`game-context-${gameId}-${number}`,name:profileGameName(gameId),kind:'game',required:false,ports:[]};next.document.resources.push(resource);
      // Freeze existing implicit positions before placing the context integration.
      next.document.editor.positions=before.filter(node=>node.id!==resource!.id&&(next.document.resources.some(resource=>resource.id===node.id)||next.document.processingNodes.some(resource=>resource.id===node.id)||next.document.virtualOutputs.some(resource=>resource.id===node.id)||next.document.desktopOutputs?.some(resource=>resource.id===node.id))).map(node=>({nodeId:node.id,x:node.position.x,y:node.position.y}));
      const bottom=Math.max(0,...before.filter(node=>node.resource?.kind!=='game').map(node=>node.position.y+280));
      next.document.editor.positions.push({nodeId:resource.id,x:oldPosition?.x??48,y:oldPosition?.y??bottom+48});
      if(oldGroup)next.document.editor.groups?.find(group=>group.id===oldGroup)?.members.push(resource.id);
    }
    for(const block of next.document.resources.filter(resource=>resource.kind==='game')){
      const source=profileGameIntegrationSource(next.document,block.id,inventory),assignment=next.assignments.find(assignment=>assignment.resourceId===block.id);
      if(source&&!block.ports.length&&!assignment?.ports.length){if(assignment)assignment.sourceId=source.id;else next.assignments.push({resourceId:block.id,sourceId:source.id,ports:[]});}
    }
  }
  const error=profileDocumentError(next.document)||profileDocumentAssignmentsError(next.document,next.assignments);if(error)throw Error(error);
  return next;
}

/** Each vehicle canvas can author simulator telemetry and commands independently. */
export function profileAddVehicleIntegration(value:DeepReadonly<ProfileGraphDraft>,tabId:string,inventory:readonly DeepReadonly<ResourceSummary>[]):ProfileGraphDraft {
  const next=JSON.parse(JSON.stringify(value)) as ProfileGraphDraft,tab=next.document.vehicleTabs?.find(tab=>tab.id===tabId);
  if(!tab)throw Error('Choose an existing vehicle tab.');
  if(next.document.resources.some(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id)))return next;
  if(next.document.resources.length>=64)throw Error('Remove a component before adding a vehicle tab; a profile supports 64 resources.');
  const used=new Set([...next.document.resources,...next.document.processingNodes,...next.document.virtualOutputs,...(next.document.desktopOutputs??[])].map(node=>node.id));
  let number=1;while(used.has(`game-context-${tab.gameId}-${number}`))number++;
  const id=`game-context-${tab.gameId}-${number}`;
  next.document.resources.push({id,name:profileGameName(tab.gameId),kind:'game',required:false,ports:[]});
  tab.nodeIds.push(id);next.document.editor.positions.push({nodeId:id,x:48,y:48});
  const source=profileGameIntegrationSource(next.document,id,inventory);
  if(source)next.assignments.push({resourceId:id,sourceId:source.id,ports:[]});
  const error=profileDocumentError(next.document)||profileDocumentAssignmentsError(next.document,next.assignments);if(error)throw Error(error);
  return next;
}

/** Empty context blocks can acquire their exact native integration when inventory arrives. */
export function profileGameContextNeedsSync(value:DeepReadonly<ProfileGraphDraft>,inventory:readonly DeepReadonly<ResourceSummary>[]):boolean {
  const id=profileGameContext(value.document);if(!id||!gameHasIntegration(getGameCatalogue().find(game=>game.id===id)))return false;
  const base=profileTabNodeIds(value.document);
  if(!value.document.resources.some(resource=>resource.kind==='game'&&base.has(resource.id)))return true;
  return value.document.resources.some(resource=>{if(resource.kind!=='game'||resource.ports.length)return false;const source=profileGameIntegrationSource(value.document,resource.id,inventory),assignment=value.assignments.find(assignment=>assignment.resourceId===resource.id);return !!source&&!assignment?.ports.length&&assignment?.sourceId!==source.id;});
}

/** Reconcile old game-wide pins only when every saved declaration has the exact advertised contract. */
export function profileReconcileDcsVehicleCatalogue(value:DeepReadonly<ProfileGraphDraft>,tabId:string,source:DeepReadonly<ResourceSummary>,ports:readonly DeepReadonly<ObservedResourcePort>[],inventory:readonly DeepReadonly<ResourceSummary>[]):ProfileGraphDraft {
  const tab=value.document.vehicleTabs?.find(tab=>tab.id===tabId);
  if(!tab||tab.gameId!=='dcs'||!profileDcsResourceMatchesVehicle(source,tab.vehicleId))throw Error('The DCS-BIOS catalogue does not match this aircraft tab.');
  let next=JSON.parse(JSON.stringify(value)) as ProfileGraphDraft;
  for(const block of next.document.resources.filter(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id))){
    const assignment=next.assignments.find(assignment=>assignment.resourceId===block.id);if(assignment?.sourceId===source.id)continue;
    const previous=inventory.find(resource=>resource.id===assignment?.sourceId),module=previous?.attributes.aircraftModule;
    if(block.ports.length&&module&&module!=='CommonData'&&module!==source.attributes.aircraftModule)throw Error('This aircraft tab has controls configured for another DCS-BIOS module. Remove those controls or add a new aircraft tab before changing its aircraft.');
    const selections=block.ports.map(port=>{const pin=assignment?.ports.find(pin=>pin.profilePortId===port.id),observed=pin&&ports.find(observed=>observed.descriptor.id===pin.observedPortId&&observed.contractHash===pin.expectedContractHash);if(!observed)throw Error(`The ${tab.vehicleName} catalogue cannot preserve “${port.name}”. Remove or explicitly reassign that control before changing this tab's aircraft.`);return {profilePortId:port.id,observed};});
    next=graphAssignResource(next,block.id,source,selections);
  }
  return next;
}

/** Changing a configured aircraft cannot silently relabel its controls as another module. */
export function profileChangeVehicleIntegration(value:DeepReadonly<ProfileGraphDraft>,tabId:string,vehicle:{id:string;name:string},inventory:readonly DeepReadonly<ResourceSummary>[]):ProfileGraphDraft {
  const document=profileChangeVehicleTab(value.document,tabId,vehicle),tab=document.vehicleTabs!.find(tab=>tab.id===tabId)!;
  if(tab.gameId==='dcs')for(const block of document.resources.filter(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id))){
    const assignment=value.assignments.find(assignment=>assignment.resourceId===block.id),previous=inventory.find(source=>source.id===assignment?.sourceId);
    if(block.ports.length&&(!previous||!profileDcsResourceMatchesVehicle(previous,vehicle.id)))throw Error('Remove the configured DCS controls or add a new aircraft tab before changing this tab’s aircraft.');
  }
  const emptyDcsBlocks=new Set(tab.gameId==='dcs'?document.resources.filter(resource=>resource.kind==='game'&&tab.nodeIds.includes(resource.id)&&!resource.ports.length).map(resource=>resource.id):[]);
  return profileSetGameContext({document,assignments:value.assignments.filter(assignment=>!emptyDcsBlocks.has(assignment.resourceId)||!!inventory.find(source=>source.id===assignment.sourceId&&profileDcsResourceMatchesVehicle(source,vehicle.id)))},profileGameContext(document),inventory);
}
