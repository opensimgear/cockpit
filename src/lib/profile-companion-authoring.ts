import { graphAssignResource, graphDraft, graphRemovePort, graphSelectProviderPort, type ProfileGraphDraft, type ProfileGraphSelection } from './profile-graph';
import { resourcePortError, type ObservedResourcePort, type ResourceSummary } from './profile-inventory';
import type { DeepReadonly } from './profiles';

export type ProfileCompanionPanel = { publicationHash:string;capabilityFingerprint:string;title:string;controlCount:number;assetCount:number };
export type ProfileCompanionCatalogue = {schemaVersion:1;revision:string;panels:ProfileCompanionPanel[];diagnostics:string[]};
export type ProfileCompanionPage = {schemaVersion:1;resourceId:string;publicationHash:string;capabilityFingerprint:string;identityFingerprint:string;offset:number;total:number;ports:ObservedResourcePort[];diagnostics:string[]};
export type ProfileCompanionPortSelection = ProfileGraphSelection & {page:DeepReadonly<ProfileCompanionPage>};
export type ProfileCompanionAuthoringCommand = 'profile_companion_catalogue'|'profile_companion_inspect';
export type ProfileCompanionAuthoringRequest = {command:'profile_companion_catalogue'}|{command:'profile_companion_inspect';resourceId:string;publicationHash:string;offset:number;limit:number};
export type ProfileCompanionAuthoringResult = ProfileCompanionCatalogue|ProfileCompanionPage;
export type ProfileCompanionAuthoringSend = (request:ProfileCompanionAuthoringRequest)=>Promise<unknown>;
const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:unknown,keys:string[]):value is Record<string,unknown>=>object(value)&&Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const text=(value:unknown,max:number):value is string=>typeof value==='string'&&value.length>0&&new TextEncoder().encode(value).length<=max&&!/[\u0000-\u001f\u007f-\u009f]/.test(value);
const hash=(value:unknown):value is string=>typeof value==='string'&&/^sha256:[0-9a-f]{64}$/.test(value);
const integer=(value:unknown,min:number,max:number):value is number=>Number.isSafeInteger(value)&&Number(value)>=min&&Number(value)<=max;
const diagnostics=(value:unknown)=>Array.isArray(value)&&value.length<=32&&value.every(item=>text(item,4096));
const freeze=<T>(value:T):DeepReadonly<T>=>{if(value&&typeof value==='object'){Object.values(value).forEach(freeze);Object.freeze(value);}return value as DeepReadonly<T>;};
const capture=<T>(value:T):T=>JSON.parse(JSON.stringify(value));
export function companionPanelValid(value:unknown):value is ProfileCompanionPanel {
  return exact(value,['publicationHash','capabilityFingerprint','title','controlCount','assetCount'])&&hash(value.publicationHash)&&hash(value.capabilityFingerprint)&&text(value.title,1024)&&integer(value.controlCount,1,128)&&integer(value.assetCount,0,64);
}
function catalogue(value:unknown):ProfileCompanionCatalogue {
  if(!exact(value,['schemaVersion','revision','panels','diagnostics'])||value.schemaVersion!==1||!hash(value.revision)||!diagnostics(value.diagnostics)||!Array.isArray(value.panels)||value.panels.length>32||!value.panels.every(companionPanelValid)||new Set(value.panels.map(panel=>panel.publicationHash)).size!==value.panels.length)throw new Error('The native retained companion panel catalogue is malformed or exceeds its bounds.');
  return value as ProfileCompanionCatalogue;
}
function page(value:unknown,request:Extract<ProfileCompanionAuthoringRequest,{command:'profile_companion_inspect'}>):ProfileCompanionPage {
  if(!exact(value,['schemaVersion','resourceId','publicationHash','capabilityFingerprint','identityFingerprint','offset','total','ports','diagnostics'])||value.schemaVersion!==1||value.resourceId!==request.resourceId||value.publicationHash!==request.publicationHash||!hash(value.capabilityFingerprint)||!hash(value.identityFingerprint)||value.offset!==request.offset||!integer(value.total,1,1024)||!Array.isArray(value.ports)||value.ports.length>request.limit||value.offset+value.ports.length>value.total||(value.offset<value.total&&!value.ports.length)||!diagnostics(value.diagnostics))throw new Error('The native companion port page changed identity or exceeds its bounds.');
  const ids=new Set<string>();
  for(const port of value.ports){
    if(!exact(port,['descriptor','contractHash','routeIds','current'])||resourcePortError(port.descriptor)||!hash(port.contractHash)||!Array.isArray(port.routeIds)||port.routeIds.length!==0||port.current!==false||ids.has((port.descriptor as {id:string}).id))throw new Error('A prospective companion port is malformed or claims live delivery authority.');
    ids.add((port.descriptor as {id:string}).id);
  }
  return value as ProfileCompanionPage;
}
/** Read-only native metadata. Browsing never uploads, assigns or activates. */
export class ProfileCompanionAuthoringClient {
  constructor(private send:ProfileCompanionAuthoringSend){}
  async catalogue():Promise<DeepReadonly<ProfileCompanionCatalogue>> {return freeze(capture(catalogue(await this.send({command:'profile_companion_catalogue'}))));}
  async inspect(resourceId:string,publicationHash:string,offset=0,limit=32):Promise<DeepReadonly<ProfileCompanionPage>> {
    if(!text(resourceId,256)||!hash(publicationHash)||!integer(offset,0,1024)||!integer(limit,1,64))throw new Error('Choose an exact companion resource, published panel and bounded port page.');
    const request={command:'profile_companion_inspect' as const,resourceId,publicationHash,offset,limit};
    return freeze(capture(page(await this.send(request),request)));
  }
}
/** Offset follows actual page length; a changed paired identity requires refresh. */
export class ProfileCompanionPortBrowser {
  private epoch=0;private offsets:number[]=[];private current:DeepReadonly<ProfileCompanionPage>|null=null;
  constructor(private client:Pick<ProfileCompanionAuthoringClient,'inspect'>){}
  get page(){return this.current;}
  get hasNext(){return !!this.current&&this.current.offset+this.current.ports.length<this.current.total;}
  get hasPrevious(){return this.offsets.length>1;}
  clear(){this.epoch++;this.offsets=[];this.current=null;}
  async open(resourceId:string,publicationHash:string){this.clear();return this.load(resourceId,publicationHash,0,false);}
  async next(){if(!this.current||!this.hasNext)return this.current;return this.load(this.current.resourceId,this.current.publicationHash,this.current.offset+this.current.ports.length,true);}
  async previous(){if(!this.current||!this.hasPrevious)return this.current;const result=await this.load(this.current.resourceId,this.current.publicationHash,this.offsets[this.offsets.length-2],true);if(result)this.offsets.splice(-2,2,result.offset);return result;}
  private async load(resourceId:string,publicationHash:string,offset:number,continued:boolean){
    const epoch=++this.epoch,previous=this.current,result=await this.client.inspect(resourceId,publicationHash,offset,32);
    if(epoch!==this.epoch)return null;
    if(continued&&previous&&(previous.identityFingerprint!==result.identityFingerprint||previous.capabilityFingerprint!==result.capabilityFingerprint||previous.total!==result.total))throw new Error('This paired companion or immutable panel catalogue changed. Refresh its ports before continuing.');
    this.current=result;if(!continued)this.offsets=[offset];else if(!this.offsets.includes(offset))this.offsets.push(offset);return result;
  }
}
export function graphCaptureCompanionPanel(value:DeepReadonly<ProfileGraphDraft>,resourceId:string,panel:DeepReadonly<ProfileCompanionPanel>,clearPorts=false):ProfileGraphDraft {
  let next=graphDraft(value);const resource=next.document.resources.find(resource=>resource.id===resourceId),assignment=next.assignments.find(assignment=>assignment.resourceId===resourceId);
  if(resource?.kind!=='companion'||!assignment||!companionPanelValid(panel))throw new Error('Assign this companion block locally and choose an actual retained panel first.');
  const current=next.document.companionAssignments.find(item=>item.resourceId===resourceId);
  if(next.document.companionAssignments.some(item=>item.resourceId!==resourceId&&next.assignments.find(assigned=>assigned.resourceId===item.resourceId)?.sourceId===assignment.sourceId))throw new Error('This paired companion already belongs to another captured panel block.');
  if(!current&&next.document.companionAssignments.length>=8)throw new Error('One profile can own at most eight companion panel instances.');
  if(current?.publicationHash!==panel.publicationHash&&resource.ports.length){
    if(!clearPorts)throw new Error('Changing the panel requires explicit removal of this companion’s selected ports and routes before selecting replacement contracts.');
    for(const port of resource.ports)next=graphRemovePort(next,resourceId,port.id);
  }
  next.document.companionAssignments=next.document.companionAssignments.filter(item=>item.resourceId!==resourceId);
  next.document.companionAssignments.push({resourceId,publicationHash:panel.publicationHash});return graphDraft(next);
}
export function graphReleaseCompanionPanel(value:DeepReadonly<ProfileGraphDraft>,resourceId:string):ProfileGraphDraft {
  const next=graphDraft(value);if(!next.document.resources.some(resource=>resource.id===resourceId&&resource.kind==='companion'))throw new Error('Choose an existing companion block.');
  next.document.companionAssignments=next.document.companionAssignments.filter(item=>item.resourceId!==resourceId);return next;
}
/** Reconciliation is explicit for every existing logical port, so a copied
 * feedback graph can select desired B even when its paired device still has A. */
