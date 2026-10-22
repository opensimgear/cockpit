<script lang="ts">
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';

  type PortOption = { value: string; label: string; description?: string; searchText?: string; reason?: string };
  let {
    label,
    placeholder = 'Search ports by name or ID',
    options,
    value = $bindable(''),
    query = $bindable(''),
    disabled = false,
    compact = false,
    limit = 200,
    onselect,
  }: {
    label: string;
    placeholder?: string;
    options: readonly PortOption[];
    value?: string;
    query?: string;
    disabled?: boolean;
    compact?: boolean;
    limit?: number;
    onselect?: (value:string)=>void;
  } = $props();

  const uid = $props.id();
  let open = $state(false);
  let activeIndex = $state(-1);
  let listbox = $state<HTMLDivElement>();
  let inputElement=$state<HTMLInputElement>();
  let dropdownAbove=$state(false),dropdownHeight=$state(280);
  let dropdownPosition=$state('');
  let scrollTop=$state(0);
  const rowHeight=76;
  const selected = $derived(options.find(option => option.value === value));
  const pageLimit = $derived(Math.max(1, Math.min(200, Math.floor(limit) || 200)));
  const indexed=$derived(options.map(option=>({option,text:`${option.label} ${option.description??''} ${option.searchText??''}`.toLowerCase()})));
  const matches = $derived.by(() => {
    const search = query.trim().toLowerCase();
    return search ? indexed.filter(item=>item.text.includes(search)).map(item=>item.option) : options;
  });
  const windowStart=$derived(Math.max(0,Math.floor(scrollTop/rowHeight)-3));
  const visibleOptions = $derived(matches.slice(windowStart, windowStart+Math.min(pageLimit,16)));
  const displayValue = $derived(open ? query : selected?.label ?? query);
  const activeId = $derived(open && activeIndex >= 0 && activeIndex < matches.length ? `${uid}-option-${activeIndex}` : undefined);

  $effect(() => {
    if (disabled && open) close();
    if (activeIndex >= matches.length) activeIndex = matches.length ? 0 : -1;
  });

  $effect(() => {
    if (!open || activeIndex < 0) return;
    const index = activeIndex;
    void tick().then(() => {
      if(open&&activeIndex===index&&listbox){const top=index*rowHeight;if(top<listbox.scrollTop)listbox.scrollTop=top;else if(top+rowHeight>listbox.scrollTop+listbox.clientHeight)listbox.scrollTop=top+rowHeight-listbox.clientHeight;scrollTop=listbox.scrollTop;}
    });
  });

  function dropdownPopover(element:HTMLDivElement){
    if(compact)element.showPopover();
    return{destroy(){if(compact&&element.matches(':popover-open'))element.hidePopover();}};
  }

  function positionDropdown(){
    if(inputElement&&typeof window!=='undefined'){const rect=inputElement.getBoundingClientRect(),below=window.innerHeight-rect.bottom-60,above=rect.top-60;dropdownAbove=below<200&&above>below;dropdownHeight=Math.max(76,Math.min(280,dropdownAbove?above:below));if(compact){const width=Math.min(Math.max(rect.width,280),window.innerWidth-24),left=Math.max(12,Math.min(rect.left,window.innerWidth-width-12));dropdownPosition=`position:fixed;left:${left}px;right:auto;width:${width}px;z-index:60;${dropdownAbove?`top:auto;bottom:${window.innerHeight-rect.top}px`:`top:${rect.bottom}px;bottom:auto`}`;}}
  }

  function show() {
    if (disabled || open) return;
    positionDropdown();
    query = '';
    open = true;
    scrollTop=0;
    const selectedIndex = options.findIndex(option => option.value === value);
    activeIndex = options.length ? selectedIndex >= 0 ? selectedIndex : 0 : -1;
  }

  function close() {
    open = false;
    query = '';
    activeIndex = -1;
  }

  function select(option: PortOption) {
    if (disabled) return;
    value = option.value;
    close();
    onselect?.(option.value);
  }

  function input(event: Event) {
    if (disabled) return;
    query = (event.currentTarget as HTMLInputElement).value;
    value = '';
    open = true;
    activeIndex = 0;
    scrollTop=0;if(listbox)listbox.scrollTop=0;
  }

  function keydown(event: KeyboardEvent) {
    if (disabled) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) {
        show();
        if (event.key === 'ArrowUp') activeIndex = matches.length - 1;
      } else if (matches.length) {
        activeIndex = (activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + matches.length) % matches.length;
      }
    } else if (event.key === 'Enter' && open) {
      event.preventDefault();
      if (matches[activeIndex]) select(matches[activeIndex]);
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === 'Tab') {
      close();
    }
  }

  function focusout(event: FocusEvent) {
    const container = event.currentTarget as HTMLDivElement;
    if (!(event.relatedTarget instanceof Node) || !container.contains(event.relatedTarget)) close();
  }
</script>

