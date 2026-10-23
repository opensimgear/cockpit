import test from 'node:test';
import assert from 'node:assert/strict';
import {compileWidget,createLinearGaugeFixture,createRadialGaugeFixture,decodeCompiledFrame,preflightWidget} from '../src/index.ts';
import {CompiledWidgetPlayer} from '../../widget-runtime/src/player.ts';

const fresh=value=>({status:'fresh',value,provenance:'game-confirmed',sourceKind:'simulator'});

test('native-sized RPM fixtures sweep static frames with raw hysteresis, independent quality and fresh recovery',async()=>{
  for(const factory of [createLinearGaugeFixture,createRadialGaugeFixture]){
    const source=factory(),preflight=preflightWidget(source,{motion:false,audio:false}),bundle=await compileWidget(source,{motion:false,audio:false}),player=new CompiledWidgetPlayer(bundle);
    assert.equal(bundle.artifact.motions.length,0);assert.equal(bundle.artifact.sounds.length,0);
    assert.equal(preflight.baseFrames,(source.id.includes('linear')?129:97)*4*2);assert(preflight.domains.find(d=>d.signal==='rpm').maxErrorPixels<=(source.id.includes('linear')?1:2));
    assert.equal(player.state.values.quality,'unknown');let now=0;
    const send=sample=>{const result=player.dispatch({type:'event',event:'feedback',feedback:{rpm:sample},nowMs:++now});assert.equal(result.error,undefined);return result;};
    assert.deepEqual(send(fresh(6980)).effects,[]);assert.equal(player.state.regions.alarm,'normal');
    const before=player.frameId;assert.deepEqual(send(fresh(6999)).effects,[]);assert.equal(player.state.values.rpm,6999);assert.equal(player.state.regions.alarm,'normal');
    const enter=send(fresh(7000));assert.deepEqual(enter.effects,[{type:'cue',cue:'redline'}]);assert.equal(player.state.regions.alarm,'redline');assert.notEqual(player.frameId,before,'Raw threshold changes alarm artwork independently of rounded RPM');
    for(const value of [6999,6980,6950,6801]){assert.deepEqual(send(fresh(value)).effects,[]);assert.equal(player.state.regions.alarm,'redline');}
    send(fresh(6800));assert.equal(player.state.regions.alarm,'normal');assert.equal(player.state.regions.sampling,'ready');
    for(const value of [...Array.from({length:161},(_,i)=>i*50),...Array.from({length:160},(_,i)=>7950-i*50)]){
      send(fresh(value));assert.equal(player.state.values.rpm,value);
      const frame=bundle.artifact.frames.find(frame=>frame.id===player.frameId),sample=frame.selector.values.rpm,domain=bundle.artifact.domains.find(d=>d.signal==='rpm');
      assert(Math.abs(sample-value)/8000*source.visualDomains[0].travelPixels<=domain.maxErrorPixels+1e-9);
      assert.equal(frame.selector.values.quality,'fresh');assert.equal(frame.selector.regions.alarm,player.state.regions.alarm);
    }
    send(fresh(8000));const fullPixels=decodeCompiledFrame(bundle.artifact,bundle.assets,player.frameId);const expired=player.sample(now+=500);assert.equal(expired.error,undefined);assert.equal(player.state.values.quality,'stale');assert.equal(player.state.values.rpm,0);assert.equal(player.state.regions.alarm,'normal');assert.deepEqual(expired.effects,[]);
    const stalePixels=decodeCompiledFrame(bundle.artifact,bundle.assets,player.frameId);assert.notDeepEqual(stalePixels,fullPixels);
    send({status:'disconnected'});assert.equal(player.state.values.quality,'disconnected');assert.equal(player.state.values.rpm,0);
    send({status:'unknown'});assert.equal(player.state.values.quality,'unknown');
    send({...fresh(8000),provenance:'host-accepted',sourceKind:'host'});assert.equal(player.state.values.quality,'unknown');assert.equal(player.state.regions.alarm,'normal');
    send(fresh(4000));assert.equal(player.state.values.quality,'fresh');assert.equal(player.state.values.rpm,4000);assert.equal(player.state.regions.alarm,'normal');
    const captured=player.state,bad=player.dispatch({type:'event',event:'feedback',feedback:{rpm:fresh(8001)},nowMs:++now});assert(bad.error);assert.equal(player.state,captured);
  }
});
