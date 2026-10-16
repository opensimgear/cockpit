import assert from 'node:assert/strict';
import test from 'node:test';
import { newDocument, createDesignerHistory, applyDesignerCommand, validateDesignerDocument } from '../../packages/companion-designer/src/index.ts';
import { firmwareWidgetPresets, simplifyFirmwareWidgets } from './firmware-widgets.ts';
test('palette matches implemented ESP32 widget kinds',()=>{
 assert.deepEqual(firmwareWidgetPresets.map(p=>p.kind),['button','indicator','text','numeric','gauge']);
});
test('unsupported widgets and bindings are removed without mutating saved content',()=>{
 let history=createDesignerHistory(newDocument('Panel'));
 const pageId=history.present.ui.pages[0].id;
 for(const kind of ['button','xy','selector','hat','encoder','slider','icon','numeric'])history=applyDesignerCommand(history,{type:'add-widget',kind,pageId,axisMode:'bipolar'});
 const before=structuredClone(history.present);
 const removedControls=before.presentation.widgets.filter(w=>!['button','numeric'].includes(w.kind)).flatMap(w=>w.controlIds);
 const cleaned=simplifyFirmwareWidgets(before);
 assert.equal(cleaned.removed,6);
 assert.deepEqual(cleaned.document.presentation.widgets.map(w=>w.kind),['button','button','numeric']);
 assert.ok(cleaned.document.ui.controls.every(c=>!removedControls.includes(c.id)));
 assert.ok(cleaned.document.bindings.every(b=>!removedControls.includes(b.controlId)));
 assert.equal(validateDesignerDocument(cleaned.document).ok,true);
 assert.deepEqual(before,history.present);
 assert.equal(simplifyFirmwareWidgets(cleaned.document).removed,0);
});
