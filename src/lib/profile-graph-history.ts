import { graphDraft, type ProfileGraphDraft } from './profile-graph';
import type { DeepReadonly } from './profiles';
import type { ProfileDocument, ProfileLocalAssignment } from './profiles';
import type { ProfileEditorStore } from './profile-editor';

type Entry={label:string;value:ProfileGraphDraft};
const frozen=<T>(value:T):DeepReadonly<T>=>{if(value&&typeof value==='object'){Object.values(value).forEach(frozen);Object.freeze(value);}return value as DeepReadonly<T>;};
/** Native Save may reorder object keys. Arrays retain their authored order. */
const graphStructure=(value:DeepReadonly<ProfileGraphDraft>):string=>JSON.stringify(value,(_key,item)=>item&&typeof item==='object'&&!Array.isArray(item)?Object.fromEntries(Object.entries(item).sort(([a],[b])=>a<b?-1:a>b?1:0)):item);
/** Bounded local authoring history. It has no persistence or runtime capability. */
export class ProfileGraphHistory {
  private entries:Entry[]=[];
  private cursor=0;
  constructor(initial:DeepReadonly<ProfileGraphDraft>,private capacity=64,private byteBudget=8*1024*1024){
    if(!Number.isInteger(capacity)||capacity<2||capacity>128||!Number.isSafeInteger(byteBudget)||byteBudget<1024||byteBudget>32*1024*1024)throw new Error('Choose a bounded graph history capacity.');
    this.reset(initial);
  }
  get current():DeepReadonly<ProfileGraphDraft>{return frozen(this.entries[this.cursor].value);}
  get canUndo():boolean{return this.cursor>0;}
  get canRedo():boolean{return this.cursor<this.entries.length-1;}
  get undoLabel():string{return this.canUndo?this.entries[this.cursor].label:'';}
  get redoLabel():string{return this.canRedo?this.entries[this.cursor+1].label:'';}
  reset(value:DeepReadonly<ProfileGraphDraft>):void{this.entries=[{label:'Opened profile',value:graphDraft(value)}];this.cursor=0;}
  commit(value:DeepReadonly<ProfileGraphDraft>,label:string):DeepReadonly<ProfileGraphDraft>{
    const next=graphDraft(value),current=this.current;
    if(next.document.id!==current.document.id||next.document.revision!==current.document.revision)throw new Error('Reset or rebase history when the editor identity or saved revision changes.');
    if(graphStructure(next)===graphStructure(current))return current;
    if(!label.trim())throw new Error('Describe this undoable graph edit.');
    this.entries=this.entries.slice(0,this.cursor+1);this.entries.push({label,value:next});this.cursor++;
    const size=()=>new TextEncoder().encode(JSON.stringify(this.entries)).length;
    while(this.entries.length>1&&(this.entries.length>this.capacity||size()>this.byteBudget)){this.entries.shift();this.cursor--;}
    return this.current;
  }
  undo():DeepReadonly<ProfileGraphDraft>{if(this.canUndo)this.cursor--;return this.current;}
  redo():DeepReadonly<ProfileGraphDraft>{if(this.canRedo)this.cursor++;return this.current;}
  /** A Save advances only optimistic revision; earlier semantic/layout edits remain undoable. */
  rebaseRevision(revision:number):void{
    if(!Number.isSafeInteger(revision)||revision<this.current.document.revision)throw new Error('Use the current or a newer saved revision.');
    this.entries=this.entries.map(entry=>({...entry,value:graphDraft({...entry.value,document:{...entry.value.document,revision}})}));
  }
}

/** One editor epoch owns history; saved revisions never become an undoable activation. */
export class ProfileEditorUndo {
  private history:ProfileGraphHistory|null=null;
  private identity='';
  constructor(private store:Pick<ProfileEditorStore,'state'|'editorEpoch'|'update'>){}
  private sync():ProfileGraphHistory|null{
    const editor=this.store.state.editor;
    if(!editor){this.history=null;this.identity='';return null;}
    const identity=`${editor.document.id}/${this.store.editorEpoch}`,value=graphDraft({document:editor.document,assignments:editor.assignments});
    if(identity!==this.identity||!this.history){this.identity=identity;this.history=new ProfileGraphHistory(value);return this.history;}
    if(this.history.current.document.revision!==editor.document.revision)this.history.rebaseRevision(editor.document.revision);
    if(graphStructure(this.history.current)!==graphStructure(value))this.history.commit(value,'Edit profile graph');
    return this.history;
  }
  get status():{canUndo:boolean;canRedo:boolean;undoLabel:string;redoLabel:string}{const history=this.sync();return{canUndo:history?.canUndo??false,canRedo:history?.canRedo??false,undoLabel:history?.undoLabel??'',redoLabel:history?.redoLabel??''};}
  edit(document:ProfileDocument,assignments?:ProfileLocalAssignment[],label='Edit profile graph'):void{
    const history=this.sync();if(!history)throw new Error('Open a profile before editing its history.');
    const value=graphDraft({document,assignments:assignments??this.store.state.editor!.assignments});
    this.store.update(value.document,value.assignments);history.commit(value,label);
  }
  step(direction:'undo'|'redo'):boolean{
    if(this.store.state.readOnly||this.store.state.busy)return false;
    const history=this.sync();if(!history||!(direction==='undo'?history.canUndo:history.canRedo))return false;
    const value=graphDraft(direction==='undo'?history.undo():history.redo());this.store.update(value.document,value.assignments);return true;
  }
}
