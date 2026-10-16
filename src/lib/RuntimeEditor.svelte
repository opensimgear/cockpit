<script lang="ts">
  import Notification from './Notification.svelte';
  import { mappingFields, mappingLimits, configurationError, configEqual, resizeMappings, type RuntimeConfig, type RuntimeSnapshot, type MappingKind } from './runtime-config';
  import type { GatewayCommand } from './gateway';
  let { config = $bindable(), snapshot, editing, busy, request, automatic = false, controllerEnabled = true }: { config:RuntimeConfig; snapshot:RuntimeSnapshot; editing:boolean; busy:boolean; request:(command:GatewayCommand,config?:RuntimeConfig)=>Promise<boolean>; automatic?:boolean; controllerEnabled?:boolean } = $props();
  const kinds:MappingKind[]=['buttons','axes','hats','encoders','selectors'];
  const singular:Record<MappingKind,string>={buttons:'Button',axes:'Axis',hats:'Hat',encoders:'Encoder',selectors:'Selector'};
  const error=$derived(configurationError(config));
  const dirty=$derived(!configEqual(config,snapshot.config)||(snapshot.view==='draft'&&snapshot.dirty));
  const unsaved=$derived(snapshot.generation!==snapshot.savedGeneration);
  function setExpansion(bank:number,enabled:boolean) { config.expanderMask=enabled ? config.expanderMask | (1<<bank) : config.expanderMask & ~(1<<bank); }
</script>

<section aria-label="Runtime module configuration">
  <div class="notice"><p>Map your physical inputs, calibrate axes and save settings on the module. Apply updates the module. {#if automatic}{#if controllerEnabled}If control counts, axis usages or neutral values change, end this setup session and use <strong>Update profile</strong> to publish the changed USB controls.{:else}This disabled device stays remembered. Its saved controls will be used when you explicitly enable its controller.{/if}{:else}Changes to control counts, axis usages or neutral values also need an explicit gateway <strong>Apply layout</strong> after this setup session ends.{/if}</p></div>
  <label class="field-label" for="runtime-name">Device name<span>Stored on module</span></label>
  <input id="runtime-name" class="text-input" bind:value={config.name} disabled={!editing||busy} maxlength="63" />
  <div class="module-counts">
    {#each kinds as kind (kind)}
      <label>{kind.charAt(0).toUpperCase()+kind.slice(1)}<input class="text-input" type="number" min="0" max={mappingLimits[kind]} value={config[kind].length} aria-label={`${kind} count`} disabled={!editing||busy} onchange={event=>resizeMappings(config,kind,Number(event.currentTarget.value))} /></label>
    {/each}
  </div>
  <details><summary>Expansion banks</summary><p class="field-help">Contacts 0–23 are on the module. Expansion bank 1 provides 24–39; each following bank adds sixteen contacts.</p>
    {#each Array.from({length:8},(_,i)=>i) as bank (bank)}<label class="field-label"><input type="checkbox" checked={!!(config.expanderMask&(1<<bank))} disabled={!editing||busy} onchange={event=>setExpansion(bank,event.currentTarget.checked)} /> Bank {bank+1} · {24+16*bank}–{39+16*bank}</label>{/each}
  </details>
  {#each kinds as kind (kind)}
    {#if config[kind].length}
      <details open={kind==='axes'}><summary>{kind.charAt(0).toUpperCase()+kind.slice(1)} · {config[kind].length}</summary>
        {#each config[kind] as row,index (index)}
          <fieldset disabled={!editing||busy}><legend>{singular[kind]} {index+1}</legend><div class="module-counts">
            {#each mappingFields[kind] as field,column (column)}
              {#if field.toggle}<label><input type="checkbox" checked={row[column]===1} aria-label={`${kind} ${index+1} ${field.label}`} onchange={event=>{ row[column]=event.currentTarget.checked?1:0; }} /> {field.label}</label>
              {:else}<label>{field.label}<input class="text-input" type="number" min={field.min} max={field.max} bind:value={row[column]} aria-label={`${kind} ${index+1} ${field.label}`} /></label>{/if}
            {/each}
          </div></fieldset>
        {/each}
        {#if kind==='axes'}<p class="field-help">Raw calibration uses 12-bit ADC samples (0–4095). Neutral and deadzone use the normalized 16-bit HID range. Usage 48–56 selects X, Y, Z, Rx, Ry, Rz, slider, dial or wheel.</p>{/if}
        {#if kind==='buttons'}<p class="field-help">Maintained switches report their current position. Momentary buttons rearm after a release. A nonzero exclusive group suppresses conflicting contacts.</p>{/if}
      </details>
    {/if}
  {/each}
  {#if error}<Notification message={String(error)} kind="error" />{/if}
  <div class="configuration-status"><span class="status-dot" class:draft-dot={dirty||unsaved}></span>{dirty?'Draft changes':unsaved?'Applied · not saved':'Saved on module'}<span>Generation {snapshot.generation}</span></div>
  <div class="save-actions">
    <button class="text-button" disabled={!editing||busy} onclick={()=>request('config_discard')}>Discard</button>
    <button class="secondary-button" disabled={!editing||busy||!!error||!dirty} onclick={()=>request('config_apply',config)}>Apply</button>
    <button class="primary-button" disabled={!editing||busy||!!error||(!dirty&&!unsaved)} onclick={()=>dirty?request('config_apply_save',config):request('config_save')}>Apply &amp; save</button>
  </div>
  <details><summary>Restore build defaults</summary><p class="field-help">Load the module's original mappings into a draft, then review and apply them.</p><button class="secondary-button" disabled={!editing||busy} onclick={()=>request('config_reset')}>Load defaults</button></details>
  <p class="field-help">{#if automatic}{#if controllerEnabled}The existing USB profile stays fixed until you explicitly update it. Changed counts, axis usages or neutral values keep its inputs neutral until then.{:else}Module settings remain editable while this device is online. Enable appends its controller to the current USB order.{/if}{:else}Gateway controller registrations keep their active profile until Apply layout reconnects USB. Changed counts, axis usages or neutral values stay neutral on the gateway until that layout is applied.{/if}</p>
  <details><summary>Generated HID descriptor</summary>{#if snapshot.layout}<p class="field-help">{snapshot.layout.buttons} buttons · {snapshot.layout.axes} axes · {snapshot.layout.hats} hats · {snapshot.layout.reportBytes} report bytes</p>{:else}<p class="field-help">Complete the input mappings before generating a valid descriptor.</p>{/if}<textarea class="text-input" aria-label="Generated HID descriptor" readonly rows="4" value={snapshot.descriptorHex}></textarea></details>
</section>

<style>
  details { margin: 14px 0; padding-top: 12px; border-top: 1px solid var(--line); }
  summary { color: var(--soft); font-size: 11px; font-weight: 600; cursor: pointer; }
  summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; }
  fieldset { min-width: 0; margin: 12px 0; padding: 10px; border: 1px solid var(--line); border-radius: 5px; background: var(--input-bg); }
  legend { padding: 0 6px; color: var(--soft); font-size: 10px; }
</style>
