import {performance} from 'node:perf_hooks';
import {BROWSER_WIDGET_CAPABILITIES,compileWidget,clearWidgetCompileCache,createToggleSource,createLinearGaugeFixture,createRadialGaugeFixture,createThreeStateFixture,preflightWidget} from '../src/index.ts';
import {createCompiledPixelDecoder} from '../../widget-runtime/src/artifact.ts';

// Reproducible artifact/resource identities; timings are observations of this host.
const rows=[];
for(const [factory,motion] of [[createToggleSource,false],[createToggleSource,true],[createLinearGaugeFixture,false],[createRadialGaugeFixture,false],[createThreeStateFixture,false]]){
  const source=factory(),options={width:source.size.width,height:source.size.height,motion,audio:false,capabilities:BROWSER_WIDGET_CAPABILITIES};
  const preflight=preflightWidget(source,options);clearWidgetCompileCache();const began=performance.now(),bundle=await compileWidget(source,options),compileMs=performance.now()-began;
  const decoder=createCompiledPixelDecoder(bundle.artifact,bundle.assets),decodeBegan=performance.now(),iterations=100;
  for(let i=0;i<iterations;i++)decoder.decode(bundle.artifact.frames[Math.floor(i*(bundle.artifact.frames.length-1)/(iterations-1))].id);
  const decodeMeanMs=(performance.now()-decodeBegan)/iterations;decoder.dispose();
  rows.push({id:source.id,options,sourceHash:bundle.artifact.sourceHash,parameterHash:bundle.artifact.parameterHash,behaviorHash:bundle.artifact.behaviorHash,capabilityHash:bundle.artifact.capabilityHash,artifactHash:bundle.artifactHash,compiler:bundle.artifact.compiler,rasterizer:bundle.artifact.rasterizer,domains:preflight.domains,baseFrames:preflight.baseFrames,maximumFrames:preflight.maximumFrames,resources:bundle.artifact.resources,compileMs,decodeIterations:iterations,decodeMeanMs});
}
console.log(JSON.stringify({runtime:process.version,platform:process.platform,architecture:process.arch,accounting:'Normative conservative storage allowances, not measured JavaScript heap usage.',rows},null,2));
