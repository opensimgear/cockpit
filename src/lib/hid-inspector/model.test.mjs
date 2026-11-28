import test from 'node:test';
import assert from 'node:assert/strict';
import {loadModules} from '../test-module-loader.mjs';
const m=(await loadModules(['./hid-inspector/model'])).get('./hid-inspector/model');
const value=(n,min='0',max='65535')=>({fieldId:'descriptor:0',elementIndex:0,semantics:'absolute',sample:{state:'valid',kind:'integer',value:n},logical:{minimum:min,maximum:max}});

test('native absolute domains retain unsigned, asymmetric and wide integers exactly',()=>{
  assert.equal(m.valuePresentation(value('32768')).percent,50);
  assert.equal(m.valuePresentation(value('0','-100','300')).percent,25);
  const wide=m.valuePresentation(value('18446744073709551615','0','18446744073709551615'));
  assert.equal(wide.text,'18446744073709551615');assert.equal(wide.percent,100);
  assert.equal(m.valuePresentation(value('999','0','100')).percent,null);
  assert.equal(m.valuePresentation(value('1','1','1')).percent,null);
  assert.equal(m.valuePresentation({...value('0'),semantics:'relative'}).kind,'Relative delta');
  assert.equal(m.valuePresentation({...value('0'),semantics:'event'}).kind,'Event');
});
test('hats, usages, null, invalid, unknown, bytes and button zero remain distinct',()=>{
  assert.equal(m.valuePresentation({...value('0'),usages:[{page:1,id:57}]}).kind,'Hat');
  assert.equal(m.valuePresentation({...value('0'),usagePage:9}).active,false);
  assert.equal(m.valuePresentation({...value('1'),usagePage:9}).active,true);
  for(const quality of ['null','invalid','unknown']){
    const p=m.valuePresentation({...value('0'),sample:{state:quality,encoded:'0'}});
    assert.equal(p.quality,quality);assert.equal(p.percent,null);assert.equal(p.text,'0');
  }
  assert.equal(m.valuePresentation({sample:{Bytes:'00ff'}}).kind,'Opaque bytes');
  assert.equal(m.valuePresentation({sample:{Array:{selector:'3',usages:[{page:7,id:3}]}}}).kind,'Usage selectors');
  assert.equal(m.sampleOf({sample:{Unknown:{reason:'no layout'}}}).reason,'no layout');
  assert.equal(m.sampleOf({sample:{Null:'5'}}).state,'null');
  assert.equal(m.sampleOf({sample:{Invalid:'7'}}).state,'invalid');
  assert.equal(m.sampleOf({sample:{Integer:'42'}}).value,'42');
  assert.equal(m.sampleOf({sample:{}}).state,'unknown');
});
test('descriptor, HidP and IOHID fields retain collection/report identity and searchable exact metadata',()=>{
  const detail={descriptor:{fields:[{collection:2,kind:'input',globals:{report_id:1,logical_min:'0',logical_max:'4095'},primary:{usages:[{page:1,id:48}]}}]},
    capabilities:{reports:[{kind:'feature',valueCaps:[{reportId:2,linkCollection:3,usagePage:1,usage:57}],buttonCaps:[{reportId:2,linkCollection:4}]}]},
    elements:[{cookie:22,parentCookie:1,reportId:3,reportType:'input',usagePage:9,usage:1}]};
  const fields=m.fieldsOf({detail});assert.equal(fields.length,4);assert.equal(fields[0].collection,'2');assert.equal(fields[1].kind,'feature');assert.equal(fields[3].id,'iohid-cookie:22');
  assert.equal(m.valuePresentation(value('1000'),fields[0]).min,'0');
  assert.ok(m.descriptorTree(detail,'logical_max').some(n=>n.value==='4095'));
  assert.equal(m.descriptorTree(detail,'missing').length,0);
  assert.equal(m.descriptorTree({big:Array.from({length:1000},(_,i)=>i)}).length,512);
  assert.equal(m.fieldsOf(null).length,0);assert.equal(m.exact(null),'unavailable');
});
test('filters are capability scoped; raw window remains bounded and preserves terminal/loss markers',()=>{
  const field={id:'descriptor:0',kind:'input',collection:'2',reportId:1,usage:{page:9,id:1},metadata:{}};
  const row={...value('1'),reportId:1,usagePage:9};
  const filter=m.emptyFilters(),changed=new Set(['descriptor:0:0']);assert.equal(m.matches(field,row,filter,changed),true);
  for(const [key,pass,fail] of [['kind','input','feature'],['report','1','2'],['collection','2','3'],['usage','descriptor','missing']]){
    assert.equal(m.matches(field,row,{...filter,[key]:pass},changed),true);assert.equal(m.matches(field,row,{...filter,[key]:fail},changed),false);
  }
  assert.equal(m.matches(field,row,{...filter,changed:true},changed),true);
  assert.equal(m.matches(field,row,{...filter,changed:true},new Set()),false);
  assert.equal(m.matches(field,row,{...filter,active:true},changed),true);
  const report={sequence:'1',receivedUs:'2',wireHex:'00'},terminal={kind:'terminal',sequence:'1',receivedUs:'2'},gap={kind:'gap',fromSequence:'2',receivedUs:'3'};
  assert.equal(m.appendReports([report],[report,terminal,gap]).length,3);
  assert.equal(m.appendReports([],Array.from({length:1000},(_,i)=>({sequence:String(i)}))).length,256);
  assert.equal(m.reportValues([{sequence:'1',samples:{values:[{field:0,element:1,semantics:'Relative',sample:{Integer:'-1'}}]}}])[0].fieldId,'descriptor:0');
  const windows=m.reportValues([{windowsReportId:2,samples:{data:[{dataIndex:3,rawValue:'4294967295',values:[{capabilityIndex:0,quality:'valid',semantics:'relative',native:'-1'}]}]}}])[0];
  assert.equal(m.valuePresentation(windows).kind,'Relative delta');assert.equal(m.valuePresentation(windows).text,'-1');
  const mac=m.reportValues([{samples:{elements:[{cookie:22,integer:'-7',relative:true}]}}])[0];
  assert.equal(m.valuePresentation(mac).text,'-7');
  assert.equal(m.valuePresentation(value('0'),{id:'flags',metadata:{bitSize:1}}).kind,'Bitfield');
});
