import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import {compile} from 'svelte/compiler';
test('Debug exposes the shared HID service only while selected and uses no polling or auto-open',async()=>{
  const view=await readFile(new URL('./DebugHidInspector.svelte',import.meta.url),'utf8');
  assert.doesNotThrow(()=>compile(view,{generate:'client'}));
  const debug=await readFile(new URL('./ProfileDebugView.svelte',import.meta.url),'utf8');
  assert.ok(debug.includes("{id:'hid',label:'HID interfaces'}"));
  assert.ok(debug.includes("<DebugHidInspector active={active&&section==='hid'}/>"));
  assert.ok(view.includes('void current.dispose()'));
  assert.ok(view.includes('onclick={()=>controller?.open()}'));
  assert.ok(!view.includes('setInterval'));
  assert.ok(view.includes('Last observed descriptor · offline'));
  assert.ok(view.includes('<Icon name="close"'));
});

test('inspector modules compile and bound imported files before allocation or native import',async()=>{
  for(const file of ['HidContract','HidMonitor','HidOperations']){
    const source=await readFile(new URL(`./hid-inspector/${file}.svelte`,import.meta.url),'utf8');
    assert.doesNotThrow(()=>compile(source,{generate:'client'}));
    if(file==='HidOperations'){
      assert.ok(source.indexOf('file.size>MAX_IMPORT_BYTES')<source.indexOf('await file.text()'));
      for(const label of ['Import HID recording','HID replay seek time','HID GET report ID'])assert.ok(source.includes(`aria-label="${label}"`));
      assert.ok(source.includes('Replay does not inject inputs or send output reports'));
      assert.ok(!source.includes('controller_hid_open'));
    }
  }
});
