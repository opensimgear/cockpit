<script lang="ts">
  import PageHeader from './PageHeader.svelte';
  import Icon from './Icon.svelte';
  import GameIntegrationBadge from './GameIntegrationBadge.svelte';
  import {pickGameIntegrationFolder} from './gateway';
  import {gameExecutableDisplayPath,gameHasIntegration,type GamesState,type GameRequest,type GameDiscoveryAction} from './games';
  import {discoveryActions,discoverySelectAll,discoveryOtherInstallations,discoveryPathError,discoveryImage} from './game-discovery';
  let {gameState,busy,request,close}:{gameState:GamesState;busy:boolean;request:(request:GameRequest)=>Promise<GamesState|false>;close:()=>void}=$props();
  let location=$state(''),error=$state(''),scanId=$state(''),drafts=$state<Record<string,Partial<GameDiscoveryAction>>>({}),paths=$state<string[]>([]),openLibraries=$state<Record<string,boolean>>({});
  let failedImages=$state<Record<string,boolean>>({});
  let showLocations=$state(false);
  let showAllLibraries=$state<Record<string,boolean>>({});
  const discovery=$derived(gameState.discovery);
  const scanning=$derived(discovery?.status==='scanning');
  const preparing=$derived(scanning&&!!discovery?.libraries.length&&discovery.libraries.every(library=>library.status!=='queued'&&library.status!=='scanning'));
  const entries=$derived(discovery?.entries??[]);
  const actions=$derived(discoveryActions(entries,drafts));
  const selectedIds=$derived(new Set(actions.filter(action=>action.selected).map(action=>action.id)));
  $effect(()=>{if(discovery&&discovery.scanId!==scanId){scanId=discovery.scanId;drafts={};}});
  function positionArtwork(event:Event){const image=event.currentTarget as HTMLImageElement;image.style.objectPosition=image.naturalHeight>image.naturalWidth?'50% 35%':'50% 50%';}
  function change(id:string,value:Partial<GameDiscoveryAction>){drafts={...drafts,[id]:{...drafts[id],...value}};}
  async function start(extra?:string){
    error='';if(extra){const invalid=discoveryPathError(extra);if(invalid){error=invalid;return;}paths=[...new Set([...paths,extra.trim()])];}
    try{const result=await request({command:'game_discovery_start',...(paths.length?{paths}:{})});if(result&&!result.error)location='';}
    catch(cause){error=cause instanceof Error?cause.message:String(cause);}
  }
  async function browse(){try{const path=await pickGameIntegrationFolder('game',location);if(path){location=path;await start(path);}}catch(cause){error=cause instanceof Error?cause.message:String(cause);}}
  async function apply(){if(!discovery||busy||scanning)return;error='';try{const result=await request({command:'game_discovery_apply',scanId:discovery.scanId,actions});if(result&&!result.error)close();}catch(cause){error=cause instanceof Error?cause.message:String(cause);}}
</script>

