// Optional passive UI check against the actual native development Cockpit.
import {nativePage,until} from '../../scripts/reactive/cdp.mjs';
import {writeFile,mkdir} from 'node:fs/promises';
const page=await nativePage();
try {
  await page.evaluate(`(()=>{
    const button=[...document.querySelectorAll('nav[aria-label="Main navigation"] button')]
      .find(button=>button.textContent.trim().startsWith('Devices'));
    if(!button)throw Error('Devices navigation missing');button.click();
  })()`);
  const text=await until(()=>page.evaluate('document.body.innerText'),text=>text.includes('VKB'));
  const matches=text.split('\n').filter(line=>/VKB|T-Rudder|Gladiator/.test(line));
  const result={at:new Date().toISOString(),title:await page.evaluate('document.title'),
    native:true,view:'Devices',matchedProductLabels:matches,
    verification:'Both attached physical controller product labels appear in actual Devices UI.',
    noInputInjection:true,noOutputWrites:true};
  if(!matches.some(x=>x.includes('T-Rudder'))||!matches.some(x=>x.includes('Gladiator')))
    throw Error('Both physical products must appear in Devices UI');
  await mkdir(new URL('.generated/',import.meta.url),{recursive:true});
  await writeFile(new URL('.generated/live-ui.json',import.meta.url),JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify(result));
} finally {page.close();}
