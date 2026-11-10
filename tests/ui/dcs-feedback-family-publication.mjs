// Extend the compiled timing panel with an independent BIOS altitude indicator.
import assert from 'node:assert/strict';
import {canonicalWidget,widgetDigest} from '../../packages/widget-runtime/src/artifact.ts';
import {compiledPanelCapabilityProjection,verifyCompiledPanelPublication} from '../../packages/widget-runtime/src/panel.ts';

export const biosFeedbackPortId='feedback:flight-channel-3:flight-output-3';

const hash=async value=>widgetDigest(new TextEncoder().encode(canonicalWidget(value)));
const parseOriginal=original=>{
  const publication=typeof original==='string'||Buffer.isBuffer(original)
    ?JSON.parse(String(original)):structuredClone(original);
  assert.ok(publication?.canonicalDocument&&Array.isArray(publication.assets),'Expected a compiled panel publication');
  return publication;
};

function biosIndicatorArtifact(source) {
  const artifact=structuredClone(source);
  artifact.id='acceptance-timing-bios';
  artifact.behavior={version:1,
    signals:[{id:'lamp',source:'feedback',type:'boolean',initial:false}],
    events:[{id:'feedback',updates:['lamp']}],outputs:[],regions:[]};
  artifact.inputs=[];
  artifact.outputs=[{id:'lamp',label:'Indicator',type:'boolean',signal:'lamp'}];
  artifact.hitRegions=[];
  artifact.frames=artifact.frames.map(frame=>({
    ...frame,
    selector:{values:{lamp:frame.selector.values.lamp},regions:{}},
  }));
  // Keep the digest width stable while the manifest resource size is measured.
  artifact.behaviorHash='0'.repeat(64);
  artifact.resources.manifestBytes=0;
  for(let attempt=0;attempt<8;attempt++){
    artifact.resources.manifestBytes=Buffer.byteLength(canonicalWidget(artifact));
    artifact.resources.residentBytes=artifact.resources.transferBytes*3+artifact.resources.dictionaryBytes
      +artifact.resources.manifestBytes*4+artifact.resources.frameMapBytes*2
      +artifact.width*artifact.height*8+768+artifact.resources.audioWorkingBytes;
    artifact.resources.activeCandidateBytes=artifact.resources.residentBytes*2;
    const measured=Buffer.byteLength(canonicalWidget(artifact));
    if(measured===artifact.resources.manifestBytes)break;
  }
  return artifact;
}

/** Clone the existing compiled raster panel, add one boolean feedback channel, and reverify hashes. */
export async function withBiosFeedback(original) {
  const source=parseOriginal(original);
  const document=JSON.parse(source.canonicalDocument);
  const timing=document.presentation.widgets.find(widget=>widget.id==='probe');
  assert.ok(timing,'The compiled timing widget is missing');
  const sourceEntry=document.presentation.artifacts.find(entry=>entry.hash===timing.artifactHash);
  assert.ok(sourceEntry,'The timing widget artifact is missing');

  const artifact=biosIndicatorArtifact(sourceEntry.artifact);
  artifact.behaviorHash=await hash(artifact.behavior);
  const artifactHash=await hash(artifact);
  const feedbackId='flight-output-3';
  document.presentation.artifacts.push({hash:artifactHash,artifact});
  document.presentation.widgets.push({
    id:'bios-indicator',pageId:timing.pageId,label:'BIOS indicator',
    bounds:{...timing.bounds,x:timing.bounds.x+timing.bounds.width},
    artifactHash,inputBindings:[],outputBindings:[{feedbackId,portId:'lamp'}],
  });
  document.ui.feedbackWidgets.push({id:feedbackId,label:'BIOS altitude indicator',
    channelId:'flight-channel-3',kind:'indicator'});

  const publication={canonicalDocument:canonicalWidget(document),assets:source.assets};
  publication.publicationHash='sha256:'+await hash(document);
  publication.capabilityFingerprint='sha256:'+await hash(compiledPanelCapabilityProjection(document));
  const verified=await verifyCompiledPanelPublication({document,assets:source.assets,
    publicationHash:publication.publicationHash,capabilityFingerprint:publication.capabilityFingerprint});
  return {...publication,canonicalDocument:canonicalWidget(verified.document),assets:verified.assets};
}

/** Attach a numeric DCS-BIOS altitude source to a Boolean companion indicator. */
export function attachBiosFeedback(document,panelNodeId,biosPortId,panelContract) {
  const telemetry=document.resources?.find(resource=>resource.id==='telemetry');
  const telemetryPort=telemetry?.ports?.find(port=>port.id===biosPortId);
  assert.ok(telemetryPort,'The selected BIOS altitude port must exist in the telemetry resource');
  assert.equal(telemetryPort.direction,'output');
  assert.ok(['integer','number'].includes(telemetryPort.contract?.schema?.type),
    'The BIOS altitude port must have a numeric contract');
  assert.equal(panelContract?.schema?.type,'boolean','The companion sink must be Boolean');
  const sink=document.resources?.find(resource=>resource.id===panelNodeId)?.ports
    ?.find(port=>port.id==='bios-lamp');
  assert.ok(sink,'The companion resource must expose bios-lamp');
  assert.deepEqual(sink.contract,panelContract,'The sink contract must match the provided panel contract');

  const nodeId=panelNodeId+'-bios-altitude';
  assert.ok(!document.processingNodes.some(node=>node.id===nodeId),'The BIOS compare node already exists');
  document.processingNodes.push({id:nodeId,name:'BIOS altitude warning',
    operation:{kind:'compare',comparison:'greater-equal',threshold:500},
    inputs:[{id:'in',name:'Altitude',direction:'input',contract:telemetryPort.contract}],
    outputs:[{id:'out',name:'Altitude warning',direction:'output',contract:panelContract}],
  });
  document.bindings.push(
    {id:nodeId+'-telemetry',from:{nodeId:'telemetry',portId:biosPortId},to:{nodeId,portId:'in'}},
    {id:nodeId+'-panel',from:{nodeId,portId:'out'},to:{nodeId:panelNodeId,portId:'bios-lamp'}},
  );
  return nodeId;
}
