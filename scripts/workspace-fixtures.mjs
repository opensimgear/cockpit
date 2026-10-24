// One current source/draft/publication crosses the JS compiler and Rust validator.
import {readFile,writeFile} from 'node:fs/promises';
import {compilePanelDraft} from '../src/lib/compiled-companion-library.ts';
const source=JSON.parse(await readFile(new URL('../examples/companion/generic-toggle.source.json',import.meta.url),'utf8'));
const draft={id:'toggle-native-fixture',title:'Saved compiled panel',pages:[{id:'main-page',title:'Main',width:640,height:400}],widgets:[{id:'toggle-instance',pageId:'main-page',label:'Toggle',sourceId:source.id,parameters:{},bounds:{x:.1,y:.1,width:.2,height:.2}}]};
const publication=await compilePanelDraft(draft,[source]);
const output=JSON.stringify({sources:[source],drafts:[draft],publications:{[draft.id]:[publication]}})+'\n';
const path=new URL('../crates/companion-service/tests/fixtures/compiled-workspace.json',import.meta.url);
if(process.argv.includes('--check')){if((await readFile(path,'utf8')).replaceAll('\r\n','\n')!==output)throw Error('Compiled workspace fixture differs from the current source model.');}
else await writeFile(path,output);
