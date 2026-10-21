<script lang="ts">
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';

  type PortOption = { value: string; label: string; description?: string; searchText?: string };
  let {
    label,
    placeholder = 'Search ports by name or ID',
    options,
    value = $bindable(''),
    query = $bindable(''),
    disabled = false,
    limit = 200,
  }: {
    label: string;
    placeholder?: string;
    options: readonly PortOption[];
    value?: string;
    query?: string;
    disabled?: boolean;
    limit?: number;
  } = $props();

  const uid = $props.id();
  let open = $state(false);
  let activeIndex = $state(-1);
  let listbox = $state<HTMLDivElement>();
  const selected = $derived(options.find(option => option.value === value));
  const pageLimit = $derived(Math.max(1, Math.min(200, Math.floor(limit) || 200)));
  const matches = $derived.by(() => {
    const search = query.trim().toLowerCase();
    return search ? options.filter(option => `${option.label} ${option.description ?? ''} ${option.searchText ?? ''}`.toLowerCase().includes(search)) : options;
  });
  const visibleOptions = $derived(matches.slice(0, pageLimit));
  const displayValue = $derived(open ? query : selected?.label ?? query);
  const activeId = $derived(open && activeIndex >= 0 && activeIndex < visibleOptions.length ? `${uid}-option-${activeIndex}` : undefined);

  $effect(() => {
    if (disabled && open) close();
    if (activeIndex >= visibleOptions.length) activeIndex = visibleOptions.length ? 0 : -1;
  });

  $effect(() => {
    if (!open || activeIndex < 0) return;
    const index = activeIndex;
    void tick().then(() => {
      if (open && activeIndex === index) listbox?.children.item(index)?.scrollIntoView({ block: 'nearest' });
    });
  });

  function show() {
    if (disabled || open) return;
    query = '';
    open = true;
    const selectedIndex = options.findIndex(option => option.value === value);
    activeIndex = options.length ? selectedIndex >= 0 && selectedIndex < pageLimit ? selectedIndex : 0 : -1;
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
  }

  function input(event: Event) {
    if (disabled) return;
    query = (event.currentTarget as HTMLInputElement).value;
    value = '';
    open = true;
    activeIndex = 0;
  }

  function keydown(event: KeyboardEvent) {
    if (disabled) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!open) {
        show();
        if (event.key === 'ArrowUp') activeIndex = visibleOptions.length - 1;
      } else if (visibleOptions.length) {
        activeIndex = (activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + visibleOptions.length) % visibleOptions.length;
      }
    } else if (event.key === 'Enter' && open) {
      event.preventDefault();
      if (visibleOptions[activeIndex]) select(visibleOptions[activeIndex]);
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

<div class="port-combobox" onfocusout={focusout}>
  <label id={`${uid}-label`} for={`${uid}-input`}>{label}</label>
  <div class="input-wrap">
    <input
      id={`${uid}-input`}
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
    <div class="port-dropdown">
      <div id={`${uid}-listbox`} class="port-options" role="listbox" aria-labelledby={`${uid}-label`} bind:this={listbox}>
        {#each visibleOptions as option, index (option.value)}
          <button
            id={`${uid}-option-${index}`}
            type="button"
            role="option"
            tabindex="-1"
            aria-selected={option.value === value}
            data-value={option.value}
            class:active={index === activeIndex}
            onpointerdown={event => event.preventDefault()}
            onpointermove={() => activeIndex = index}
            onclick={() => select(option)}
          >
            <span class="port-name">{option.label}</span>
            {#if option.description}<span class="port-description">{option.description}</span>{/if}
          </button>
        {/each}
      </div>
      <p id={`${uid}-count`} class="port-count" role="status">
        {#if !matches.length}No matching ports.
        {:else if matches.length > pageLimit}Showing {pageLimit} of {matches.length} results. Type to narrow the list.
        {:else}{matches.length} {matches.length === 1 ? 'result' : 'results'}{/if}
      </p>
    </div>
  {/if}
</div>

<style>
  .port-combobox { position: relative; display: grid; gap: 6px; min-width: 0; }
  label { color: var(--muted); font-size: 10px; }
  .input-wrap { position: relative; min-width: 0; }
  input { box-sizing: border-box; width: 100%; min-width: 0; padding: 9px 29px 9px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--input-bg); color: var(--text); font: inherit; font-size: 11px; text-overflow: ellipsis; }
  input:focus, input:focus-visible { outline: none; outline-offset: 0; border-color: var(--accent); }
  input:disabled { opacity: .55; cursor: not-allowed; }
  input::placeholder { color: var(--muted); }
  .chevron { position: absolute; right: 8px; top: 50%; display: flex; transform: translateY(-50%) rotate(90deg); color: var(--text); pointer-events: none; }
  .port-dropdown { position: absolute; top: 100%; left: 0; right: 0; z-index: 20; margin-top: 4px; border: 1px solid var(--line); border-radius: 5px; background: var(--surface); box-shadow: 0 8px 22px #0004; overflow: hidden; }
  .port-options { max-height: min(280px, 35vh); overflow-y: auto; overscroll-behavior: contain; }
  .port-options button { display: grid; gap: 3px; box-sizing: border-box; width: 100%; min-width: 0; padding: 9px 10px; border: 0; border-radius: 0; border-bottom: 1px solid var(--line); color: var(--text); background: var(--surface); text-align: left; font: inherit; cursor: pointer; overflow-wrap: anywhere; }
  .port-options button:last-child { border-bottom: 0; }
  .port-options button.active, .port-options button:hover { background: var(--surface-raised); box-shadow: inset 2px 0 var(--accent); }
  .port-options button[aria-selected='true'] .port-name { color: var(--accent); }
  .port-name { font-size: 11px; line-height: 1.4; }
  .port-description { color: var(--muted); font-size: 10px; line-height: 1.4; }
  .port-count { margin: 0; padding: 8px 10px; border-top: 1px solid var(--line); color: var(--muted); font-size: 10px; line-height: 1.5; }
</style>