export function graphReconcileCompanionPanel(value:DeepReadonly<ProfileGraphDraft>,resourceId:string,source:DeepReadonly<ResourceSummary>,panel:DeepReadonly<ProfileCompanionPanel>,observedPage:DeepReadonly<ProfileCompanionPage>,selections:readonly ProfileCompanionPortSelection[]):ProfileGraphDraft {
  if(source.resourceType!=='companion'||!companionPanelValid(panel)||observedPage.resourceId!==source.id||observedPage.publicationHash!==panel.publicationHash||observedPage.capabilityFingerprint!==panel.capabilityFingerprint)throw new Error('Inspect the exact retained panel for this selected paired companion before reconciliation.');
  for(const selection of selections){
    const captured=selection.page;
    if(!captured||captured.resourceId!==observedPage.resourceId||captured.publicationHash!==observedPage.publicationHash||captured.capabilityFingerprint!==observedPage.capabilityFingerprint||captured.identityFingerprint!==observedPage.identityFingerprint||captured.total!==observedPage.total||!captured.ports.some(port=>JSON.stringify(port)===JSON.stringify(selection.observed)))throw new Error('Every reconciled port must come from the same captured native companion/publication catalogue. Refresh changed pages.');
  }
  const next=graphAssignResource(value,resourceId,source,selections);
  if(next.document.companionAssignments.some(item=>item.resourceId!==resourceId&&next.assignments.find(local=>local.resourceId===item.resourceId)?.sourceId===source.id))throw new Error('This paired companion already belongs to another captured panel block.');
  if(!next.document.companionAssignments.some(item=>item.resourceId===resourceId)&&next.document.companionAssignments.length>=8)throw new Error('One profile can own at most eight companion panel instances.');
  next.document.companionAssignments=next.document.companionAssignments.filter(item=>item.resourceId!==resourceId);next.document.companionAssignments.push({resourceId,publicationHash:panel.publicationHash});return graphDraft(next);
}
export function graphSelectCompanionPort(value:DeepReadonly<ProfileGraphDraft>,resourceId:string,observedPage:DeepReadonly<ProfileCompanionPage>,port:DeepReadonly<ObservedResourcePort>):ProfileGraphDraft {
  const assignment=value.assignments.find(item=>item.resourceId===resourceId),captured=value.document.companionAssignments.find(item=>item.resourceId===resourceId);
  if(!assignment||assignment.sourceId!==observedPage.resourceId||captured?.publicationHash!==observedPage.publicationHash||!observedPage.ports.some(item=>JSON.stringify(item)===JSON.stringify(port)))throw new Error('Capture this exact panel for the assigned companion before choosing one of its declared ports.');
  return graphSelectProviderPort(value,resourceId,assignment.sourceId,port);
}