<PageHeader title="Discover games" eyebrow="GAMES" subtitle="Review discovered installations before applying changes." onback={close} backLabel="All games">
  {#snippet actions()}<button class="secondary-button" aria-expanded={showLocations} aria-controls="game-discovery-locations" onclick={()=>showLocations=!showLocations}><Icon name="plus" size={14}/>Add location</button><button class="secondary-button" disabled={busy||scanning} onclick={()=>start()}><Icon name="refresh" size={14}/>Rescan</button>{/snippet}
</PageHeader>
<div class="discovery-workspace">
  {#if showLocations}
  <section id="game-discovery-locations" class="panel locations" aria-label="Added paths">
    <div class="sidebar-title"><Icon name="folder" size={18}/><h3>Added paths</h3></div>
    <p>Add a folder containing game folders. Only its immediate subfolders are checked. Existing installations and platform library trees are skipped.</p>
    <form onsubmit={event=>{event.preventDefault();void start(location);}}><input aria-label="Discovery folder" placeholder="C:\Games" bind:value={location} disabled={busy||scanning}/><button class="secondary-button" type="button" disabled={busy||scanning} onclick={browse}><Icon name="folder" size={14}/>Browse</button><button class="primary-button" disabled={busy||scanning||!location.trim()}><Icon name="plus" size={14}/>Add</button></form>
  </section>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if scanning}<p class="scan-status" role="status">{preparing?(entries.length?'Reading platform metadata and preparing artwork… Discovered games are shown below.':'Matching discovered installations…'):'Scanning libraries…'}</p>{/if}
  <div class="libraries" aria-label="Library scan progress">
    {#each discovery?.libraries??[] as library (library.id)}
      {@const libraryEntries=entries.filter(entry=>[library.id.toLowerCase(),library.name.toLowerCase()].includes(entry.library.toLowerCase()))}
      {@const selectedCount=libraryEntries.filter(entry=>selectedIds.has(entry.id)).length}
      {@const otherInstallations=showAllLibraries[library.id]?discoveryOtherInstallations(gameState.catalogue,entries,discovery?.observations??[],library.id):[]}
      {#if scanning||libraryEntries.length||library.found||library.message}
      <details class="panel library" data-library={library.id} open={openLibraries[library.id]??(libraryEntries.length>0||library.status==='scanning')} ontoggle={event=>{openLibraries={...openLibraries,[library.id]:event.currentTarget.open};}}>
        <summary class="library-heading"><span class="platform-icon" data-platform={library.id}><Icon name={library.id==='local'?'disk':library.id} size={23}/></span><span class="library-name">{library.name}<small>{library.found} installations found · {libraryEntries.length} changes</small></span><span class="library-status" class:scanning={library.status==='scanning'||library.status==='preparing'}><Icon name={library.status==='complete'?'check':library.status==='scanning'||library.status==='preparing'?'refresh':library.status==='failed'?'alert':'info'} size={14}/>{library.status}</span><span class="library-chevron"><Icon name="chevron" size={15}/></span></summary>
        <div class="library-content">
          {#if library.status==='scanning'||library.status==='preparing'}<progress aria-label={library.name+' scan progress'}></progress>{/if}
          {#if library.message}<p class="library-message">{library.message}</p>{/if}
          {#if libraryEntries.length||discovery?.status==='ready'&&library.found>0}
          <div class="library-selection"><label class="entry-choice"><input type="checkbox" role="switch" aria-label={'Show all games in '+library.name} checked={showAllLibraries[library.id]??false} onchange={event=>{showAllLibraries={...showAllLibraries,[library.id]:event.currentTarget.checked};}}/><span>Show all</span></label><label class="entry-choice"><span>Select all</span><input type="checkbox" aria-label={'Select all games in '+library.name} checked={libraryEntries.length>0&&selectedCount===libraryEntries.length} indeterminate={selectedCount>0&&selectedCount<libraryEntries.length} disabled={busy||scanning||discovery?.status!=='ready'||!libraryEntries.length} onchange={event=>{drafts=discoverySelectAll(libraryEntries,drafts,event.currentTarget.checked);}}/></label></div>
          {/if}
          {#each [{title:'New and changed installations',items:libraryEntries.filter(entry=>entry.action!=='remove')},{title:'Removed installations',items:libraryEntries.filter(entry=>entry.action==='remove')}].filter(group=>group.items.length) as group}
            <section class="results" aria-label={library.name+' '+group.title}><h3>{group.title}</h3>
              {#each group.items as entry (entry.id)}
                {@const selected=selectedIds.has(entry.id)}
                {@const targetId=drafts[entry.id]?.gameId??entry.gameId}
                {@const target=gameState.catalogue.find(game=>game.id===targetId)}
                {@const image=discoveryImage(entry,target)}
                {@const actionLabel=entry.action==='add'?'Add':entry.action==='update'?'Update':'Remove'}
                <article class="discovery-entry" data-entry-id={entry.id}>
                  <div class="entry-art" aria-hidden="true">{#if image&&!failedImages[image]}<img src={image} alt="" loading="lazy" decoding="async" onload={positionArtwork} onerror={()=>{failedImages={...failedImages,[image]:true};}}/>{:else}<Icon name="gamepad" size={38}/>{/if}</div>
                  <div class="entry-details"><div class="entry-heading"><strong>{entry.name}</strong><GameIntegrationBadge available={!!entry.integration||gameHasIntegration(target)}/></div>
                    {#if entry.message}<p class="entry-message">{entry.message}</p>{/if}
                    {#if entry.action!=='remove'}
                    <div class="entry-controls"><label class="executable-choice"><span class="sr-only">Executable</span><select aria-label={entry.name+' executable'} disabled={busy||scanning||!selected} value={drafts[entry.id]?.executable??entry.executable??entry.executableCandidates[0]??''} onchange={event=>change(entry.id,{executable:event.currentTarget.value||null})}>{#if !entry.executableCandidates.length}<option value="">No executable found</option>{/if}{#each entry.executableCandidates as executable}<option value={executable}>{gameExecutableDisplayPath(executable,entry.installPath)}</option>{/each}</select></label>
                    </div>
                    {/if}
                  </div>
                  <label class="entry-choice"><span>{actionLabel}</span><input type="checkbox" aria-label={actionLabel+' '+entry.name} checked={selected} disabled={busy||scanning||discovery?.status!=='ready'} onchange={event=>change(entry.id,{selected:event.currentTarget.checked})}/></label>
                </article>
              {/each}
            </section>
          {/each}
          {#if showAllLibraries[library.id]&&otherInstallations.length}
            <section class="results" aria-label={library.name+' Other detected installations'}><h3>Other detected installations</h3>
              {#each otherInstallations as installation (installation.id)}
                <article class="discovery-entry detected-entry" data-detected-id={installation.id}>
                  <div class="entry-art" aria-hidden="true">{#if installation.image&&!failedImages[installation.image]}<img src={installation.image} alt="" loading="lazy" decoding="async" onload={positionArtwork} onerror={()=>{failedImages={...failedImages,[installation.image]:true};}}/>{:else}<Icon name="gamepad" size={38}/>{/if}</div>
                  <div class="entry-details"><div class="entry-heading"><strong>{installation.name}</strong><GameIntegrationBadge available={installation.integrationAvailable}/></div><div class="entry-controls"><label class="executable-choice"><span class="sr-only">Executable</span><select aria-label={installation.name+' detected executable'} disabled value={installation.executable??''}>{#if !installation.executableCandidates.length}<option value="">No executable found</option>{/if}{#each installation.executableCandidates as executable}<option value={executable}>{gameExecutableDisplayPath(executable,installation.installPath)}</option>{/each}</select></label></div></div>
                  <span class="entry-status">{installation.status}</span>
                </article>
              {/each}
            </section>
          {/if}
        </div>
      </details>
      {/if}
    {/each}
  </div>
  {#if discovery?.diagnostics.length}<section class="panel diagnostics"><h3>Scan details</h3>{#each discovery.diagnostics as message}<p>{message}</p>{/each}</section>{/if}
  <footer><p>{actions.filter(action=>action.selected).length} selected changes{#if discovery?.status==='failed'} · Scan failed. Review scan details and retry.{/if}</p><button class="primary-button" disabled={busy||scanning||discovery?.status!=='ready'} onclick={apply}><Icon name="check" size={14}/>Apply</button></footer>
</div>
<style>
  .library-selection{display:flex;align-items:center;justify-content:space-between;gap:16px;padding-top:14px}.entry-status{font-size:var(--ui-text-size);color:var(--muted);text-align:right;max-width:144px;justify-self:end}
  .discovery-workspace{display:grid;gap:16px}.locations,.diagnostics{padding:18px}h3{margin:0;font-size:var(--ui-heading-size)}p,small{color:var(--muted);font-size:var(--ui-text-size);line-height:var(--ui-line-height)}form{display:flex;gap:8px;flex-wrap:wrap}form input{flex:1;min-width:240px}input:not([type=checkbox]),select{padding:8px 10px;border:1px solid var(--line);border-radius:5px;background:var(--bg);color:var(--text);min-width:0}select{max-width:100%;font-size:var(--ui-text-size)}.libraries{display:grid;gap:10px}.library{overflow:hidden}.library-heading{display:flex;align-items:center;gap:12px;padding:15px 18px;cursor:pointer;list-style:none}.library-heading::-webkit-details-marker{display:none}.platform-icon{display:flex;align-items:center;justify-content:center;width:36px;height:36px;border:1px solid var(--line);border-radius:8px;background:var(--surface-raised);color:var(--soft);flex-shrink:0}.library-name{display:grid;gap:3px;flex:1;font-weight:600;font-size:var(--ui-heading-size)}.library-name small{font-weight:400;font-size:var(--ui-text-size)}.library-status{display:flex;align-items:center;gap:6px;color:var(--muted);font-size:var(--ui-text-size);text-transform:capitalize}.library-status.scanning{color:var(--accent)}.library-status.scanning :global(svg){animation:scan-spin 1.2s linear infinite}.library-chevron{display:flex;color:var(--muted)}.library[open]>.library-heading .library-chevron{transform:rotate(90deg)}.library-content{padding:0 18px 14px;border-top:1px solid var(--line)}progress{width:100%;margin-top:12px}.results{margin-top:16px}.results h3{font-size:var(--ui-text-size);font-weight:600;color:var(--muted)}.discovery-entry{display:grid;grid-template-columns:216px minmax(0,1fr) 144px;align-items:center;gap:14px;padding:12px 0;border-bottom:1px solid var(--line)}.discovery-entry:last-child{border-bottom:0;padding-bottom:0}.entry-art{position:relative;width:100%;height:100%;min-height:120px;max-height:168px;align-self:center;display:flex;align-items:center;justify-content:center;background:var(--surface-raised);border:1px solid var(--line);border-radius:5px;overflow:hidden;color:var(--muted)}.entry-art img{position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:center;display:block}.entry-details{min-width:0;align-self:stretch}.entry-heading{display:flex;align-items:center}.entry-heading>strong{min-width:0;font-size:var(--ui-heading-size);font-weight:600;letter-spacing:-.2px;line-height:1.45}.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}.entry-choice{display:flex;align-items:center;gap:8px;font-size:var(--ui-text-size);color:var(--soft);cursor:pointer;flex-shrink:0}.discovery-entry>.entry-choice{justify-content:flex-end;min-width:0}.entry-choice input{accent-color:var(--accent);width:15px;height:15px;margin:0}.entry-controls{display:grid;gap:8px;margin-top:6px}.entry-controls label{display:grid;gap:4px;font-size:var(--ui-text-size);color:var(--muted)}.executable-choice select{font-family:ui-monospace,monospace;font-size:var(--ui-text-size)}.entry-message,.library-message{margin:8px 0 0;font-size:var(--ui-text-size)}.error{color:var(--red)}.scan-status{margin:0}footer{display:flex;align-items:center;justify-content:space-between;gap:16px;position:sticky;bottom:0;padding:12px 18px;background:var(--surface);border:1px solid var(--line);border-radius:6px}footer p{margin:0}@keyframes scan-spin{to{transform:rotate(360deg)}}@media(prefers-reduced-motion:reduce){.library-status.scanning :global(svg){animation:none}}@media(max-width:600px){.library-heading{padding:12px;gap:9px}.library-content{padding:0 12px 12px}.library-status{font-size:var(--ui-text-size)}.platform-icon{width:30px;height:30px}.library-name{font-size:var(--ui-text-size)}.library-name small{font-size:var(--ui-text-size)}.discovery-entry{grid-template-columns:108px minmax(0,1fr) 96px;gap:10px}.entry-art{min-height:96px;max-height:144px}.entry-choice{font-size:var(--ui-text-size)}}
</style>
