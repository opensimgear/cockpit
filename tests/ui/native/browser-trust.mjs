// Test-session trust is scoped to the server key verified against Cockpit's CA.
import {createHash,X509Certificate} from 'node:crypto';
import {isIP} from 'node:net';
import {connect as tlsConnect} from 'node:tls';
export async function verifiedServerKey(url,certificate){
 const address=new URL(url);if(address.protocol!=='https:')throw Error('Expected the native HTTPS browser endpoint.');
 return new Promise((resolve,reject)=>{
  const socket=tlsConnect({host:address.hostname,port:Number(address.port||443),...(isIP(address.hostname)?{}:{servername:address.hostname}),ca:certificate,rejectUnauthorized:true});
  socket.setTimeout(5000,()=>socket.destroy(Error('Cockpit certificate verification timed out')));socket.once('error',reject);
  socket.once('secureConnect',()=>{try{const peer=new X509Certificate(socket.getPeerCertificate().raw);resolve(createHash('sha256').update(peer.publicKey.export({format:'der',type:'spki'})).digest('base64'));}catch(error){reject(error);}finally{socket.end();}});
 });
}
export function redactDiagnostic(value){return String(value).replace(/(#pair=)[A-Za-z0-9_-]+/g,'$1[redacted]').replace(/("(?:secret|token|bearerToken|authorization)"\s*:\s*")[^"]+("?)/gi,'$1[redacted]$2');}
