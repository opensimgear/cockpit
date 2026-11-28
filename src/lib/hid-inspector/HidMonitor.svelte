<script lang="ts">
  import type {HidRecord} from '../controller-hid';
  import {emptyFilters,exact,fieldsOf,matches,object,reportValues,rows,valuePresentation} from './model';
  let {detail,current,reports,evidence={},previous=[]}:{detail:HidRecord;current:HidRecord|null;reports:HidRecord[];evidence?:HidRecord;previous?:HidRecord[]}=$props();
  let filters=$state(emptyFilters());
  const fields=$derived(fieldsOf(detail));
  const values=$derived.by(()=>{
    const currentValues=rows(object(current?.result).values),events=reportValues(reports).filter(value=>String(value.semantics).toLowerCase()!=='absolute');
    return [...currentValues,...events.slice(-128)];
  });
  const changed=$derived.by(()=>{
    const before=new Map(previous.map(value=>[`${value.fieldId}:${value.elementIndex}`,JSON.stringify(value.sample)]));
    return new Set(values.filter(value=>before.get(`${value.fieldId}:${value.elementIndex}`)!==JSON.stringify(value.sample)).map(value=>`${value.fieldId}:${value.elementIndex}`));
  });
  const findField=(value:HidRecord)=>fields.find(field=>String(value.fieldId)===field.id||String(value.fieldId).startsWith(`${field.id}:`));
  const shown=$derived(values.filter(value=>matches(findField(value),value,filters,changed)));
</script>
<section aria-label="Decoded HID monitor">
  <h3>Received values</h3>
  <p role="status">Native state: {exact(object(current?.result).status)} · sequence {exact(object(current?.result).latest)} · acquired {exact(object(current?.result).receivedUs)} µs</p>
  {#if evidence.gap}<p role="alert">Display history has a gap. Earlier reports have left the native display buffer. A recording started before the loss retains its own bounded evidence.</p>{/if}
  {#if evidence.truncated}<p role="alert">Native report evidence was truncated: {exact(evidence.truncated)}. Export retained recording evidence and reduce the recording window.</p>{/if}
  {#if evidence.terminal||object(current?.result).terminal}<p role="status">Capture ended or disconnected: {exact(evidence.terminal??object(current?.result).terminal)}. Reconnect requires explicit selection and a new generation.</p>{/if}
  <div class="filters">
    <label>Kind<select aria-label="HID report kind" bind:value={filters.kind}><option value="">All</option><option>input</option><option>output</option><option>feature</option></select></label>
    <label>Report<input aria-label="HID report filter" bind:value={filters.report} placeholder="Exact ID"/></label>
    <label>Collection<input aria-label="HID collection filter" bind:value={filters.collection}/></label>
    <label>Usage / field<input aria-label="HID usage filter" bind:value={filters.usage}/></label>
    <label class="check"><input type="checkbox" bind:checked={filters.changed}/>Changed</label><label class="check"><input type="checkbox" bind:checked={filters.active}/>Active buttons</label>
  </div>
  <div class="values">{#each shown as value,index (`${value.fieldId}/${value.elementIndex}/${value.sequence}/${index}`)}
    {@const field=findField(value)}{@const presentation=valuePresentation(value,field)}
    <article data-quality={presentation.quality}>
      <div class="title"><strong>{presentation.kind}</strong><span>{presentation.quality}</span></div>
      <code>{exact(value.fieldId)} · element {exact(value.elementIndex)}</code>
      <output>{presentation.text}</output>
      {#if presentation.percent!==null}<meter min="0" max="100" value={presentation.percent} aria-label="Position within native logical bounds"></meter>{/if}
      {#if presentation.kind==='Button'&&presentation.quality==='valid'}<span class:pressed={presentation.active}>{presentation.active?'Pressed':'Released'}</span>{/if}
      <p>Logical {presentation.min} … {presentation.max}<br/>Report {exact(value.reportId??field?.reportId)} · collection {field?.collection??'unavailable'}<br/>Sequence {exact(value.sequence)} · {exact(value.receivedUs)} µs</p>
      {#if presentation.kind==='Relative delta'}<p>One received delta; never accumulated into a position.</p>{/if}
      {#if presentation.kind==='Hat'}<p>Native hat domain; null remains distinct from direction zero.</p>{/if}
      <details><summary>Exact sample and metadata</summary><pre>{JSON.stringify({value,capability:field?.metadata},null,2)}</pre></details>
    </article>
  {/each}</div>
  {#if !shown.length}<p>No matching observed values. Waiting and unavailable samples do not establish a zero position.</p>{/if}
  <h3>Raw deliveries and decode outcomes</h3>
  <p>Latest {reports.length} displayed deliveries (maximum 256). Native recording is independent of this display window. Raw context is filtered by report ID only; collection and usage filters apply to the decoded values above.</p>
  <div class="reports">{#each reports.filter(row=>!filters.report||exact(row.reportId??row.windowsReportId)===filters.report||row.kind==='gap'||row.kind==='terminal').reverse() as report,index (`${report.sequence}/${index}`)}<details><summary>{exact(report.kind??report.type??'report')} · #{exact(report.sequence)} · {exact(report.receivedUs)} µs · report {exact(report.reportId??report.windowsReportId)}</summary><div class="raw"><code>{exact(report.wireHex??object(report.bytes).value)}</code><pre>{JSON.stringify(report.samples??report,null,2)}</pre></div></details>{/each}</div>
</section>
<style>
  section{margin-top:20px}h3{font-size:var(--ui-heading-size);margin:16px 0 10px}p,label,summary,code{font-size:var(--ui-text-size);line-height:var(--ui-line-height)}p,label{color:var(--muted)}.filters{display:flex;flex-wrap:wrap;gap:10px;align-items:end}.filters label{display:grid;gap:6px;max-width:180px}.filters .check{display:flex;align-items:center;min-height:var(--ui-control-height)}input,select{min-height:var(--ui-control-height);max-width:100%}.values{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:10px;margin-top:14px}.values article{border:1px solid var(--border);border-radius:6px;padding:12px;min-width:0}.title{display:flex;justify-content:space-between;gap:8px}.title span{color:var(--muted)}code,output{display:block;overflow-wrap:anywhere}output{font-variant-numeric:tabular-nums;font-size:var(--ui-heading-size);margin:10px 0}meter{width:100%}pre{max-height:240px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;font-size:var(--ui-text-size)}[data-quality='null'],[data-quality='invalid'],[data-quality='unknown']{border-style:dashed}.pressed{color:var(--accent)}summary{cursor:pointer}.reports{max-height:450px;overflow:auto}.reports details{padding:10px;border-bottom:1px solid var(--border)}.raw{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,2fr);gap:12px}.raw code{white-space:pre-wrap;word-break:break-all}@media(max-width:700px){.raw{grid-template-columns:1fr}}
</style>
