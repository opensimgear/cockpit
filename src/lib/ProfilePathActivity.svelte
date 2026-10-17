<script lang="ts">
  import {onMount,untrack} from 'svelte';
  import type {DeepReadonly,ProfileEndpoint} from './profiles';
  import type {NativeProfileOwner} from './profile-runtime';
  import type {ProfileTopologyNode} from './profile-activity';
  import {profileOwnersEqual,type ProfileInspectedPort} from './profile-inspection';
  import {ProfileActivityController,profileActivityPort} from './profile-activity-controller';
  let {controller,owner=null,active=true,selectedId='',editorProfileId}: {controller:ProfileActivityController;owner?:DeepReadonly<NativeProfileOwner>|null;active?:boolean;selectedId?:string;editorProfileId?:string}=$props();
  const activity=untrack(()=>controller);
  let view=$state.raw(activity.state),mounted=$state(false),followSelection=$state(true),query=$state('');
  const ownerKey=$derived(owner?JSON.stringify([owner.session,owner.generation,owner.profileId,owner.profileRevision,owner.gameId]):'');
  const filter=$derived(followSelection&&owner?.profileId===editorProfileId?selectedId:'');
  const matching=$derived(!!owner&&!!view.owner&&profileOwnersEqual(owner,view.owner));
  const topology=$derived(matching?view.topology:null),path=$derived(matching?view.path:null);
  const connections=$derived((topology?.edges??[]).filter(edge=>`${edge.id} ${edge.from.nodeId} ${edge.from.portId} ${edge.to.nodeId} ${edge.to.portId}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())));
  const sources=$derived(path?.nodes.filter(node=>node.kind!=='processing'&&path.edges.some(edge=>edge.from.nodeId===node.id))??[]);
  const processors=$derived.by(()=>{
    const remaining=path?.nodes.filter(node=>node.kind==='processing')??[],ordered:DeepReadonly<ProfileTopologyNode>[]=[];
    while(remaining.length){const index=remaining.findIndex(node=>!path?.edges.some(edge=>edge.to.nodeId===node.id&&remaining.some(parent=>parent.id===edge.from.nodeId)));if(index<0)return [...ordered,...remaining];ordered.push(...remaining.splice(index,1));}
    return ordered;
  });
  const destinations=$derived(path?.nodes.filter(node=>node.kind!=='processing'&&path.edges.some(edge=>edge.to.nodeId===node.id))??[]);
  onMount(()=>{mounted=true;const stop=activity.subscribe(next=>view=next);return()=>{mounted=false;stop();};});
  function watchActivity(){
    const key=ownerKey;if(!mounted||!active||!key){activity.clear();return;}
    const captured=untrack(()=>owner)!,selection=untrack(()=>filter);
    const refresh=()=>{if(!document.hidden){if(!activity.state.topology)void activity.open(captured,selection);else void activity.refresh();}};
    refresh();const timer=window.setInterval(refresh,2000);document.addEventListener('visibilitychange',refresh);
    return()=>{window.clearInterval(timer);document.removeEventListener('visibilitychange',refresh);activity.clear();};
  }
  $effect(watchActivity);
  function refreshActivity(){if(!owner)return;if(!activity.state.topology)void activity.open(owner,filter);else void activity.refresh();}
  function syncSelection(){const selected=followSelection&&owner?.profileId===editorProfileId?selectedId:'';if(followSelection&&mounted&&active&&matching&&view.topology&&view.selectedNodeId!==selected)void activity.selectBlock(selected);}
  $effect(syncSelection);
  function chooseConnection(id:string){followSelection=false;void activity.selectEdge(id);}
  function setFollowSelection(next:boolean){followSelection=next;if(next)void activity.selectBlock(owner?.profileId===editorProfileId?selectedId:'');}
  function ports(node:DeepReadonly<ProfileTopologyNode>,role:'source'|'processing'|'destination'):DeepReadonly<ProfileEndpoint>[] {
    const endpoints=(path?.edges??[]).flatMap(edge=>[...(edge.from.nodeId===node.id&&role!=='destination'?[edge.from]:[]),...(edge.to.nodeId===node.id&&role!=='source'?[edge.to]:[])]);
    return [...new Map(endpoints.map(endpoint=>[JSON.stringify([endpoint.nodeId,endpoint.portId]),endpoint])).values()];
  }
  function sample(endpoint:DeepReadonly<ProfileEndpoint>):DeepReadonly<ProfileInspectedPort>|undefined{return matching?profileActivityPort(view,endpoint):undefined;}
  function value(port:DeepReadonly<ProfileInspectedPort>):string{
    if(port.sample.status!=='known')return port.sample.status;
    if(port.sample.value.kind==='action-pack')return port.sample.value.value.arguments.map(argument=>`${argument.id} = ${JSON.stringify(argument.value)}`).join(', ')||'No arguments';
    return JSON.stringify(port.sample.value.value);
  }
  function capture(nodeId:string):string{const current=view.captures.find(capture=>capture.nodeId===nodeId);return current?current.capturedFromUs===current.capturedThroughUs?`${current.capturedFromUs} μs`:`${current.capturedFromUs}–${current.capturedThroughUs} μs`:'Not captured';}
  function connection(id:string):string{const edge=topology?.edges.find(edge=>edge.id===id);return edge?`${edge.from.nodeId} / ${edge.from.portId} → ${edge.to.nodeId} / ${edge.to.portId}`:id;}
</script>

{#snippet block(node:DeepReadonly<ProfileTopologyNode>,role:'source'|'processing'|'destination')}
  <article class="activity-block" class:old={view.stale} aria-label={`${role} ${node.name}`}>
    <div class="activity-role">{role==='processing'?'Transformation':role==='source'?'Source':'Destination'} · {node.kind}</div>
    <strong>{node.name}</strong><code class="activity-id">{node.id}</code><p>{node.summary}</p>
    <ul>{#each ports(node,role) as endpoint (endpoint.portId)}{@const observed=sample(endpoint)}<li><code>{endpoint.portId}</code>{#if observed}<span class="sample-status" class:known={observed.sample.status==='known'}>{observed.sample.status==='known'?'Known sample':observed.sample.status}</span><span class="sample-value">{value(observed)}</span><small>{observed.direction==='input'?'Upstream/requested':'Graph producer'} · {observed.contract.delivery}{observed.contract.delivery==='event'?' · last event sample':''} · {observed.observedUs===null?'never observed':`${observed.observedUs} μs`}</small>{:else}<span class="sample-status">No declared sample captured</span>{/if}</li>{/each}</ul>
    <small class="capture-time">{view.stale?'Stale capture':'Native capture'} · {capture(node.id)}</small>
  </article>
{/snippet}

<section class="profile-path panel" aria-label="Applied source to destination activity">
  <div class="path-heading"><div><strong>Applied path activity</strong><p>Follow the running graph from exact source ports through visible transformations to requested destinations.</p></div>{#if owner}<button class="text-button" disabled={view.loading} onclick={refreshActivity}>{view.loading?'Reading path…':'Refresh path'}</button>{/if}</div>
  {#if owner}
    <p class="path-help">Applied revision {owner.profileRevision} · run {owner.generation}. Each block is captured separately. Known samples include neutral values such as false and zero; they are not delivery acknowledgements or evidence that a physical output changed.</p>
    {#if selectedId&&editorProfileId===owner.profileId}<label class="path-follow"><input type="checkbox" checked={followSelection} onchange={event=>setFollowSelection(event.currentTarget.checked)}/> Follow selected applied block <code>{selectedId}</code></label>{/if}
    {#if topology}
      <div class="path-tools"><label>Find applied connection<input type="search" maxlength="100" bind:value={query} placeholder="Exact block or port ID"/></label><label>Applied connection<select value={view.edgeId} onchange={event=>chooseConnection(event.currentTarget.value)}><option value="" disabled>Choose an applied connection</option>{#each connections as edge (edge.id)}<option value={edge.id}>{connection(edge.id)} · {edge.id}</option>{/each}</select></label><label class="path-expand"><input type="checkbox" checked={view.expanded} onchange={event=>activity.selectEdge(view.edgeId,event.currentTarget.checked)}/> Include connected transformation branches</label></div>
      {#if query&&!connections.length}<p class="path-help" role="status">No applied connection matches this search.</p>{/if}
      {#if path&&!path.error&&path.edges.length}
        <div class="path-stages" aria-label="Observed source processing and requested destination stages">
          <section aria-label="Source blocks"><h4>Sources</h4>{#each sources as node (node.id)}{@render block(node,'source')}{:else}<p class="path-help">No external source on this selected route.</p>{/each}</section>
          <span class="stage-arrow" aria-hidden="true">→</span>
          <section aria-label="Processing blocks"><h4>Transformations</h4>{#each processors as node (node.id)}{@render block(node,'processing')}{:else}<p class="path-help">Direct exact port connection.</p>{/each}</section>
          <span class="stage-arrow" aria-hidden="true">→</span>
          <section aria-label="Destination blocks"><h4>Destinations</h4>{#each destinations as node (node.id)}{@render block(node,'destination')}{:else}<p class="path-help">No external destination on this selected route.</p>{/each}</section>
        </div>
        <details class="exact-connections" open><summary>{path.edges.length} exact applied {path.edges.length===1?'connection':'connections'}</summary><p class="path-help">Highlighted connections have a known producer sample, including neutral values. The highlight does not confirm delivery to the destination.</p><ul aria-label="Exact applied connection sample states">{#each path.edges as edge (edge.id)}{@const produced=sample(edge.from)}{@const requested=sample(edge.to)}<li class:sample-known={!view.stale&&produced?.sample.status==='known'} class:sample-old={view.stale}><code>{edge.id}</code><span class="connection-endpoint"><code>{edge.from.nodeId} / {edge.from.portId}</code><small>Producer · {produced?value(produced):'not captured'}</small></span><span class="connection-arrow" aria-hidden="true">→</span><span class="connection-endpoint"><code>{edge.to.nodeId} / {edge.to.portId}</code><small>Requested · {requested?value(requested):'not captured'}</small></span></li>{/each}</ul></details>
      {:else if path&&!path.error&&path.nodes.length}<p class="path-help">Applied block <strong>{path.nodes[0].name}</strong> has no connected route. Its {view.captures[0]?.ports.length??0} declared ports remain available in the sample table.</p>{/if}
      <p class="path-capture" role="status">{view.stale?'Path samples are stale or waiting for a capture.':'Path samples match this exact applied owner.'} Topology captured {topology.capturedFromUs}–{topology.capturedThroughUs} μs.</p>
    {:else}<p class="path-help" role="status">{view.loading?'Loading the immutable applied graph…':'No applied topology loaded.'}</p>{/if}
    {#if matching&&view.error}<p class="path-error" role="status">{view.error}</p>{/if}
  {:else}<p class="path-help">No current applied owner is available. This view never starts a profile or changes a draft.</p>{/if}
</section>

<style>
  .profile-path{padding:17px;margin-bottom:22px;}.path-heading{display:flex;align-items:flex-start;justify-content:space-between;gap:14px;}.path-heading p,.path-help,.path-capture{color:var(--muted);font-size:12px;line-height:1.55;}.path-heading p{margin:5px 0;}.path-follow,.path-expand{display:flex;gap:7px;align-items:center;font-size:12px;}.path-tools{display:flex;align-items:end;flex-wrap:wrap;gap:12px;margin:15px 0;}.path-tools>label:not(.path-expand){display:flex;flex-direction:column;gap:5px;font-size:11px;min-width:160px;}.path-tools>label:nth-child(2){flex:1;min-width:220px;}.path-tools input[type=search],.path-tools select{width:100%;max-width:100%;box-sizing:border-box;border:1px solid var(--line);background:var(--surface);color:var(--text);border-radius:6px;padding:8px;font-size:12px;}.path-stages{display:grid;grid-template-columns:minmax(0,1fr) auto minmax(0,1fr) auto minmax(0,1fr);gap:12px;margin-top:18px;}.path-stages h4{margin:0 0 10px;font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--muted);}.stage-arrow{align-self:center;color:var(--muted);}.activity-block{border:1px solid var(--line);border-radius:8px;background:var(--surface);padding:12px;margin-bottom:10px;min-width:0;overflow-wrap:anywhere;}.activity-block.old{opacity:.65;}.activity-role{font-size:10px;color:var(--muted);text-transform:uppercase;margin-bottom:7px;}.activity-block>strong{display:block;font-size:12px;}.activity-id{display:block;color:var(--muted);font-size:10px;margin-top:4px;}.activity-block p{font-size:11px;line-height:1.5;margin:9px 0;color:var(--muted);}.activity-block ul,.exact-connections ul{list-style:none;padding:0;margin:0;}.activity-block li{border-top:1px solid var(--line);padding-top:9px;margin-top:9px;display:flex;flex-wrap:wrap;gap:5px;align-items:center;font-size:11px;}.sample-status{font-size:10px;border-radius:4px;padding:2px 5px;color:var(--muted);background:var(--surface-raised);}.sample-status.known{color:var(--text);border:1px solid var(--line);}.sample-value{display:block;width:100%;font-family:var(--font-mono,monospace);white-space:pre-wrap;}.activity-block small{display:block;width:100%;font-size:10px;color:var(--muted);line-height:1.5;}.capture-time{border-top:1px solid var(--line);margin-top:10px;padding-top:8px;}.path-error{color:#d3a078;font-size:12px;}.exact-connections{font-size:11px;}.exact-connections summary{cursor:pointer;color:var(--muted);}.exact-connections li{display:flex;align-items:center;flex-wrap:wrap;gap:12px;margin-top:8px;border-left:2px solid var(--line);padding:9px;}.exact-connections li.sample-known{border-left-color:var(--accent);background:var(--accent-bg);}.exact-connections li.sample-old{opacity:.65;}.exact-connections code{color:var(--muted);}.connection-endpoint{display:grid;gap:5px;min-width:0;flex:1;overflow-wrap:anywhere;}.connection-endpoint small{color:var(--soft);}.connection-arrow{color:var(--muted);}.sample-known .connection-arrow{color:var(--accent);}@media(max-width:850px){.path-stages{grid-template-columns:1fr;}.stage-arrow{justify-self:center;transform:rotate(90deg);}.path-heading{flex-wrap:wrap;}}
</style>
