// Actual desktop settings and listener: exported root and visible Brave setup.
import test from 'node:test';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {createHash,X509Certificate} from 'node:crypto';
import {connect} from 'node:tls';
import {mkdir} from 'node:fs/promises';

const require=createRequire(import.meta.url),module=process.env.OSG_PLAYWRIGHT_MODULE??'playwright';
const {chromium}=require(module),{expect}=require(module+'/test');
// WebView2 may ignore CDP pointer input; dispatch through the actual UI control.
async function click(control){await expect(control).toBeEnabled();await control.evaluate(element=>element.click());}

function verifiedLeaf(url,ca){
  const address=new URL(url);
  return new Promise((resolve,reject)=>{
    const socket=connect({host:address.hostname,port:Number(address.port||443),ca,rejectUnauthorized:true,
      ...(address.hostname==='localhost'?{servername:address.hostname}:{})});
    socket.setTimeout(5000,()=>socket.destroy(Error('Listener TLS verification timed out')));
    socket.once('error',reject);
    socket.once('secureConnect',()=>{try{assert.equal(socket.authorized,true);resolve(new X509Certificate(socket.getPeerCertificate().raw));}catch(error){reject(error);}finally{socket.end();}});
  });
}

test('public root CA verifies the live listener and Brave instructions are visible', {timeout:60000},async()=>{
  const browser=await chromium.connectOverCDP(process.env.OSG_NATIVE_CDP_URL??'http://127.0.0.1:9222');
  try{
    const page=browser.contexts().flatMap(context=>context.pages()).find(page=>page.url().startsWith('http://127.0.0.1:1420/'));
    assert.ok(page,'Start the actual native development Cockpit with CDP enabled.');
    await page.reload();
    const native=async command=>{
      const reply=await page.evaluate(command=>window.__TAURI_INTERNALS__.invoke('gateway_request',{request:{command,clientId:'certificate-native-acceptance'}}),command);
      assert.equal(reply.ok,true,reply.error);return reply.state.companions;
    };
    const initial=await native('companion_status');
    assert.equal(initial.listener.enabled,true,'Enable the live companion listener before this acceptance test.');
    const exported=(await native('companion_browser_certificate')).certificatePem,ca=new X509Certificate(exported);
    assert.equal(ca.ca,true);assert.equal(ca.subject,ca.issuer);assert.equal(ca.verify(ca.publicKey),true);
    assert.match(ca.subject,/CN=OpenSimGear Companion CA /);assert.doesNotMatch(exported,/PRIVATE KEY/);
    const pin='sha256:'+createHash('sha256').update(ca.publicKey.export({type:'spki',format:'der'})).digest('hex');
    assert.equal(pin,initial.listener.caSpkiSha256);
    const before=await verifiedLeaf(initial.browserUrl,exported);assert.equal(before.ca,false);
    assert.equal(before.verify(ca.publicKey),true);assert.notDeepEqual(before.raw,ca.raw);
    await click(page.getByRole('button',{name:'Settings',exact:true}));
    await click(page.getByRole('tab',{name:'Companions',exact:true}));
    const settings=page.getByRole('region',{name:'Companion listener settings'});
    await expect(settings.getByRole('button',{name:'Export public certificate',exact:true})).toBeEnabled();
    const instructions=settings.locator('details.browser-install');
    if(await instructions.getAttribute('open')===null)await click(instructions.locator('summary'));
    await expect(instructions).toContainText('opensimgear-cockpit-root-ca.crt');
    await expect(instructions).toContainText('Current User');
    await expect(instructions).toContainText('Trusted Root Certification Authorities');
    await expect(instructions).toContainText('Completely quit and reopen Brave');
    await mkdir('.research/companion-certificate',{recursive:true});
    await settings.screenshot({path:'.research/companion-certificate/settings.png'});
    console.log(JSON.stringify({rootSubject:ca.subject,rootVerified:true,braveInstructionsVisible:true}));
  }finally{await browser.close();}
});
