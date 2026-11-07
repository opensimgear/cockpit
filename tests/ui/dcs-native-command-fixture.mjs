import assert from 'node:assert/strict';
import {createSocket} from 'node:dgram';
import {randomUUID} from 'node:crypto';
import {setTimeout as delay} from 'node:timers/promises';

const debug='native-command-debug';
const state={delivery:'state',schema:{type:'boolean'},freshnessMs:1000,fallback:false};
const event={delivery:'event',schema:{type:'boolean'}};
const port=(id,direction,contract)=>({id,name:id,direction,contract});

// The real DCS-BIOS command sender writes only to this private UDP receiver.
export function nativeDcsCommands(cdp,request) {
  const socket=createSocket('udp4'), datagrams=[], expected=[];
  let bound=false, ran=false, observedResource;
  socket.on('message',message=>{
    if(datagrams.length<16)datagrams.push({text:message.toString('utf8'),atNs:String(process.hrtime.bigint())});
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

    async run(owner,start,pressPanel) {
      assert.equal(bound,true);
      assert.deepEqual(datagrams,[],'One-shot commands must stay quiet at baseline');
      const at=async offset=>{
        const target=start+BigInt(offset)*1_000_000n;
        let remaining=target-process.hrtime.bigint();
        while(remaining>0n){
          await delay(Math.max(1,Number(remaining)/1e6));
          remaining=target-process.hrtime.bigint();
        }
        return String(process.hrtime.bigint());
      };
      const debugPress=async()=>{
        const sentAt=await at(25);
        await request({command:'profile_debug_controller_input',owner,nodeId:debug,
          streamId:randomUUID(),sequence:'1',changes:[{portId:'button-1',value:true}]});
        return sentAt;
      };
      const panelPress=async()=>{const sentAt=await at(60);await pressPanel();return sentAt;};
      const [debugAtNs,panelAtNs]=await Promise.all([debugPress(),panelPress()]);
      while(datagrams.length<expected.length&&process.hrtime.bigint()<start+190_000_000n)await delay(2);
      if (datagrams.length!==expected.length) {
        const inspection=(await request({command:'profile_resources_inspect',resourceId:observedResource,offset:0,limit:1})).profileResources;
        console.error('DCS command receiver inventory', JSON.stringify(inspection.resource?.connections));
        console.error('DCS command receiver diagnostics', await cdp.evaluate('window.__dcsCommandTrace.reasons'));
      }
      assert.deepEqual(datagrams.map(packet=>packet.text),expected,'Both authored controls must reach the private receiver once');
      ran=true;
      return {debugAtNs,panelAtNs,datagrams:datagrams.map(packet=>({...packet}))};
    },

    async close() {
      if(!bound)return;
      bound=false;
      await new Promise(resolve=>socket.close(resolve));
      await cdp.evaluate('window.__dcsCommandTrace?.close(); delete window.__dcsCommandTrace');
      if(ran)assert.deepEqual(datagrams.map(packet=>packet.text),expected,'Stop and reapply must not replay one-shot commands');
    },
  };
}