<svelte:window onresize={()=>{if(compact&&open)positionDropdown();}}/>
<svelte:document onscrollcapture={event=>{if(compact&&open&&!(event.target instanceof Element&&event.target.closest('.port-options')))positionDropdown();}}/>
<div class="port-combobox" class:compact onfocusout={focusout}>
  <label id={`${uid}-label`} for={`${uid}-input`}>{label}</label>
  <div class="input-wrap">
    <input
      id={`${uid}-input`}
      bind:this={inputElement}
      role="combobox"
      aria-label={label}
      aria-autocomplete="list"
      aria-expanded={open && !disabled}
      aria-controls={`${uid}-listbox`}
      aria-activedescendant={activeId}
      aria-describedby={open ? `${uid}-count` : undefined}
      data-value={value}
      autocomplete="off"
      spellcheck="false"
      {placeholder}
      {disabled}
      value={displayValue}
      onfocus={show}
      onclick={show}
      oninput={input}
      onkeydown={keydown}
    />
    <span class="chevron" aria-hidden="true"><Icon name="chevron" size={12}/></span>
  </div>
  {#if open && !disabled}
    <div class="port-dropdown" popover={compact?'manual':undefined} use:dropdownPopover class:above={dropdownAbove} style={compact?dropdownPosition:undefined}>
      <div id={`${uid}-listbox`} class="port-options" role="listbox" aria-labelledby={`${uid}-label`} style={`max-height:${dropdownHeight}px`} bind:this={listbox} onscroll={event=>scrollTop=event.currentTarget.scrollTop}>
        <div class="virtual-spacer" aria-hidden="true" style={`height:${windowStart*rowHeight}px`}></div>
        {#each visibleOptions as option, offset (option.value)}
          {@const index=windowStart+offset}
          <button
            id={`${uid}-option-${index}`}
            type="button"
            role="option"
            tabindex="-1"
            aria-selected={option.value === value}
            aria-posinset={index+1}
            aria-setsize={matches.length}
            data-value={option.value}
            class:active={index === activeIndex}
            onpointerdown={event => event.preventDefault()}
            onpointermove={() => activeIndex = index}
            onclick={() => select(option)}
          >
            <span class="port-name">{option.label}</span>
            {#if option.description}<span class="port-description">{option.description}</span>{/if}
            {#if option.reason}<span class="port-reason">{option.reason}</span>{/if}
          </button>
        {/each}
        <div class="virtual-spacer" aria-hidden="true" style={`height:${Math.max(0,matches.length-windowStart-visibleOptions.length)*rowHeight}px`}></div>
      </div>
      <p id={`${uid}-count`} class="port-count" role="status">
        {#if !matches.length}No matching ports.
        {:else}{matches.length} {matches.length === 1 ? 'result' : 'results'}{#if matches.length>16} · Scroll or type to filter.{/if}{/if}
      </p>
    </div>
  {/if}
</div>

<style>
  .port-combobox { position: relative; display: grid; gap: 6px; min-width: 0; }
  .compact label{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%);white-space:nowrap}
  .port-combobox.compact{gap:0}.port-combobox.compact input{height:28px;padding:4px 24px 4px 6px;font-size:11px}
  label { color: var(--muted); font-size: 10px; }
  .input-wrap { position: relative; min-width: 0; }
  input { box-sizing: border-box; width: 100%; min-width: 0; padding: 9px 29px 9px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font: inherit; font-size: 11px; text-overflow: ellipsis; }
  input:focus, input:focus-visible { outline: none; outline-offset: 0; border-color: var(--accent); }
  input:disabled { opacity: .55; cursor: not-allowed; }
  input::placeholder { color: var(--muted); }
  .chevron { position: absolute; right: 8px; top: 50%; display: flex; transform: translateY(-50%) rotate(90deg); color: var(--text); pointer-events: none; }
  .port-dropdown { position: absolute; top: 100%; left: 0; right: 0; z-index: 20; margin-top: 4px; border: 1px solid var(--line); border-radius: 5px; background: var(--surface); box-shadow: 0 8px 22px #0004; overflow: hidden; }
  .port-dropdown:popover-open{margin:0;padding:0;color:var(--text)}
  .port-dropdown.above{top:auto;bottom:100%;margin-top:0;margin-bottom:4px}
  .port-options { max-height: min(280px, 35vh); overflow-y: auto; overscroll-behavior: contain; }
  .port-options button { display: flex; flex-direction:column;justify-content:center;gap:3px; box-sizing: border-box; width: 100%;height:76px; min-width: 0; padding: 9px 10px; border: 0; border-radius: 0; border-bottom: 1px solid var(--line); color: var(--text); background: var(--surface); text-align: left; font: inherit; cursor: pointer; overflow:hidden; }
  .port-options button:last-child { border-bottom: 0; }
  .port-options button.active, .port-options button:hover { background: var(--surface-raised); box-shadow: inset 2px 0 var(--accent); }
  .port-options button[aria-selected='true'] .port-name { color: var(--accent); }
  .port-name { font-size: 11px; line-height: 1.4; }
  .port-description { color: var(--muted); font-size: 10px; line-height: 1.4; white-space:nowrap;max-width:100%;overflow:hidden;text-overflow:ellipsis; }
  .port-name{white-space:nowrap;max-width:100%;overflow:hidden;text-overflow:ellipsis}.port-reason{color:#d3a078;font-size:9px;line-height:1.3;white-space:nowrap;max-width:100%;overflow:hidden;text-overflow:ellipsis}.virtual-spacer{pointer-events:none}
  .port-count { margin: 0; padding: 8px 10px; border-top: 1px solid var(--line); color: var(--muted); font-size: 10px; line-height: 1.5; }
</style>
