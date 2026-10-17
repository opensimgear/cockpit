import {DEFAULT_PROFILE_ID,MAX_PROFILES,parseProfileDocument,exportProfileDocument,type DeepReadonly,type ProfileSummary,type ProfilesClient} from './profiles';
export type ProfileLibraryClient=Pick<ProfilesClient,'delete'|'import'|'export'>;
export type SavedProfileTarget={id:string;name:string;revision:number};
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(v);
const canonical=(value:unknown):string=>JSON.stringify(value&&typeof value==='object'?Array.isArray(value)?value.map(v=>JSON.parse(canonical(v))):Object.fromEntries(Object.entries(value).sort(([a],[b])=>a<b?-1:a>b?1:0).map(([key,v])=>[key,JSON.parse(canonical(v))])):value);
export function savedProfileTarget(value:DeepReadonly<ProfileSummary>|SavedProfileTarget):Readonly<SavedProfileTarget>{if(!uuid(value.id)||!Number.isSafeInteger(value.revision)||value.revision<1||typeof value.name!=='string'||!value.name.trim())throw Error('Choose an exact saved profile revision.');return Object.freeze({id:value.id,name:value.name,revision:value.revision});}
export function profileImportNameError(name:string):string{return !name.trim()||[...name.trim()].length>80||/[\u0000-\u001f\u007f-\u009f]/.test(name)||name.trim()==='Default'?'Choose a name of 1–80 characters other than the protected Default name.':'';}
export async function deleteSavedProfile(client:ProfileLibraryClient,value:SavedProfileTarget,readOnly:boolean):Promise<void>{
 const target=savedProfileTarget(value);if(readOnly)throw Error('The profile library is read-only. Resolve its diagnostics before deleting.');if(target.id===DEFAULT_PROFILE_ID)throw Error('The built-in Default profile cannot be deleted.');
 const result=await client.delete(target.id,target.revision);if(result.deletedId!==target.id)throw Error('Deletion did not confirm this exact profile. Refresh the library before trying again.');
}
export async function importSavedProfile(client:ProfileLibraryClient,source:string,name:string,profileCount:number,readOnly:boolean){
 if(readOnly)throw Error('The profile library is read-only. Resolve its diagnostics before importing.');if(!Number.isInteger(profileCount)||profileCount<1||profileCount>=MAX_PROFILES)throw Error('The profile library is full or unavailable (32 including Default). Delete an unused named profile first.');
 const error=profileImportNameError(name);if(error)throw Error(error);const portable=parseProfileDocument(source),captured=exportProfileDocument(portable),saved=await client.import(captured,name.trim());
 const checked=parseProfileDocument(exportProfileDocument(saved));if(checked.id===portable.id||checked.id===DEFAULT_PROFILE_ID||checked.revision!==1||checked.name!==name.trim()||canonical(checked)!==canonical({...portable,id:checked.id,revision:1,name:name.trim()}))throw Error('Import did not confirm a fresh independent profile. Refresh the library before trying again.');return checked;
}
export async function exportSavedProfile(client:ProfileLibraryClient,value:SavedProfileTarget){const target=savedProfileTarget(value),document=parseProfileDocument(await client.export(target.id));if(document.id!==target.id)throw Error('Export returned another profile. Refresh the saved profile before exporting.');return{document,text:exportProfileDocument(document)};}
export function profileExportFilename(name:string):string{return (name.replace(/[^a-zA-Z0-9._-]/g,'-').replace(/^-+|-+$/g,'').slice(0,80)||'profile')+'.json';}
