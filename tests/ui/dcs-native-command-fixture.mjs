import assert from 'node:assert/strict';
import {createSocket} from 'node:dgram';
import {createHash,randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';
import {canonicalWidget} from '../../packages/widget-runtime/src/artifact.ts';
import {compiledPanelCapabilityProjection,verifyCompiledPanelPublication} from '../../packages/widget-runtime/src/panel.ts';
import {alignClock} from './dcs-panel-presentation-timing.mjs';
import {intervals,summarizeMilliseconds} from './vjoy-rate-transport.mjs';

const debug='native-command-debug';
const state={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
const event={delivery:'event',schema:{type:'boolean'}};
const port=(id,direction,contract)=>({id,name:id,direction,contract});
const extendedPairs=24;
const soakPairLimit=180;
const pairIntervalMs=180;
const nodeMs=ns=>Number(ns)/1e6;
const digest=value=>createHash('sha256').update(value).digest('hex');

export async function commandPanelPublication(original) {
  const document=JSON.parse(original.canonicalDocument);
  const widget=document.presentation.widgets.find(widget=>widget.id==='probe');
  const entry=document.presentation.artifacts.find(entry=>entry.hash===widget.artifactHash);
  const {artifact}=entry, {behavior,resources}=artifact;
  const region=behavior.regions.find(region=>region.id==='interaction');
  behavior.events.push({id:'release'});
  region.transitions.push({id:'release',event:'release',from:'ready',
    actions:[{type:'emit',port:'button',value:{literal:false}}]});
  artifact.hitRegions.find(region=>region.id==='press').events={press:'activate',release:'release',cancel:'release'};
  artifact.behaviorHash=digest(canonicalWidget(behavior));
  for(let attempt=0;attempt<8;attempt++) {
    const manifestBytes=Buffer.byteLength(canonicalWidget(artifact));
    const delta=manifestBytes-resources.manifestBytes;
    if(delta===0)break;
    resources.manifestBytes=manifestBytes;
    resources.residentBytes+=delta*4;
    resources.activeCandidateBytes=resources.residentBytes*2;
  }
  const previousHash=entry.hash;
  entry.hash=digest(canonicalWidget(artifact));
  for(const widget of document.presentation.widgets) {
    if(widget.artifactHash===previousHash)widget.artifactHash=entry.hash;
  }
  const canonicalDocument=canonicalWidget(document), assets=original.assets;
  const publicationHash='sha256:'+digest(canonicalDocument);
  const capabilityFingerprint='sha256:'+digest(canonicalWidget(compiledPanelCapabilityProjection(document)));
  await verifyCompiledPanelPublication({document,assets,publicationHash,capabilityFingerprint});
  return {canonicalDocument,assets,publicationHash,capabilityFingerprint};
}

async function waitUntil(target,signal) {
  let remaining=target-process.hrtime.bigint();
  while(remaining>0n) {
    await delay(Math.max(1,nodeMs(remaining)),undefined,{signal});
    remaining=target-process.hrtime.bigint();
  }
}

function sendPanelKey(button,type) {
  return button.evaluate((element,type)=>{
    const atMs=performance.now();
    element.dispatchEvent(new KeyboardEvent(type,{key:'Enter',bubbles:true,cancelable:true}));
    return atMs;
  },type);
}

function summarizeSource(samples,clock) {
  const offers=samples.map(sample=>sample.offeredAtMs);
  const arrivalDelta=samples.map(sample=>nodeMs(sample.receivedAtNs)-sample.offeredAtMs);
  if(clock)assert.ok(arrivalDelta.every(delta=>delta+clock.high>=0),'Command receipt precedes browser activation');
  return {samples,offerIntervalsMs:samples.length>1?summarizeMilliseconds(intervals(offers)):null,
    receiptLatencyMs:{
      lower:summarizeMilliseconds(arrivalDelta.map(delta=>clock?Math.max(0,delta+clock.low):delta)),
      upper:summarizeMilliseconds(arrivalDelta.map(delta=>delta+(clock?.high??0))),
    }};
}

// The real DCS-BIOS command sender writes only to this private UDP receiver.
export function nativeDcsCommands(cdp,request) {
  const socket=createSocket('udp4'), datagrams=[], expected=[], sent=[];
  let bound=false, ran=false, observedResource;
  socket.on('message',message=>{
    if(datagrams.length<=2*soakPairLimit)datagrams.push({text:message.toString('utf8'),atNs:String(process.hrtime.bigint())});
  });

  return {
    async open() {
      assert.equal(bound,false);
      await new Promise((resolve,reject)=>{
        socket.once('error',reject);
        socket.bind({port:7778,address:'127.0.0.1',exclusive:true},()=>{socket.removeListener('error',reject);resolve();});
      });
      bound=true;
      return socket.address().port;
    },

    async attach(document,assignments,vehicleId) {
      assert.equal(bound,true,'Open the DCS command receiver before setup');
      await cdp.evaluate(`(async()=>{
        const {SharedObservations}=await import('/src/lib/streams/shared.ts');
        const {createNativeObservationTransport}=await import('/src/lib/streams/native.ts');
        const observer=new SharedObservations(createNativeObservationTransport());
        const trace=window.__dcsCommandTrace={reasons:[],close:()=>observer.close()};
        observer.subscribe('profiles.runtime',state=>{
          const status=state.frame?.payload?.profileSession;
          for(const reason of [...(status?.degradation??[]),status?.fault].filter(Boolean)) {
            if(!trace.reasons.includes(reason)&&trace.reasons.length<32)trace.reasons.push(reason);
          }
        });
      })()`);
      const {resource:observed}=(await request({command:'profile_dcs_vehicle_resource',vehicleId})).profileResources;
      observedResource=observed.id;
      const resource={id:'aircraft-commands',name:'Aircraft commands',kind:'game',required:true,ports:[]};
      const assignment={resourceId:resource.id,sourceId:observed.id,ports:[]};
      document.resources.push(resource);
      assignments.push(assignment);
      const nodeIds=[resource.id];
      document.vehicleTabs=[...(document.vehicleTabs??[]),{id:'aircraft-command-tab',gameId:'dcs',vehicleId,vehicleName:vehicleId,nodeIds}];
      const panel=document.resources.find(row=>row.id==='combined-browser-panel');
      const press=panel?.ports.find(row=>row.id==='press');
      assert.ok(press,'The browser panel press must already be assigned');
      const actions=[];
      for(let offset=0,total=1;offset<total&&actions.length<2;){
        const page=(await request({command:'profile_resources_inspect',resourceId:observed.id,offset,limit:128})).profileResources;
        total=page.total;
        assert.ok(page.ports.length||offset===total,'Aircraft catalogue paging must advance');
        offset+=page.ports.length;
        actions.push(...page.ports.filter(row=>row.current&&row.descriptor.id.startsWith('command:')
          &&row.descriptor.id.endsWith(':fixed_step')&&row.descriptor.direction==='input'));
      }
      assert.ok(actions.length>=2,'The current DCS aircraft must advertise two fixed-step commands');
      document.resources.push({id:debug,name:'DCS command debug input',kind:'debug-controller',required:false,
        ports:[port('button-1','output',state)]});
      const sources=[
        {nodeId:debug,portId:'button-1',contract:state,tag:'debug',value:'INC'},
        {nodeId:panel.id,portId:'press',contract:press.contract,tag:'panel',value:'DEC'},
      ];
      for(const [index,{nodeId,portId,contract,tag,value}] of sources.entries()){
        const action=actions[index], prefix='native-command-'+tag;
        const logical=prefix+'-step', edge=prefix+'-edge', constant=prefix+'-value', pack=prefix+'-action';
        nodeIds.push(edge,constant,pack);
        const {parameters,semantics}=action.descriptor.contract;
        assert.equal(semantics,'one-shot');
        assert.deepEqual(parameters.map(parameter=>parameter.id),['value']);
        const argument={delivery:'state',schema:parameters[0].schema,freshnessMs:1000,fallback:null};
        resource.ports.push({...action.descriptor,id:logical});
        assignment.ports.push({profilePortId:logical,observedPortId:action.descriptor.id,
          expectedContractHash:action.contractHash});
        document.processingNodes.push(
          {id:edge,name:'Rising edge '+tag,operation:{kind:'event-edge',edge:'rising'},
            inputs:[port('in','input',contract)],outputs:[port('out','output',event)]},
          {id:constant,name:'Fixed-step value '+tag,operation:{kind:'constant',value},
            inputs:[],outputs:[port('out','output',argument)]},
          {id:pack,name:'DCS command '+tag,operation:{kind:'action-pack',arguments:['value']},
            inputs:[port('fire','input',event),port('value','input',argument)],
            outputs:[port('invoke','output',{delivery:'invocation',parameters,semantics})]},
        );
        const bind=(id,fromNode,fromPort,toNode,toPort)=>document.bindings.push({
          id:prefix+'-'+id,from:{nodeId:fromNode,portId:fromPort},to:{nodeId:toNode,portId:toPort},
        });
        bind('press',nodeId,portId,edge,'in');
        bind('fire',edge,'out',pack,'fire');
        bind('argument',constant,'out',pack,'value');
        bind('command',pack,'invoke',resource.id,logical);
        expected.push(action.descriptor.id.split(':')[1]+' '+value+'\n');
      }
    },

    runSoak(owner,start,durationMs,page,signal) {
      const leadMs=1000;
      const intervalMs=Math.max(1000,Math.ceil((durationMs-leadMs)/soakPairLimit));
      return this.run(owner,start,durationMs,page,{signal,intervalMs,leadMs,pairLimit:soakPairLimit});
    },

    async run(owner,start,durationMs,page,{signal,intervalMs=pairIntervalMs,leadMs=0,pairLimit=extendedPairs}={}) {
      assert.equal(bound,true);
      assert.deepEqual(datagrams,[],'One-shot commands must stay quiet at baseline');
      assert.notEqual(expected[0],expected[1],'The two sources need distinct command receipts');
      const button=page.getByRole('button',{name:'probe: press',exact:true});
      const initial=await alignClock(page), streamId=randomUUID();
      const debugSamples=[], panelSamples=[];
      const pairs=Math.min(pairLimit,Math.floor((durationMs-leadMs)/intervalMs));
      assert.ok(pairs>0,'The mixed workload needs room for a command pair');
      const deadline=start+BigInt(durationMs)*1_000_000n;
      let sequence=0;
      const debugInput=value=>request({command:'profile_debug_controller_input',owner,nodeId:debug,
        streamId,sequence:String(++sequence),changes:[{portId:'button-1',value}]});
      const receipt=async (source,offeredAtMs)=>{
        const index=sent.length;
        sent.push(expected[source]);
        while(datagrams.length<=index&&process.hrtime.bigint()<deadline)await delay(2,undefined,{signal});
        assert.equal(datagrams[index]?.text,expected[source],`Missing or out-of-order DCS command at index ${index}`);
        assert.equal(datagrams.length,index+1,'A command must have exactly one receipt');
        return {offeredAtMs,receivedAtNs:datagrams[index].atNs,text:datagrams[index].text};
      };
      try {
        for(let index=0;index<pairs;index++) {
          signal?.throwIfAborted();
          const pairStart=start+BigInt(leadMs+index*intervalMs)*1_000_000n;
          await waitUntil(pairStart+25_000_000n,signal);
          const debugAtMs=nodeMs(process.hrtime.bigint());
          await debugInput(true);
          debugSamples.push(await receipt(0,debugAtMs));
          await debugInput(false);
          await waitUntil(pairStart+65_000_000n,signal);
          const panelAtMs=await sendPanelKey(button,'keydown');
          try {panelSamples.push(await receipt(1,panelAtMs));}
          finally {await sendPanelKey(button,'keyup');}
        }
        assert.deepEqual(datagrams.map(packet=>packet.text),sent,'Both authored controls must reach the private receiver once per press');
      } catch(error) {
        const inspection=(await request({command:'profile_resources_inspect',resourceId:observedResource,offset:0,limit:1})).profileResources;
        console.error('DCS command receiver inventory', JSON.stringify(inspection.resource?.connections));
        console.error('DCS command receiver diagnostics', await cdp.evaluate('window.__dcsCommandTrace.reasons'));
        throw error;
      }
      const final=await alignClock(page);
      const clock={low:Math.max(initial.low,final.low),high:Math.min(initial.high,final.high)};
      assert.ok(clock.high>=clock.low,'Browser clock alignment changed during the command samples');
      ran=true;
      return {initial,final,clock,debug:summarizeSource(debugSamples),panel:summarizeSource(panelSamples,clock),
        datagrams:datagrams.map(packet=>({...packet}))};
    },

    async close() {
      if(!bound)return;
      bound=false;
      await new Promise(resolve=>socket.close(resolve));
      await cdp.evaluate('window.__dcsCommandTrace?.close(); delete window.__dcsCommandTrace');
      if(ran)assert.deepEqual(datagrams.map(packet=>packet.text),sent,'Stop and reapply must not replay one-shot commands');
    },
  };
}
