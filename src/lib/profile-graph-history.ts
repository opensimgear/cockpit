import { graphDraft, type ProfileGraphDraft } from './profile-graph';
import type { DeepReadonly } from './profiles';

type Entry={label:string;value:ProfileGraphDraft};
const frozen=<T>(value:T):DeepReadonly<T>=>{if(value&&typeof value==='object'){Object.values(value).forEach(frozen);Object.freeze(value);}return value as DeepReadonly<T>;};
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
    if(JSON.stringify(next)===JSON.stringify(current))return current;
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
