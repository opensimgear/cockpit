import {supportedGames,type GameId} from './games';
import {canvasNodes} from './profile-canvas';
import {graphRemoveNode,type ProfileGraphDraft} from './profile-graph';
import {profileDocumentError,profileDocumentAssignmentsError,type DeepReadonly,type ProfileDocument} from './profiles';
import type {ResourceSummary} from './profile-inventory';

export function profileGameContext(document:Pick<DeepReadonly<ProfileDocument>,'eligibility'>):GameId|'' {
  return supportedGames.find(game=>game.id===document.eligibility.gameIds[0])?.id??'';
}
export function profileGameName(id:string):string{return supportedGames.find(game=>game.id===id)?.name??id;}

/** The saved context and its integration block change together in one Undo edit. */
export function profileSetGameContext(value:DeepReadonly<ProfileGraphDraft>,gameId:GameId|'',inventory:readonly DeepReadonly<ResourceSummary>[]):ProfileGraphDraft {
  if(gameId&&!supportedGames.some(game=>game.id===gameId))throw Error('Choose a supported game context or None.');
  let next=JSON.parse(JSON.stringify(value)) as ProfileGraphDraft;
  const before=canvasNodes(value.document,value.assignments,inventory),games=value.document.resources.filter(resource=>resource.kind==='game');
  const source=inventory.find(resource=>resource.resourceType==='game-integration'&&resource.attributes.gameId===gameId);
  const keep=gameId?games.find(resource=>{
    const assignment=value.assignments.find(assignment=>assignment.resourceId===resource.id),observed=inventory.find(resource=>resource.id===assignment?.sourceId);
    return observed?.attributes.gameId===gameId||profileGameContext(value.document)===gameId&&games.length===1&&!observed;
  }):undefined;
  const oldPosition=before.find(node=>node.resource?.kind==='game')?.position;
  const oldGroup=value.document.editor.groups?.find(group=>games[0]&&group.members.includes(games[0].id))?.id;
  for(const resource of games)if(resource.id!==keep?.id)next=graphRemoveNode(next,resource.id);
  next.document.eligibility.gameIds=gameId?[gameId]:[];
  if(gameId){
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
    if(source&&!next.assignments.some(assignment=>assignment.resourceId===resource.id)&&!resource.ports.length)next.assignments.push({resourceId:resource.id,sourceId:source.id,ports:[]});
  }
  const error=profileDocumentError(next.document)||profileDocumentAssignmentsError(next.document,next.assignments);if(error)throw Error(error);
  return next;
}

/** Empty context blocks can acquire their exact native integration when inventory arrives. */
export function profileGameContextNeedsSync(value:DeepReadonly<ProfileGraphDraft>,inventory:readonly DeepReadonly<ResourceSummary>[]):boolean {
  const id=profileGameContext(value.document);if(!id)return false;
  if(!value.document.resources.some(resource=>resource.kind==='game'))return true;
  return value.document.resources.some(resource=>resource.kind==='game'&&!resource.ports.length&&!value.assignments.some(assignment=>assignment.resourceId===resource.id))&&inventory.some(resource=>resource.resourceType==='game-integration'&&resource.attributes.gameId===id);
}
