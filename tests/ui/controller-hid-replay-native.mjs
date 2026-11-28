import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import test from 'node:test';
import {nativePage, until} from '../../scripts/reactive/cdp.mjs';

const manifest=fileURLToPath(new URL('../../crates/hid-report/Cargo.toml',import.meta.url));
const corpus=name=>JSON.parse(readFileSync(new URL(`../hid-contract/authored/${name}.json`,import.meta.url),'utf8'));

function archive(fixture, sample) {
  const input={descriptorHex:fixture.descriptorHex,reports:[{
    kind:sample.kind,wireHex:sample.wireHex,framing:sample.reportId==null?'unnumbered':'id-prefix',
  }]};
  const decoded=JSON.parse(execFileSync('cargo',['run','--quiet','--locked','--manifest-path',manifest,'--example','qualify'],{
    input:JSON.stringify(input),encoding:'utf8',
  }));
  const interfaceId=`authored-${fixture.id}`;
  const descriptor={interfaceId,product:fixture.id,provenance:'authored portable descriptor',
    descriptorHex:fixture.descriptorHex,descriptor:decoded.descriptor};
  const reason={kind:'stopped'};
  const records=[{sequence:'1',receivedUs:'100',wireHex:sample.wireHex,reportId:sample.reportId,samples:decoded.reports[0]},
    {kind:'terminal',sequence:'1',receivedUs:'200',reason}];
  const value={format:'osg-hid-recording',descriptor,interfaceId,generation:'authored-1',
    startedUnixMs:'0',startedReceivedUs:'0',startingSequence:'0',status:'stopped',
    reason,lastSequence:'1',recordCount:records.length,records,bytes:0};
  while(value.bytes!==Buffer.byteLength(JSON.stringify(value)))value.bytes=Buffer.byteLength(JSON.stringify(value));
  return value;
}

const centered={id:'unsigned-centered',descriptorHex:'05010904a1010930150026ffff751095018102c0',
  samples:[{kind:'input',reportId:null,wireHex:'0080'}]};

function scenarios() {
  const normal=[
    [centered,'32768'],[corpus('hat-domains'),'65000'],[corpus('packed-joystick'),'Null'],
    [corpus('relative-mouse'),'-127'],[corpus('boot-keyboard'),'40'],[corpus('vendor-wide'),'18446744073709551615'],
  ];
  const archives=normal.map(([fixture,expected])=>[archive(fixture,fixture.samples[0]),expected]);
  archives.push([archive(corpus('hat-domains'),{kind:'input',wireHex:'fe01',reportId:254}),'Unknown report kind/ID layout']);
  return archives;
}

async function click(page, label) {
  await until(()=>page.evaluate(`(() => {
    const button=[...document.querySelectorAll('button')].find(b=>(b.textContent.trim()===${JSON.stringify(label)}||b.getAttribute('aria-label')===${JSON.stringify(label)})&&!b.disabled&&b.getClientRects().length);
    if(button)button.click();
    return !!button;
  })()`));
}

test('running Debug renders authored native HID domains through offline file replay',{
  skip:process.env.OSG_HID_NATIVE!=='1',timeout:180000,
},async()=>{
  const page=await nativePage();
  try {
    await click(page,'Debug');
    await click(page,'HID interfaces');
    for(const [value,expected] of scenarios()) {
      const text=JSON.stringify(value);
      await until(()=>page.evaluate(`(() => {
        const input=document.querySelector('input[type=file][accept*="json"]');
        if(!input||input.disabled)return false;
        const transfer=new DataTransfer();
        transfer.items.add(new File([${JSON.stringify(text)}], 'authored.hid.json',{type:'application/json'}));
        input.files=transfer.files;
        input.dispatchEvent(new Event('change',{bubbles:true}));
        return true;
      })()`));
      // Wait for this archive's identity before stepping, even when two archives use the same descriptor.
      await until(()=>page.evaluate(`document.body.innerText.includes(${JSON.stringify(value.interfaceId)})`));
      await click(page,'Step');
      await until(()=>page.evaluate(`(() => {
        const delivery=document.querySelector('[aria-label="Decoded HID monitor"] .reports details');
        if(!delivery)return false;
        if(!delivery.open)delivery.querySelector('summary').click();
        return delivery.open;
      })()`));
      await until(()=>page.evaluate(`document.querySelector('[aria-label="Decoded HID monitor"]').innerText.toLowerCase().includes(${JSON.stringify(expected.toLowerCase())})`));
      if(value.records[0].samples.values.length){
        const cards=await page.evaluate(`[...document.querySelectorAll('[aria-label="Decoded HID monitor"] article')].map(card=>card.innerText).join(' ')`);
        assert.ok(cards.toLowerCase().includes(expected.toLowerCase()), `${value.interfaceId}: ${cards}`);
      }
      assert.ok(await page.evaluate(`document.querySelector('[aria-label="Decoded HID monitor"]').innerText.toLowerCase().includes(${JSON.stringify(value.records[0].wireHex)})`),
        `wire bytes for ${value.interfaceId}`);
      await click(page,'Close replay');
    }
  }finally { page.close(); }
});
