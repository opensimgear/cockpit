#!/usr/bin/env node
// Authored software fixture: real catalogue names/interfaces, synthetic addresses.
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
const controls=Object.values(await document('A-10C')).flatMap(Object.values);
const lookup=id=>controls.find(control=>control.identifier===id);
const output=id=>lookup(id)?.outputs?.[0];
const caution=output('MASTER_CAUTION');
const aircraftField=find(await document('MetadataStart'),'_ACFT_NAME');
const counterField=find(await document('MetadataEnd'),'_UPDATE_COUNTER');
if (!caution||!aircraftField||!counterField) throw new Error('Incomplete peer catalogue.');
const commands=createSocket('udp4'), exports=createSocket('udp4');
let aircraft='A-10C',warning=false,paused=false,fragment=false,frozen=false,counter=0,framesSent=0;
let frequency=Buffer.from('251.000'),preset=Buffer.from('01'),volume=32768,presetSelector=0,mhz=5,squelch=0,testPressed=0;
let flooding=false,halfFrame=false,heldFrame=null,malformed=false,frameSequence=0,commandSequence=0,timer;
let intervalMs=Number(options['interval-ms']);
const history=[];
const block=(address,data)=>{const header=Buffer.alloc(4);header.writeUInt16LE(address);header.writeUInt16LE(data.length,2);return Buffer.concat([header,data]);};
const integer=(output,value)=>{const data=Buffer.alloc(2);data.writeUInt16LE((value<<output.shift_by)&output.mask);return block(output.address,data);};
const string=(field,value)=>{const data=Buffer.alloc((field.max_length+1)&~1);value.copy(data,0,0,field.max_length);return block(field.address,data);};
const send=bytes=>exports.send(bytes,Number(options['export-port']),options['export-host']);
function payload() {
  if(!frozen)counter=(counter+1)&255;
  frameSequence++;
  if(flooding) {
    frequency=Buffer.from(`${220+(frameSequence%30)}.${String(frameSequence%1000).padStart(3,'0')}`);
    preset=Buffer.from(String(frameSequence%20+1).padStart(2,'0'));
    volume=(frameSequence*997)&65535;
  }
  const name=Buffer.alloc(aircraftField.max_length);name.write(aircraft,'ascii');
  const fields=[Buffer.from([0x55,0x55,0x55,0x55]),block(aircraftField.address,name),integer(caution,warning?1:0)];
  for(const [id,value] of [['UHF_FREQUENCY',frequency],['UHF_PRESET',preset]])if(output(id))fields.push(string(output(id),value));
  for(const [id,value] of [['UHF_VOL',volume],['UHF_PRESET_SEL',presetSelector],['UHF_10MHZ_SEL',mhz],['UHF_SQUELCH',squelch],['UHF_TEST',testPressed]])if(output(id))fields.push(integer(output(id),value));
  fields.push(integer(counterField,counter));return Buffer.concat(fields);
}
function transmit(bytes) {
  if(fragment){send(bytes.subarray(0,3));send(bytes.subarray(3,13));send(bytes.subarray(13));}else send(bytes);
}
function frame() {
  if(paused||heldFrame)return;
  if(malformed){send(Buffer.from([0x55,0x55,0x55,0x55,1,0,3,0,1,2,3]));framesSent++;return;}
  const bytes=payload();
  if(halfFrame){heldFrame=bytes;transmit(bytes.subarray(0,bytes.length-6));}else transmit(bytes);
  framesSent++;
}
function setTimer(){clearInterval(timer);timer=setInterval(frame,intervalMs);}
function parseCommand(line) {
  const match=/^([A-Z0-9_]+) (\S+)$/.exec(line);if(!match)return null;
  const [,name,argument]=match,control=lookup(name);if(!control)return null;
  const interfaces=control.inputs??[];
  if(['INC','DEC'].includes(argument)&&interfaces.some(i=>i.interface==='fixed_step'))return {name,argument,delta:argument==='INC'?1:-1};
  const variable=interfaces.find(i=>i.interface==='variable_step');
  if(/^[+-]\d+$/.test(argument)&&variable&&Math.abs(Number(argument))<=variable.max_value&&Number(argument)!==0)return {name,argument,delta:Number(argument)};
  const state=interfaces.find(i=>i.interface==='set_state');
  if(/^\d+$/.test(argument)&&state&&Number(argument)<=state.max_value)return {name,argument,value:Number(argument)};
  return null;
}
commands.on('message',bytes=>{
  if(bytes.length>2048)return;
  const lines=bytes.toString('utf8').split('\n').filter(Boolean);
  for(const line of lines){
    const parsed=parseCommand(line);if(!parsed)continue;
    history.push({line,aircraft,receivedAtMs:Date.now(),sequence:++commandSequence,counter});if(history.length>256)history.shift();
    const {name,value,delta}=parsed;
    if(name==='UFC_MASTER_CAUTION'&&value===1&&(aircraft==='A-10C'||aircraft==='A-10C_2'))warning=false;
    if(name==='UHF_TEST')testPressed=value;
    if(name==='UHF_SQUELCH')squelch=value;
    if(name==='UHF_VOL')volume=Math.max(0,Math.min(65535,value??volume+delta));
    if(name==='UHF_PRESET_SEL'){presetSelector=Math.max(0,Math.min(20,value??presetSelector+delta));preset=Buffer.from(String(Math.min(20,presetSelector+1)).padStart(2,'0'));}
    if(name==='UHF_10MHZ_SEL'){mhz=Math.max(0,Math.min(9,value??mhz+delta));frequency=Buffer.from(`${200+mhz*10}.000`);}
  }
});
commands.bind(Number(options['command-port']),'127.0.0.1');
setTimer();
const input=createInterface({input:process.stdin});
input.on('line',line=>{
  const match=/^sim (\d+) (\S+)(?: (.*))?$/.exec(line);if(!match)return;
  const [,id,command,arg]=match;let ok=true;
  if(command==='warning'&&/^[01]$/.test(arg??''))warning=arg==='1';
  else if(command==='pause'&&/^[01]$/.test(arg??''))paused=arg==='1';
  else if(command==='fragment'&&/^[01]$/.test(arg??''))fragment=arg==='1';
  else if(command==='freeze'&&/^[01]$/.test(arg??''))frozen=arg==='1';
  else if(command==='flood'&&/^[01]$/.test(arg??''))flooding=arg==='1';
  else if(command==='half_frame'&&/^[01]$/.test(arg??'')){halfFrame=arg==='1';if(!halfFrame)heldFrame=null;}
  else if(command==='complete'&&heldFrame){send(heldFrame.subarray(heldFrame.length-6));heldFrame=null;}
  else if(command==='rate'&&/^\d+$/.test(arg??'')&&Number(arg)>=1&&Number(arg)<=1000){intervalMs=1000/Number(arg);setTimer();}
  else if(command==='coalesce'&&/^\d+$/.test(arg??'')&&Number(arg)>=1&&Number(arg)<=32){send(Buffer.concat(Array.from({length:Number(arg)},payload)));framesSent+=Number(arg);}
  else if(command==='aircraft'&&arg!==undefined&&arg.length<=23&&!/[\x00-\x1f\x7f]/.test(arg))aircraft=arg==='NONE'?'':arg;
  else if(command==='frequency'&&arg!==undefined&&Buffer.byteLength(arg)<=7&&!/[\x00-\x1f\x7f]/.test(arg))frequency=Buffer.from(arg);
  else if(command==='frequency_hex'&&/^(?:[0-9a-fA-F]{2}){1,7}$/.test(arg??''))frequency=Buffer.from(arg,'hex');
  else if(command==='preset'&&arg!==undefined&&Buffer.byteLength(arg)<=2&&!/[\x00-\x1f\x7f]/.test(arg))preset=Buffer.from(arg);
  else if(command==='volume'&&/^\d+$/.test(arg??'')&&Number(arg)<=65535)volume=Number(arg);
  else if(command==='malformed'&&arg===undefined)send(Buffer.from([0x55,0x55,0x55,0x55,1,0,3,0,1,2,3]));
  else if(command==='malformed'&&/^[01]$/.test(arg??''))malformed=arg==='1';
  else if(command!=='inspect')ok=false;
  process.stdout.write(`${JSON.stringify({id:Number(id),ok,error:ok?null:'invalid_request',simulation:{aircraft,warning,paused,fragment,frozen,counter,framesSent,commandCount:commandSequence,commands:history,frequency:frequency.toString('utf8'),preset:preset.toString('utf8'),volume,presetSelector,mhz,squelch,testPressed,flooding,halfFrame,holdingFrame:heldFrame!==null,intervalMs}})}\n`);
});
function stop(){clearInterval(timer);commands.close();exports.close();input.close();}
process.once('SIGTERM',stop);process.once('SIGINT',stop);
