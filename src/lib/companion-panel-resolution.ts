import {panelResolutionWarning} from '../../packages/widget-runtime/src/panel-resolution.ts';
import type {CompanionHostState} from './companions.ts';
import type {CompiledDraft} from './compiled-companion-library.ts';

/** Warnings use current destination geometry, never the editor preview size. */
export function panelDestinationWarnings(page: CompiledDraft['pages'][number] | undefined, companions: readonly string[], state: CompanionHostState): {companionId:string;message:string}[] {
  if(!page)return [];
  const warnings:{companionId:string;message:string}[]=[];
  for(const client of state.host?.clients??[]){
    if(client.connected!==true||!companions.includes(String(client.companionId))||!Array.isArray(client.displayReports))continue;
    const width=Math.max(0,...client.displayReports.map(report=>report&&typeof report.displayWidth==='number'&&Number.isFinite(report.displayWidth)&&report.displayWidth<=65536?report.displayWidth:0));
    const height=Math.max(0,...client.displayReports.map(report=>report&&typeof report.displayHeight==='number'&&Number.isFinite(report.displayHeight)&&report.displayHeight<=65536?report.displayHeight:0));
    const message=panelResolutionWarning(page,width,height,1);
    if(message)warnings.push({companionId:String(client.companionId),message:`${String(client.name??client.companionId)}: ${message}`});
  }
  return warnings;
}
