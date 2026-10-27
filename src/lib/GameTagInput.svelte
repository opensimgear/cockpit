<script lang="ts">
  import {tick} from 'svelte';
  import Icon from './Icon.svelte';
  import type {GameTags} from './games';
  let {tags,values,disabled=false,label='Tags',placeholder='Search tags…',multiline=false,removeLabel='Remove',onchange,oncreate,onrename}:{tags:GameTags;values:string[];disabled?:boolean;label?:string;placeholder?:string;multiline?:boolean;removeLabel?:string;onchange:(ids:string[])=>Promise<void>|void;oncreate?:(name:string)=>Promise<void>|void;onrename?:(id:string,name:string)=>Promise<void>|void}=$props();
  const id=$props.id();
  let query=$state(''),open=$state(false),active=$state(0),pending=$state(false),error=$state(''),editing=$state<string|null>(null),input=$state<HTMLInputElement|HTMLTextAreaElement>();
  const options=$derived(Object.entries(tags).filter(([key,name])=>!values.includes(key)&&name.toLowerCase().includes(query.trim().toLowerCase())).sort((a,b)=>a[1].localeCompare(b[1])));
  const activeIndex=$derived(Math.min(active,Math.max(0,options.length-1)));
  async function perform(action:()=>Promise<void>|void){
    if(disabled||pending)return;
    pending=true;error='';
    try{await action();query='';active=0;editing=null;}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}
    finally{pending=false;await tick();input?.focus();}
  }
  function change(next:string[]){return perform(()=>onchange(next));}
  function rename(key:string){editing=key;query=tags[key]??key;open=false;error='';input?.focus();input?.select();}
  function add(key:string){if(!values.includes(key))void change([...values,key]);}
  function keydown(event:KeyboardEvent){
    if(event.isComposing||disabled||pending)return;
    if(event.key==='ArrowDown'||event.key==='ArrowUp'){
      event.preventDefault();
      active=open&&options.length?(activeIndex+(event.key==='ArrowDown'?1:-1)+options.length)%options.length:0;open=true;
      void tick().then(()=>input?.ownerDocument.getElementById(`${id}-option-${activeIndex}`)?.scrollIntoView({block:'nearest'}));
    }else if(event.key==='Enter'){
      event.preventDefault();
      if(editing&&onrename)void perform(()=>onrename!(editing!,query.trim()));
      else if(open&&options.length)add(options[activeIndex][0]);
      else if(oncreate&&query.trim())void perform(()=>oncreate!(query.trim()));
    }else if(event.key==='Escape'){if(editing||!multiline)event.preventDefault();if(editing){query='';editing=null;}open=false;}
    else if(event.key==='Backspace'&&!query&&!editing&&values.length){event.preventDefault();void change(values.slice(0,-1));}
  }
</script>

<div class="tag-input">
  <label for={id}>{label}</label>
  <div class="tag-field" role="group" aria-label={label} class:multiline class:disabled={disabled||pending} onpointerdown={event=>{if(event.target===event.currentTarget){event.preventDefault();input?.focus();}}} onfocusout={event=>{if(!event.currentTarget.contains(event.relatedTarget as Node))open=false;}}>
    {#each values as key (key)}
      <span class="tag-chip" class:editing={editing===key} data-tag-id={key}>{#if onrename}<button type="button" class="tag-name" aria-label={`Rename ${tags[key]??key}`} disabled={disabled||pending} onclick={()=>rename(key)}><span>{tags[key]??key}</span><Icon name="pencil" size={10}/></button>{:else}<span>{tags[key]??key}</span>{/if}<button type="button" aria-label={`${removeLabel} ${tags[key]??key}`} disabled={disabled||pending} onclick={()=>void change(values.filter(value=>value!==key))}><Icon name="close" size={11}/></button></span>
    {/each}
    {#if multiline}
      <textarea bind:this={input} {id} bind:value={query} {placeholder} rows="1" disabled={disabled||pending} aria-label={editing?`New name for ${tags[editing]}`:label} onkeydown={keydown}></textarea>
    {:else}
      <input bind:this={input} {id} bind:value={query} {placeholder} disabled={disabled||pending} role="combobox" aria-autocomplete="list" aria-expanded={open&&!disabled&&!pending} aria-controls={`${id}-options`} aria-activedescendant={open&&options.length?`${id}-option-${activeIndex}`:undefined} onfocus={()=>open=true} oninput={()=>{open=true;active=0;}} onkeydown={keydown}/>
    {/if}
    {#if open&&!disabled&&!pending&&!multiline}
      <div id={`${id}-options`} class="suggestions" role="listbox" aria-label={`${label} suggestions`}>
        {#each options as [key,name],index (key)}
          <button type="button" tabindex="-1" id={`${id}-option-${index}`} role="option" aria-selected={activeIndex===index} class:active={activeIndex===index} onpointerdown={event=>event.preventDefault()} onclick={()=>add(key)}>{name}</button>
        {:else}<p>No matching tags</p>{/each}
      </div>
    {/if}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .tag-input{min-width:0}label{display:block;font-size:var(--ui-text-size);font-weight:570;color:var(--soft);margin-bottom:10px}
  .tag-field{position:relative;display:flex;align-items:center;flex-wrap:wrap;gap:6px;min-height:40px;padding:7px;border:1px solid var(--line);border-radius:5px;background:var(--input-bg)}
  .tag-field.multiline{min-height:180px;max-height:340px;overflow-y:auto;align-content:flex-start;align-items:center}.tag-chip.editing{outline:1px solid var(--accent)}
  .disabled{opacity:.65}
  .tag-chip{display:inline-flex;align-items:center;gap:5px;max-width:100%;padding:4px 6px;border-radius:4px;background:var(--accent-bg);color:var(--text);font-size:var(--ui-text-size)}.tag-chip>span{overflow-wrap:anywhere}
  button{display:flex;align-items:center;justify-content:center;width:20px;height:20px;padding:0;border:0;background:transparent;color:var(--soft);cursor:pointer}button:hover{color:var(--text)}button:focus-visible{outline:2px solid var(--accent)}
  button.tag-name{width:auto;height:auto;min-width:0;gap:5px;text-align:left;font:inherit;color:inherit}.tag-name span{overflow-wrap:anywhere}
  input,textarea{flex:1;min-width:100px;width:100px;border:0;outline:0;background:transparent;color:var(--text);font:inherit;font-size:var(--ui-text-size);padding:4px}
  textarea{resize:none;line-height:20px;min-width:160px;min-height:28px;box-sizing:border-box}input:focus-visible,textarea:focus-visible{outline:0}
  .suggestions{position:absolute;z-index:20;top:calc(100% + 5px);left:0;right:0;max-height:230px;overflow-y:auto;padding:5px;background:var(--surface);border:1px solid var(--line);border-radius:5px;box-shadow:0 8px 24px #0004}
  [role=option]{display:block;width:100%;height:auto;text-align:left;padding:9px;border-radius:3px;font-size:var(--ui-text-size);cursor:pointer}[role=option]:hover,[role=option].active{background:var(--accent-bg);color:var(--text)}p{margin:7px;color:var(--muted);font-size:var(--ui-text-size)}.error{color:#e5b19b}
</style>
