import assert from 'node:assert/strict';
import {setTimeout as delay} from 'node:timers/promises';
import {until} from '../../scripts/reactive/cdp.mjs';
import {alignClock,summarize} from './dcs-panel-presentation-timing.mjs';

export const airborneChannel='flight.ownship.inAir';
const sampleCount=17;

export async function withInspectorPresentation(cdp, submitOwnship, readScope, run) {
  const inspector=await openInspectorPresentation(cdp);
  const before=readScope();
  try {
    const result=await run(()=>inspector.measure(submitOwnship));
    const after=readScope(), presentation=result.inspectorPresentation;
    if(after.previewError)throw after.previewError;
    assert.ok(before.active&&after.active,'The same native telemetry observer must remain active');
    assert.equal(after.unscopedInspectorReads,before.unscopedInspectorReads,'Inspector reads must use its exact observation');
    presentation.scopedReads=after.inspectorReads-before.inspectorReads;
    assert.ok(presentation.scopedReads>=presentation.samples.length);
    const budgetMisses=presentation.udpToFrameUpperMs.p95>100?['Foreground inspector presentation exceeded 100 ms p95']:[];
    return {...result,budgetMisses};
  } finally { await inspector.close(); }
}

// Only timestamps from the running Svelte WebView count. CDP observes the
// finished samples without supplying their timestamp or refreshing telemetry.
async function openInspectorPresentation(cdp) {
  const clock=()=>alignClock({evaluate:()=>cdp.evaluate('performance.now()')});
  await cdp.evaluate(`(() => {
    const section=document.querySelector('[aria-label="Incoming game telemetry"]');
    const game=section.querySelector('[aria-label="Telemetry game"]');
    game.value='dcs'; game.dispatchEvent(new Event('change',{bubbles:true}));
    const search=section.querySelector('[aria-label="Search game telemetry"]');
    search.value=${JSON.stringify(airborneChannel)};
    search.dispatchEvent(new Event('input',{bubbles:true}));
    const findCell=()=>{
      const label=[...section.querySelectorAll('tbody th code')]
        .find(node=>node.textContent===${JSON.stringify(airborneChannel)});
      return label?.closest('tr')?.querySelector('.channel-value');
    };
    const read=()=>{
      if(document.visibilityState!=='visible'||section.closest('[hidden]'))return null;
      const cell=findCell(),rect=cell?.getBoundingClientRect();
      if(!rect||rect.width===0||rect.height===0||rect.top<0||rect.bottom>innerHeight)return null;
      return cell.textContent.trim();
    };
    const samples=[];
    let previous='False';
    const observer=new MutationObserver(()=>{
      const value=read();
      if(!['True','False'].includes(value)||value===previous||samples.length>=${sampleCount})return;
      previous=value;
      const sample={value,domAt:performance.now()};
      samples.push(sample);
      requestAnimationFrame(()=>{sample.frameAt=performance.now();});
    });
    window.__dcsInspectorPresentation={read,samples,
      show(){const cell=findCell();cell?.scrollIntoView({block:'center'});return !!cell;},
      start:()=>observer.observe(section,{childList:true,characterData:true,subtree:true}),
      close:()=>observer.disconnect()};
  })()`);
  try {
    await until(()=>cdp.evaluate('window.__dcsInspectorPresentation.show()'),Boolean,2500);
    await until(()=>cdp.evaluate('window.__dcsInspectorPresentation.read()'),value=>value==='False',2500);
    const initial=await clock();
    await cdp.evaluate('window.__dcsInspectorPresentation.start()');
    return {
      async measure(submitOwnship) {
        const samples=[];
        for(let index=0;index<sampleCount;index++){
          if(index)await delay(100);
          const airborne=index%2===0;
          const submission=await submitOwnship(airborne);
          const sample=await until(()=>cdp.evaluate(`window.__dcsInspectorPresentation.samples[${index}] ?? null`),
            value=>Number.isFinite(value?.frameAt),1000);
          assert.equal(sample.value,airborne?'True':'False');
          samples.push({...submission,...sample});
        }
        const final=await clock();
        const low=Math.max(initial.low,final.low),high=Math.min(initial.high,final.high);
        assert.ok(high>=low,'Cockpit WebView clock alignment changed during the burst');
        for(const [index,sample] of samples.entries()){
          assert.ok(sample.domAt-high>=sample.submittedMs&&sample.frameAt>=sample.domAt,
            'Inspector value must follow its UDP submission and precede the render opportunity');
          if(index)assert.ok(sample.packetSequence>samples[index-1].packetSequence);
        }
        const upper=field=>summarize(samples.map(sample=>sample[field]-low-sample.submittedMs));
        return {samples,initial,final,uncertaintyMs:(high-low)/2,renderedFromMs:samples[0].frameAt-high,
          renderedThroughMs:samples.at(-1).frameAt-low,udpToDomUpperMs:upper('domAt'),udpToFrameUpperMs:upper('frameAt')};
      },
      close:()=>closeInspectorPresentation(cdp),
    };
  }catch(error){
    await closeInspectorPresentation(cdp);
    throw error;
  }
}

function closeInspectorPresentation(cdp) {
  return cdp.evaluate(`(() => {
    window.__dcsInspectorPresentation?.close();
    delete window.__dcsInspectorPresentation;
  })()`);
}
