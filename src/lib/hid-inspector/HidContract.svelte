<script lang="ts">
  import {descriptorTree,detailOf,exact,fieldsOf,object,rows} from './model';
  import type {HidRecord} from '../controller-hid';
  let {detail}:{detail:HidRecord}=$props();
  let search=$state('');
  const source=$derived(detailOf(detail));
  const fields=$derived(fieldsOf(detail));
  const tree=$derived(descriptorTree(detail,search));
  const capabilities=$derived(object(source.capabilities));
</script>
<section aria-label="Descriptor explorer">
  <h3>Descriptor and capabilities</h3>
  <dl><dt>Interface</dt><dd>{exact(detail.interfaceId??source.interfaceId)}</dd><dt>Contract</dt><dd>{exact(detail.contractFingerprint)}</dd><dt>Access</dt><dd>{exact(source.access)}</dd><dt>Provenance</dt><dd>{exact(source.provenance??source.backend)}</dd></dl>
  <div class="capabilities">{#each ['input','output','feature'] as kind}<article><strong>{kind}</strong><p>{fields.filter(field=>field.kind===kind).length} declared fields</p><pre>{JSON.stringify(rows(capabilities.reports).filter(report=>report.kind===kind),null,2)}</pre></article>{/each}</div>
  <label>Search descriptor<input aria-label="Search HID descriptor" type="search" bind:value={search} placeholder="Collection, usage, bounds, units, flags, report…"/></label>
  <div class="tree" role="tree" aria-label="Native descriptor tree">{#each tree as node (node.path)}<div role="treeitem" aria-level={node.depth+1} aria-selected="false" tabindex="0" style={`padding-left:${Math.min(node.depth,12)*12}px`}><code>{node.path}</code> <span>{node.value}</span></div>{/each}</div>
  <p class="muted">Showing up to 512 matching nodes. Search includes full paths and exact values. Collections, layouts, flags and unavailable evidence retain native names.</p>
</section>
<style>
  section{margin-top:20px}h3{font-size:var(--ui-heading-size);margin:0 0 12px}dl{display:grid;grid-template-columns:auto 1fr;gap:6px 12px;font-size:var(--ui-text-size)}dt{color:var(--muted)}dd{margin:0;overflow-wrap:anywhere}.capabilities{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:8px}.capabilities article{border:1px solid var(--border);border-radius:6px;padding:10px}.capabilities strong{text-transform:capitalize}p,label{font-size:var(--ui-text-size);color:var(--muted)}label{display:grid;gap:6px;margin:12px 0}input{width:100%;min-height:var(--ui-control-height)}pre{max-height:160px;overflow:auto;white-space:pre-wrap;font-size:var(--ui-text-size)}.tree{max-height:350px;overflow:auto;border:1px solid var(--border);padding:10px;line-height:var(--ui-line-height);font-size:var(--ui-text-size)}.tree div{overflow-wrap:anywhere}.tree span{color:var(--muted)}@media(max-width:700px){.capabilities{grid-template-columns:1fr}}
</style>
