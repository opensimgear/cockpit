<script lang="ts">
  import { untrack } from 'svelte';
  import CompiledWidget from './CompiledWidget.svelte';
  import { bundleForWidget, panelControlValue, panelFeedbackEvents, type CompiledPanelPublication } from '../../widget-runtime/src/panel.ts';
  import type { BehaviorFeedback, Scalar } from '../../widget-runtime/src/behavior.ts';
  import type { AudioAvailability } from '../../widget-runtime/src/player.ts';
  import type {PreparedCompiledSurfaces} from './compiled-surface.ts';
  let { publication, prepared, pageId, interactive = true, feedback = {}, suppressFeedbackCues = true, motion = true, muted = false, onoutput, onerror, onaudio }: {
    publication: CompiledPanelPublication; pageId?: string; interactive?: boolean;
    prepared?:PreparedCompiledSurfaces;
    feedback?: Record<string, BehaviorFeedback & { receivedAtMs?: number }>;
    suppressFeedbackCues?: boolean;
    motion?: boolean; muted?: boolean;
    onoutput?: (output: { controlId: string; value: boolean | number }) => void; onerror?: (message: string) => void;
    onaudio?: (availability: AudioAvailability) => void;
  } = $props();
  type Player = { dispatch: (event:string,updates?:Record<string,Scalar>,feedback?:Record<string,BehaviorFeedback>,suppressCues?:boolean)=>void; reset:()=>void;suspend:()=>void;enableAudio:()=>Promise<void> };
  let players: Record<string,Player> = {};
  let audioStates=$state.raw<Record<string,AudioAvailability>>({});
  const ready = new Set<string>(), initializedFeedback = new Set<string>(), delivered = new Map<string,string>();
  const activePage = $derived(pageId ?? publication.document.ui.pages[0].id);
  const viewport = $derived(publication.document.presentation.viewports.find(viewport=>viewport.pageId===activePage));
  const widgets = $derived(publication.document.presentation.widgets.filter(widget=>widget.pageId===activePage));
  const bundles = $derived(new Map(publication.document.presentation.widgets.map(widget=>[widget.id,bundleForWidget(publication,widget.id)])));
  function samples():Record<string,BehaviorFeedback> {
    const result:Record<string,BehaviorFeedback>={};
    for(const [channel,sample] of Object.entries(feedback))result[channel]={status:sample.status,...(sample.value!==undefined?{value:sample.value}:{}),...(sample.sourceAgeMs!==undefined?{sourceAgeMs:sample.sourceAgeMs}:{}),...(sample.provenance?{provenance:sample.provenance}:{}),...(sample.sourceKind?{sourceKind:sample.sourceKind}:{})};
    return result;
  }
  function applyFeedback(id:string){
    if(!ready.has(id)||!players[id])return;
    const events=panelFeedbackEvents(publication,id,samples());
    // Arrival identity is included even when value/quality are unchanged: a fresh
    // simulator packet renews freshness, while a render/tick never does.
    const widget=publication.document.presentation.widgets.find(widget=>widget.id===id)!;
    const channels=widget.outputBindings.map(binding=>publication.document.ui.feedbackWidgets.find(output=>output.id===binding.feedbackId)!.channelId);
    const arrivals=Object.fromEntries(channels.map(channel=>[channel,feedback[channel]?.receivedAtMs]));
    const signature=JSON.stringify({events,arrivals});
    if(delivered.get(id)===signature)return;delivered.set(id,signature);
    const suppressCues=suppressFeedbackCues||!initializedFeedback.has(id);
    for(const event of events){
      const adjusted=structuredClone(event.feedback),now=performance.now();
      for(const binding of widget.outputBindings){const port=bundles.get(id)!.artifact.outputs.find(port=>port.id===binding.portId)!,channel=publication.document.ui.feedbackWidgets.find(output=>output.id===binding.feedbackId)!.channelId,received=feedback[channel]?.receivedAtMs;
        if(adjusted[port.signal]&&received!==undefined)adjusted[port.signal].sourceAgeMs=Math.min(86400000,(adjusted[port.signal].sourceAgeMs??0)+Math.max(0,Math.floor(now-received)));
      }
      players[id].dispatch(event.event,undefined,adjusted,suppressCues);
    }
    if(channels.length&&channels.every(channel=>feedback[channel]))initializedFeedback.add(id);else initializedFeedback.delete(id);
  }
  function output(id:string,port:string,value:Scalar){if(!interactive)return;try{onoutput?.(panelControlValue(publication,id,port,value));}catch(error){onerror?.(error instanceof Error?error.message:String(error));}}
  export function reset():void {for(const player of Object.values(players))player?.reset();delivered.clear();initializedFeedback.clear();}
  export function suspend():void {for(const player of Object.values(players))player?.suspend();delivered.clear();initializedFeedback.clear();}
  export async function enableAudio():Promise<void>{await Promise.all(Object.values(players).map(player=>player?.enableAudio()));}
  $effect(()=>{const current=publication.publicationHash,page=activePage;untrack(()=>{void current;void page;ready.clear();delivered.clear();initializedFeedback.clear();for(const id of Object.keys(players))if(!widgets.some(widget=>widget.id===id))delete players[id];});});
  $effect(()=>{const current=feedback,suppressed=suppressFeedbackCues;untrack(()=>{void current;void suppressed;for(const widget of widgets)applyFeedback(widget.id);});});
  $effect(()=>{const states=widgets.filter(widget=>bundles.get(widget.id)!.artifact.sounds.length>0).map(widget=>audioStates[widget.id]??'locked');const order:AudioAvailability[]=['blocked','unsupported','muted','hidden','locked','available'];onaudio?.(order.find(status=>states.includes(status))??'unsupported');});
</script>

<div class="compiled-panel" aria-label={publication.document.ui.title} data-publication-hash={publication.publicationHash} style:aspect-ratio={viewport?`${viewport.width}/${viewport.height}`:undefined}>
  {#each widgets as widget (`${publication.publicationHash}:${widget.id}`)}
    <section class="compiled-panel-widget" data-widget-id={widget.id} style:left={`${widget.bounds.x*100}%`} style:top={`${widget.bounds.y*100}%`} style:width={`${widget.bounds.width*100}%`} style:height={`${widget.bounds.height*100}%`} aria-label={widget.label}>
      <CompiledWidget bind:this={players[widget.id]} bundle={bundles.get(widget.id)!} prepared={prepared?.[widget.id]} name={widget.label} {interactive} {motion} {muted} onready={()=>{ready.add(widget.id);applyFeedback(widget.id);}} onoutput={value=>output(widget.id,value.port,value.value)} onaudio={status=>{if(audioStates[widget.id]!==status)audioStates={...audioStates,[widget.id]:status};}} {onerror}/>
    </section>
  {/each}
</div>

<style>
  .compiled-panel{position:relative;width:100%;height:100%;min-width:0;min-height:0;background:#0d1820;overflow:hidden}.compiled-panel-widget{position:absolute;box-sizing:border-box;min-width:0;min-height:0;padding:0;margin:0;background:transparent}
</style>
