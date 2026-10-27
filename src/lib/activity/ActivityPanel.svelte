<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import PageHeader from '../PageHeader.svelte';
  import Icon from '../Icon.svelte';
  import { gatewayRequest } from '../gateway';
  import ActivityFilters from './ActivityFilters.svelte';
  import { ActivityClient, ActivityLogController, activityFilterCount, activityLabel, activityQuery, emptyActivityFilters } from './model';
  const controller = new ActivityLogController(new ActivityClient(gatewayRequest));
  let view = $state.raw(controller.state), filters = $state(emptyActivityFilters());
  let search = $state(''), settledSearch = $state(''), offset = $state(0), throughId = $state<number | null>(null), mounted = $state(false), filtersOpen = $state(false), confirming = $state(false), dateError = $state('');
  const count = $derived(activityFilterCount(filters));
  const signature = $derived(JSON.stringify([filters, settledSearch]));
  const displayedOffset = $derived(view.query?.offset ?? 0);
  const end = $derived(displayedOffset + (view.page?.events.length ?? 0));
  function refresh() {
    try {
      const query = { ...activityQuery(filters, settledSearch, offset), throughId }; dateError = '';
      void controller.load(query).then(applied => { if (!applied && !controller.state.loading) { offset = controller.state.query?.offset ?? 0; throughId = controller.state.query?.throughId ?? null; } });
    }
    catch (error) { dateError = error instanceof Error ? error.message : String(error); }
  }
  $effect(() => { const value = search; const timer = setTimeout(() => settledSearch = value, 250); return () => clearTimeout(timer); });
  $effect(() => { signature; if (mounted) { offset = 0; throughId = null; untrack(refresh); } });
  onMount(() => {
    const stop = controller.subscribe(next => view = next); mounted = true;
    const tick = () => { if (!document.hidden && offset === 0 && !confirming && !view.loading) refresh(); };
    const timer = setInterval(tick, 3000); document.addEventListener('visibilitychange', tick);
    return () => { mounted = false; clearInterval(timer); document.removeEventListener('visibilitychange', tick); stop(); controller.dispose(); };
  });
  function page(next: number) { throughId = next === 0 ? null : throughId ?? view.page?.throughId ?? null; offset = next; refresh(); }
  async function clear() { if (await controller.clear()) { confirming = false; offset = 0; throughId = null; } }
  function show(dialog: HTMLDialogElement) { dialog.showModal(); return () => dialog.close(); }
  function date(value: number) { return new Date(value).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' }); }
</script>

<PageHeader title="Activity" eyebrow="THE RECORD OF YOUR RIG" subtitle="A searchable history of system changes, connections, and runtime events.">
  {#snippet actions()}<button class="secondary-button" disabled={view.clearing} onclick={() => confirming = true}><Icon name="trash" size={14}/>Clear all</button>{/snippet}
</PageHeader>
<div class="activity-toolbar">
  <label class="activity-search"><Icon name="search" size={16}/><input aria-label="Search activity" type="search" maxlength="512" placeholder="Search all activity…" bind:value={search}/></label>
  <button class="secondary-button" class:active={filtersOpen || count > 0} aria-label="Filter activity" aria-expanded={filtersOpen} aria-controls="activity-filter-section" onclick={() => filtersOpen = !filtersOpen}><Icon name="sliders" size={14}/>Filters{#if count}<span class="filter-count">{count}</span>{/if}</button>
  <button class="secondary-button" disabled={view.loading || view.clearing} onclick={refresh}><Icon name="refresh" size={14}/>Refresh</button>
</div>
<div id="activity-filter-section">{#if filtersOpen}<ActivityFilters bind:filters page={view.page}/>{/if}</div>
{#if dateError || view.error}<p class="activity-error" role="alert">{dateError || view.error}</p>{/if}
<section class="panel activity-log" aria-label="Activity log" aria-busy={view.loading}>
  <div class="log-heading"><h2>Recent activity</h2><span role="status">{view.loading ? 'Reading…' : `${view.page?.total ?? 0} events`}{#if displayedOffset > 0} · Live updates paused on older pages{/if}</span></div>
  <div class="log-columns" aria-hidden="true"><span>Time</span><span>Category</span><span>Activity</span><span>Source</span></div>
  {#each view.page?.events ?? [] as event (event.id)}
    <details class="log-event" class:warning={event.level === 'warning'} class:error={event.level === 'error'} data-event-id={event.id}>
      <summary><time datetime={new Date(event.occurredAt).toISOString()} title={new Date(event.occurredAt).toLocaleString()}>{date(event.occurredAt)}</time><span class="event-category">{activityLabel(event.category)}</span><span class="event-message"><span class="severity" title={activityLabel(event.level)} aria-label={activityLabel(event.level)}><Icon name={event.level === 'error' ? 'close' : event.level === 'warning' ? 'activity' : 'check'} size={12}/></span><strong>{event.message}</strong>{#if event.entityName}<span class="event-entity">{event.entityName}</span>{/if}</span><span class="event-source">{event.source}<Icon name="chevron" size={12}/></span></summary>
      <div class="event-detail"><p><strong>{event.message}</strong></p>{#if event.detail}<p>{event.detail}</p>{/if}<dl><div><dt>Type</dt><dd>{event.eventType}</dd></div><div><dt>Severity</dt><dd>{activityLabel(event.level)}</dd></div>{#if event.entityId}<div><dt>Entity</dt><dd>{event.entityName ?? ''} <code>{event.entityId}</code></dd></div>{/if}<div><dt>Recorded</dt><dd>{new Date(event.occurredAt).toISOString()}</dd></div><div><dt>Event</dt><dd>#{event.id}</dd></div></dl></div>
    </details>
  {:else}<div class="activity-empty"><Icon name="activity" size={26}/><strong>{view.loading ? 'Loading activity…' : search || count ? 'No matching activity' : 'No activity recorded yet'}</strong><p>{search || count ? 'Try another search or clear the filters.' : 'System changes will appear here automatically.'}</p>{#if search || count}<button class="text-button" onclick={() => { search = ''; settledSearch = ''; filters = emptyActivityFilters(); }}><Icon name="close" size={13}/>Clear filters</button>{/if}</div>{/each}
  {#if view.page && (displayedOffset > 0 || view.page.total > view.page.events.length)}<div class="activity-pagination"><span>{view.page.events.length ? displayedOffset + 1 : 0}–{end} of {view.page.total}</span><div><button class="secondary-button" disabled={!displayedOffset || view.loading || view.clearing} onclick={() => page(Math.max(0, displayedOffset - 100))}><Icon name="chevron" class="previous" size={13}/>Newer</button><button class="secondary-button" disabled={end >= view.page.total || view.loading || view.clearing} onclick={() => page(displayedOffset + 100)}>Older<Icon name="chevron" size={13}/></button></div></div>{/if}
</section>
{#if confirming}<dialog aria-labelledby="activity-clear-title" oncancel={event => { event.preventDefault(); if (!view.clearing) confirming = false; }} {@attach show}>
  <h2 id="activity-clear-title">Clear all activity?</h2><p>This permanently deletes every retained event, including events outside your current search and filters. New activity will continue to be recorded.</p>{#if view.error}<p class="activity-error" role="alert">{view.error}</p>{/if}<div class="dialog-actions"><button class="secondary-button" disabled={view.clearing} onclick={() => confirming = false}><Icon name="close" size={14}/>Cancel</button><button class="primary-button" disabled={view.clearing} onclick={clear}><Icon name="trash" size={14}/>{view.clearing ? 'Clearing…' : 'Clear all'}</button></div>
</dialog>{/if}

<style>
  .activity-toolbar{display:flex;gap:10px;align-items:center;margin-bottom:22px}.activity-search{display:flex;align-items:center;gap:10px;border:1px solid var(--line);border-radius:6px;background:var(--input-bg);padding:0 12px;flex:1;min-width:130px;color:var(--muted)}.activity-search:focus-within{outline:2px solid var(--accent);outline-offset:2px}.activity-search input{width:100%;min-width:0;border:0;outline:0;background:transparent;color:var(--text);padding:11px 0;font:inherit;font-size:11px}.active{border-color:var(--accent);background:var(--accent-bg)}.filter-count{border-radius:4px;padding:1px 5px;background:var(--accent-bg);font-size:10px}.log-heading{display:flex;align-items:center;justify-content:space-between;padding:15px 18px;border-bottom:1px solid var(--line);gap:12px}.log-heading h2{font-size:12px;margin:0}.log-heading>span{font-size:10px;color:var(--muted)}.log-columns,.log-event summary{display:grid;grid-template-columns:150px 100px minmax(0,1fr) 90px;gap:12px;align-items:center;padding:10px 18px}.log-columns{font-size:9px;color:var(--muted);text-transform:uppercase;letter-spacing:.07em;background:var(--input-bg)}.log-event{border-top:1px solid var(--line)}.log-event:first-of-type{border-top:0}.log-event summary{list-style:none;cursor:pointer;min-height:39px;box-sizing:border-box;font-size:11px}.log-event summary::-webkit-details-marker{display:none}.log-event summary:hover{background:var(--surface-raised)}.log-event summary:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}.log-event time{font-variant-numeric:tabular-nums;color:var(--muted);font-size:10px}.event-category{font-size:10px;color:var(--soft)}.event-message{display:flex;align-items:center;gap:8px;min-width:0}.event-message strong{font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.event-entity{color:var(--muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:10px}.severity{display:flex;flex-shrink:0;color:var(--muted)}.warning .severity{color:#d3a078}.error .severity{color:var(--danger,#e5a29b)}.event-source{display:flex;align-items:center;justify-content:space-between;font-size:9px;color:var(--muted)}.log-event[open] .event-source :global(svg){transform:rotate(90deg)}.event-detail{background:var(--input-bg);border-top:1px solid var(--line);padding:12px 18px 16px;overflow-wrap:anywhere}.event-detail p{font-size:11px;white-space:pre-wrap;line-height:1.65;margin:0 0 12px}.event-detail dl{display:flex;gap:12px 26px;flex-wrap:wrap;margin:0;font-size:10px}.event-detail dl>div{display:grid;gap:5px}.event-detail dt{color:var(--muted)}.event-detail dd{margin:0}.event-detail code{color:var(--muted)}.activity-empty{display:grid;justify-items:center;gap:12px;padding:46px 20px;color:var(--muted)}.activity-empty strong{font-size:12px;color:var(--text)}.activity-empty p{font-size:11px;margin:0;text-align:center}.activity-pagination{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:14px 18px;border-top:1px solid var(--line);font-size:10px;color:var(--muted)}.activity-pagination>div{display:flex;gap:8px}.activity-pagination :global(.previous){transform:rotate(180deg)}.activity-error{color:var(--danger,#e5b19b);font-size:12px;line-height:1.6}dialog{position:fixed;inset:0;margin:auto;width:min(480px,calc(100% - 32px));padding:24px;box-sizing:border-box;background:var(--surface);color:var(--text);border:1px solid var(--line);border-radius:10px;box-shadow:0 20px 80px #0005}dialog::backdrop{background:#0009}dialog h2{font-size:18px;margin:0}dialog p{color:var(--muted);font-size:12px;line-height:1.7;margin:16px 0}.dialog-actions{display:flex;justify-content:flex-end;gap:10px}@media(max-width:900px){.log-columns,.log-event summary{grid-template-columns:133px 75px minmax(0,1fr);gap:8px;padding-left:12px;padding-right:12px}.event-source,.log-columns>span:last-child{display:none}.event-entity{display:none}}@media(max-width:600px){.activity-toolbar{flex-wrap:wrap}.activity-search{flex-basis:100%}.log-columns{display:none}.log-event summary{grid-template-columns:1fr auto;gap:6px 10px}.event-message{grid-column:1/-1;grid-row:2}.event-category{text-align:right}.log-heading{padding:14px 12px;flex-wrap:wrap}}
</style>
