<script lang="ts">
  import ProfilePortCombobox from '../ProfilePortCombobox.svelte';
  import {graphProviderPortMatches,type ProfileGraphSelection} from '../profile-graph';
  import type {DeepReadonly,ProfilePort} from '../profiles';
  import type {ObservedResourcePort} from '../profile-inventory';

  let {ports,options,busy=false,readOnly=false,assign}:{
    ports:readonly DeepReadonly<ProfilePort>[];options:readonly DeepReadonly<ObservedResourcePort>[];
    busy?:boolean;readOnly?:boolean;assign:(selections:ProfileGraphSelection[])=>boolean;
  }=$props();
  let choices=$state<Record<string,string>>({});
  const key=(port:DeepReadonly<ObservedResourcePort>)=>JSON.stringify([port.descriptor.id,port.contractHash]);
  function matches(port:DeepReadonly<ProfilePort>){return options.filter(observed=>observed.current&&graphProviderPortMatches(port,observed));}
  function choose(portId:string,selected:string){
    if(busy||readOnly)return;
    const port=ports.find(port=>port.id===portId);
    if(!port||!matches(port).some(observed=>key(observed)===selected))return;
    choices={...choices,[portId]:selected};
    const selections=ports.flatMap(port=>{const observed=matches(port).find(observed=>key(observed)===choices[port.id]);return observed?[{profilePortId:port.id,observed}]:[];});
    if(selections.length===ports.length&&selections.length)assign(selections);
  }
</script>

<section class="integration-assignment" aria-label="Match saved signals">
  <h4>Match saved signals</h4>
  <p>Choose the native signal for each saved port to preserve its routes.</p>
  {#each ports as port (port.id)}
    <ProfilePortCombobox label={port.name} placeholder="Choose a matching signal" value={choices[port.id]??''} options={matches(port).map(observed=>({value:key(observed),label:observed.descriptor.name,description:observed.descriptor.id}))} disabled={busy||readOnly} compact onselect={value=>choose(port.id,value)}/>
  {/each}
</section>

<style>
  .integration-assignment{display:grid;gap:8px}
  h4{margin:0;font-size:var(--ui-heading-size);font-weight:570;color:var(--soft)}
  p{margin:0;font-size:var(--ui-text-size);color:var(--muted);line-height:var(--ui-line-height)}
</style>
