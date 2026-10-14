#!/usr/bin/env node
// A local DCS-BIOS wire peer for development; the catalogue uses synthetic addresses.
import { createSocket } from 'node:dgram';
import { readFile } from 'node:fs/promises';
import { createInterface } from 'node:readline';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const options = { 'command-port': '7778', 'export-port': '5010', 'export-host': '127.0.0.1', 'interval-ms': '33', catalogue: resolve(dirname(fileURLToPath(import.meta.url)), 'catalogue') };
for (let i=2;i<process.argv.length;i+=2) {
  const key=process.argv[i]?.slice(2);
  if (!(key in options) || !process.argv[i+1]) throw new Error('Use --command-port, --export-port, --export-host, --interval-ms or --catalogue.');
  options[key]=process.argv[i+1];
}
for (const key of ['command-port','export-port','interval-ms']) {
  const value=Number(options[key]);
  if (!Number.isInteger(value)||value<1||value>(key==='interval-ms'?1000:65535)) throw new Error(`Invalid ${key}`);
}
const document=async name=>JSON.parse(await readFile(resolve(options.catalogue,`${name}.json`),'utf8'));
const find=(data,id)=>Object.values(data).flatMap(Object.values).find(control=>control.identifier===id)?.outputs?.[0];
const caution=find(await document('A-10C'),'MASTER_CAUTION');
const aircraftField=find(await document('MetadataStart'),'_ACFT_NAME');
const counterField=find(await document('MetadataEnd'),'_UPDATE_COUNTER');
if (!caution||!aircraftField||!counterField) throw new Error('Incomplete peer catalogue.');
const commands=createSocket('udp4'), exports=createSocket('udp4');
let aircraft='A-10C',warning=false,paused=false,fragment=false,frozen=false,counter=0,framesSent=0;
const history=[];
const block=(address,data)=>{const header=Buffer.alloc(4);header.writeUInt16LE(address);header.writeUInt16LE(data.length,2);return Buffer.concat([header,data]);};
const integer=(output,value)=>{const data=Buffer.alloc(2);data.writeUInt16LE((value<<output.shift_by)&output.mask);return block(output.address,data);};
const send=bytes=>exports.send(bytes,Number(options['export-port']),options['export-host']);
function frame() {
  if(paused)return;
  if(!frozen)counter=(counter+1)&255;
  const name=Buffer.alloc(aircraftField.max_length);name.write(aircraft,'ascii');
  const bytes=Buffer.concat([Buffer.from([0x55,0x55,0x55,0x55]),block(aircraftField.address,name),integer(caution,warning?1:0),integer(counterField,counter)]);
  if(fragment){send(bytes.subarray(0,3));send(bytes.subarray(3,13));send(bytes.subarray(13));}else send(bytes);
  framesSent++;
}
commands.on('message',bytes=>{
  if(bytes.length>128)return;
  const lines=bytes.toString('utf8').split('\n').filter(Boolean);
  for(const line of lines){
    const match=/^UFC_MASTER_CAUTION ([01])$/.exec(line);if(!match)continue;
    history.push({line,aircraft});if(history.length>256)history.shift();
    if(match[1]==='1'&&(aircraft==='A-10C'||aircraft==='A-10C_2'))warning=false;
  }
});
commands.bind(Number(options['command-port']),'127.0.0.1');
const timer=setInterval(frame,Number(options['interval-ms']));
const input=createInterface({input:process.stdin});
input.on('line',line=>{
  const match=/^sim (\d+) (.*)$/.exec(line);if(!match)return;
  const [command,arg]=match[2].split(' ');let ok=true;
  if(command==='warning'&&/^[01]$/.test(arg??''))warning=arg==='1';
  else if(command==='pause'&&/^[01]$/.test(arg??''))paused=arg==='1';
  else if(command==='fragment'&&/^[01]$/.test(arg??''))fragment=arg==='1';
  else if(command==='freeze'&&/^[01]$/.test(arg??''))frozen=arg==='1';
  else if(command==='aircraft'&&arg!==undefined&&arg.length<=23)aircraft=arg==='NONE'?'':arg;
  else if(command==='malformed')send(Buffer.from([0x55,0x55,0x55,0x55,1,0,3,0,1,2,3]));
  else if(command!=='inspect')ok=false;
  process.stdout.write(`${JSON.stringify({id:Number(match[1]),ok,error:ok?null:'invalid_request',simulation:{aircraft,warning,paused,fragment,counter,framesSent,commands:history}})}\n`);
});
function stop(){clearInterval(timer);commands.close();exports.close();input.close();}
process.once('SIGTERM',stop);process.once('SIGINT',stop);
