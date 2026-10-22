import manifest from '../assets/widget-profiles.json' with { type: 'json' };
import type { Validation } from '../../companion-contracts/src/index.ts';

export interface RenderProfile {
  formatVersion: 1; engine: string; engineVersion: 1; fontSet: string;
  textFormat: typeof manifest.textFormat;
  id: string; features: string[]; limits: Record<keyof typeof manifest.profiles.compact.limits,number>;
  firmwareVersion: string; boardProfile: string; screen: {width:number;height:number};
}
const fields=['formatVersion','engine','engineVersion','fontSet','textFormat','id','features','limits','firmwareVersion','boardProfile','screen'];
const plain=(v:unknown):v is Record<string,unknown>=>!!v&&typeof v==='object'&&!Array.isArray(v);
/** Validate an actual device report; no caller invents a profile from its name,
 * firmware date or dimensions. Profiles are content metadata, never authority. */
export function validateRenderProfile(input:unknown):Validation<RenderProfile>{
  try{
    const serialized=JSON.stringify(input);if(!serialized||new TextEncoder().encode(serialized).length>4096)throw Error('Renderer profile exceeds its metadata budget.');
    const value=JSON.parse(serialized) as Record<string,unknown>;
    if(!plain(value)||Object.keys(value).length!==fields.length||Object.keys(value).some(key=>!fields.includes(key)))throw Error('Invalid renderer profile fields.');
    for(const key of ['formatVersion','engine','engineVersion','fontSet'] as const)if(value[key]!==manifest[key])throw Error(`Unsupported renderer ${key}.`);
    const format=value.textFormat;
    if(!plain(format)||Object.keys(format).length!==Object.keys(manifest.textFormat).length||Object.entries(manifest.textFormat).some(([key,v])=>format[key]!==v))throw Error('Unsupported renderer text format.');
    const profile=Object.values(manifest.profiles).find(profile=>profile.id===value.id);
    if(!profile)throw Error('Unknown renderer profile ID.');
    const limits=value.limits;
    if(!plain(limits)||Object.keys(limits).length!==Object.keys(profile.limits).length||Object.entries(profile.limits).some(([key,n])=>limits[key]!==n))throw Error('Renderer limits do not match its versioned profile.');
    const features=value.features;
    if(!Array.isArray(features)||features.length!==manifest.features.length||new Set(features).size!==features.length||features.some(feature=>!manifest.features.includes(feature)))throw Error('Unsupported or repeated renderer feature.');
    if(typeof value.firmwareVersion!=='string'||! /^[!-~]{1,64}$/.test(value.firmwareVersion)||typeof value.boardProfile!=='string'||! /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(value.boardProfile))throw Error('Invalid renderer firmware/board identity.');
    const screen=value.screen;
    if(!plain(screen)||Object.keys(screen).length!==2||!['width','height'].every(key=>Number.isInteger(screen[key])&&Number(screen[key])>=64&&Number(screen[key])<=1024))throw Error('Invalid renderer geometry.');
    return {ok:true,value:{...value,features:[...manifest.features]} as unknown as RenderProfile};
  }catch(error){return {ok:false,error:error instanceof Error?error.message:'Invalid renderer profile.'};}
}
