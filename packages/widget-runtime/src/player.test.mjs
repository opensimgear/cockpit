import assert from 'node:assert/strict';
import test from 'node:test';
import { CompiledWidgetPlayer, CompiledSoundPlayer, createBrowserAudioDriver, selectCompiledFrame } from './player.ts';

function bundle() {
  return { artifact: {
    behavior: { version:1, signals:[{id:'requested',type:'boolean',source:'local',initial:false}], events:[{id:'activate'}], outputs:[{id:'command',type:'boolean',neutral:false}], regions:[{id:'control',initial:'ready',states:['ready'],transitions:[{id:'toggle',from:'ready',event:'activate',actions:[{type:'set',signal:'requested',value:{op:'not',value:{ref:'requested'}}},{type:'emit',port:'command',value:{op:'not',value:{ref:'requested'}}},{type:'cue',cue:'click'}]}]}] },
    domains:[{signal:'requested',values:[false,true],numeric:false,maxErrorPixels:0}],
    frames:[{id:'off',tiles:[],selector:{values:{requested:false},regions:{control:'ready'}}},{id:'on',tiles:[],selector:{values:{requested:true},regions:{control:'ready'}}},...['p1','p2','p3'].map(id=>({id,tiles:[]}))],
    motions:[{id:'switch',fromFrameId:'off',toFrameId:'on',frames:['off','p1','p2','p3','on'],durationMs:100,interruption:'reverse'}],
    sounds:[{id:'click-sound',cue:'click',assetId:'wave',volume:.5,cooldownMs:50,stopOnExit:true}],
  }, assets:[{assetId:'wave',base64:'AQID'}], artifactHash:'unused-test-fixture' };
}
test('compiled selection quantizes finite domains and matches orthogonal region states', () => {
  const artifact = bundle().artifact;
  assert.equal(selectCompiledFrame(artifact,{values:{requested:true},regions:{control:'ready'}}),'on');
  artifact.domains=[{signal:'rpm',values:[0,50,100],numeric:true}];
  artifact.frames=[0,50,100].map(v=>({id:String(v),selector:{values:{rpm:v},regions:{control:'ready'}}}));
  assert.equal(selectCompiledFrame(artifact,{values:{rpm:74},regions:{control:'ready'}}),'50');
  assert.equal(selectCompiledFrame(artifact,{values:{rpm:10000},regions:{control:'ready'}}),'100');
  assert.equal(selectCompiledFrame(artifact,{values:{rpm:'unknown'},regions:{control:'ready'}}),'0');
  assert.throws(()=>selectCompiledFrame(artifact,{values:{rpm:0},regions:{control:'missing'}}),/No compiled presentation/);
});
test('semantic outputs are immediate while baked motion reverses from displayed progress', () => {
  const player = new CompiledWidgetPlayer(bundle());
  assert.equal(player.frameId,'off');
  const first=player.dispatch({type:'event',event:'activate',nowMs:0});
  assert.equal(first.state.values.requested,true);
  assert.deepEqual(first.effects,[{type:'emit',port:'command',value:true},{type:'cue',cue:'click'}]);
  assert.equal(player.sample(50).frameId,'p2');
  assert.equal(player.animating,true);
  player.dispatch({type:'event',event:'activate',nowMs:50});
  assert.equal(player.frameId,'p2');
  assert.equal(player.sample(75).frameId,'p1');
  assert.equal(player.sample(100).frameId,'off');
  assert.equal(player.animating,false);
  assert.match(player.dispatch({type:'event',event:'missing',nowMs:100}).error,/Unknown behavior event/);
});
test('static and hidden players keep the same logical traces without decorative playback', () => {
  const player=new CompiledWidgetPlayer(bundle());
  player.setMotionEnabled(false);
  assert.equal(player.dispatch({type:'event',event:'activate',nowMs:1}).frameId,'on');
  player.setMotionEnabled(true); player.setVisible(false);
  assert.equal(player.dispatch({type:'event',event:'activate',nowMs:2}).frameId,'off');
  player.setVisible(true); player.dispatch({type:'event',event:'activate',nowMs:3});
  player.setMotionEnabled(false); assert.equal(player.frameId,'on');
  player.setMotionEnabled(true); player.setVisible(false); assert.equal(player.frameId,'on');
  assert.deepEqual(player.dispatch({type:'reset',nowMs:4}).effects,[{type:'emit',port:'command',value:false}]);
});
test('interrupt policy snaps unknown routes and restarts explicit routes', () => {
  const b=bundle(); b.artifact.motions[0].interruption='restart';
  b.artifact.motions.push({id:'back',fromFrameId:'on',toFrameId:'off',frames:['on','p3','p2','p1','off'],durationMs:100,interruption:'restart'});
  const player=new CompiledWidgetPlayer(b);
  player.dispatch({type:'event',event:'activate',nowMs:0}); player.sample(50);
  player.dispatch({type:'event',event:'activate',nowMs:50});
  assert.equal(player.sample(100).frameId,'p2');
  player.sample(150);
  assert.equal(player.frameId,'off');
  b.artifact.motions=[];
  const snap=new CompiledWidgetPlayer(b); assert.equal(snap.dispatch({type:'event',event:'activate',nowMs:0}).frameId,'on');
});
test('continuous smoothing walks existing samples, retargets current progress and keeps raw behavior values', () => {
  const b=bundle();b.artifact.behavior={version:1,signals:[{id:'rpm',type:'number',source:'input',initial:0,min:0,max:100}],events:[{id:'value',updates:['rpm']}],regions:[]};
  b.artifact.domains=[{signal:'rpm',values:[0,25,50,75,100],numeric:true,maxErrorPixels:1,smoothingMs:100}];
  b.artifact.frames=[0,25,50,75,100].map(v=>({id:'rpm'+v,tiles:[],selector:{values:{rpm:v},regions:{}}}));b.artifact.motions=[];
  const player=new CompiledWidgetPlayer(b);
  assert.equal(player.dispatch({type:'event',event:'value',updates:{rpm:100},nowMs:0}).frameId,'rpm0');
  assert.equal(player.state.values.rpm,100);assert.equal(player.animating,true);
  assert.equal(player.sample(50).frameId,'rpm50');
  player.dispatch({type:'event',event:'value',updates:{rpm:0},nowMs:50});
  assert.equal(player.sample(100).frameId,'rpm25');
  assert.equal(player.sample(150).frameId,'rpm0');assert.equal(player.animating,false);
  player.dispatch({type:'event',event:'value',updates:{rpm:100},nowMs:151});player.setMotionEnabled(false);
  assert.equal(player.frameId,'rpm100');
  assert.equal(player.dispatch({type:'event',event:'value',updates:{rpm:25},nowMs:152}).frameId,'rpm25');
});
function audioDriver() {
  const stopped=[],played=[];
  return {stopped,played,resume:async()=>{},play:async(bytes,volume)=>{const id=played.length;played.push({bytes,volume});return {stop:()=>stopped.push(id)};},close(){this.closed=true;}};
}
test('optional audio has no pregesture, muted, hidden or reconnect replay queue', async () => {
  const driver=audioDriver(), sound=new CompiledSoundPlayer(bundle(),driver);
  assert.equal(sound.availability,'locked');assert.equal(await sound.cue('click',0),false);
  await sound.unlock(); assert.equal(sound.availability,'available');assert.equal(driver.played.length,0);
  assert.equal(await sound.cue('click',1),true);assert.equal(await sound.cue('click',20),false);
  assert.equal(await sound.cue('unknown',60),false);assert.equal(await sound.cue('click',NaN),false);
  sound.exitState(); assert.equal(driver.stopped.length,1);
  sound.setMuted(true); assert.equal(sound.availability,'muted');assert.equal(await sound.cue('click',100),false);
  sound.setMuted(false);sound.setVisible(false);assert.equal(sound.availability,'hidden');assert.equal(await sound.cue('click',150),false);
  sound.setVisible(true);assert.equal(driver.played.length,1);assert.equal(await sound.cue('click',200),true);
  sound.close();assert.equal(await sound.cue('click',300),false);await sound.unlock();assert.equal(driver.closed,true);
  assert.equal(new CompiledSoundPlayer(bundle()).availability,'unsupported');
});
test('audio playback failures, missing assets and voice budget remain silent and bounded', async () => {
  const locked=new CompiledSoundPlayer(bundle(),{resume:async()=>{throw Error('Blocked');},play:async()=>{},close(){}});
  await locked.unlock();assert.equal(locked.availability,'blocked');
  const broken=new CompiledSoundPlayer(bundle(),{resume:async()=>{},play:async()=>{throw Error('Device gone');},close(){}});
  await broken.unlock();assert.equal(await broken.cue('click',0),false);assert.equal(broken.availability,'blocked');
  const b=bundle();b.assets=[];const missing=new CompiledSoundPlayer(b,audioDriver());await missing.unlock();assert.equal(await missing.cue('click',0),false);
  const sounds=new CompiledSoundPlayer(bundle(),audioDriver());await sounds.unlock();
  for(let i=0;i<4;i++)assert.equal(await sounds.cue('click',i*100),true);
  assert.equal(await sounds.cue('click',1000),false);
});
test('a sound decoded after mute is discarded and finished voices release capacity', async () => {
  let decoded,finish,stopped=false;
  const driver={resume:async()=>{},play:()=>new Promise(resolve=>decoded=resolve),close(){}};
  const sound=new CompiledSoundPlayer(bundle(),driver);await sound.unlock();
  const pending=sound.cue('click',0);sound.setMuted(true);decoded({stop(){stopped=true;}});
  assert.equal(await pending,false);assert.equal(stopped,true);
  sound.setMuted(false);
  const playing=sound.cue('click',100);decoded({stop(){},finished:new Promise(resolve=>finish=resolve)});assert.equal(await playing,true);
  finish();await Promise.resolve();
  sound.close();
});
test('authored exit policy preserves ongoing and decoding voices that may outlive a state', async () => {
  let decoded,stopped=0;const b=bundle();b.artifact.sounds[0].stopOnExit=false;
  const sound=new CompiledSoundPlayer(b,{resume:async()=>{},play:()=>new Promise(resolve=>decoded=resolve),close(){}});await sound.unlock();
  const pending=sound.cue('click',0);sound.exitState();decoded({stop(){stopped++;}});assert.equal(await pending,true);sound.exitState();assert.equal(stopped,0);sound.close();assert.equal(stopped,1);
});
test('browser audio adapter preserves admitted PCM sample rate and voice memory through gain', async () => {
  const original=globalThis.AudioContext;
  try {
    delete globalThis.AudioContext;assert.equal(createBrowserAudioDriver(),undefined);
    let context,closes=0;const samples=[-32768,32767,16384,-16384],wave=new Uint8Array(62),view=new DataView(wave.buffer);
    for(const [at,text]of[[0,'RIFF'],[8,'WAVE'],[12,'JUNK'],[22,'fmt '],[46,'data']])for(let i=0;i<text.length;i++)wave[at+i]=text.charCodeAt(i);
    view.setUint32(4,54,true);view.setUint32(16,1,true);view.setUint32(26,16,true);view.setUint16(30,1,true);view.setUint16(32,2,true);view.setUint32(34,8000,true);view.setUint32(38,32000,true);view.setUint16(42,4,true);view.setUint16(44,16,true);view.setUint32(50,8,true);samples.forEach((value,i)=>view.setInt16(54+i*2,value,true));
    globalThis.AudioContext=class {
      state='suspended';destination={};sources=[];
      constructor(){context=this;}
      async resume(){this.state='running';}
      createBuffer(channels,count,rate){assert.deepEqual([channels,count,rate],[2,2,8000]);const data=[new Float32Array(count),new Float32Array(count)];return{getChannelData:channel=>data[channel],data};}
      createBufferSource(){const source={connect(){},disconnect(){},start(){},stop(){this.onended?.();}};this.sources.push(source);return source;}
      createGain(){return {gain:{value:0},connect(){},disconnect(){}};}
      async close(){closes++;this.state='closed';}
    };
    const driver=createBrowserAudioDriver();await assert.rejects(driver.play(new Uint8Array(3),1),/locked/);
    await driver.resume();await assert.rejects(driver.play(new Uint8Array(3),.7),/WAV/);const voice=await driver.play(wave,.7);assert.deepEqual(Array.from(context.sources[0].buffer.data[0]),[-1,.5]);assert.deepEqual(Array.from(context.sources[0].buffer.data[1]),[32767/32768,-.5]);voice.stop();await voice.finished;
    await driver.play(wave,.2);driver.close();driver.close();assert.equal(context.state,'closed');assert.equal(closes,1);await assert.rejects(driver.play(wave,.2),/locked/);
    globalThis.AudioContext=class{state='suspended';async resume(){}};
    await assert.rejects(createBrowserAudioDriver().resume(),/locked/);
  } finally { if(original)globalThis.AudioContext=original;else delete globalThis.AudioContext; }
});
