export function tinySource(){return {formatVersion:2,id:'fixture',revision:1,title:'Fixture',size:{width:32,height:32},parameters:[],behavior:{version:1,signals:[],events:[],regions:[]},visualDomains:[],inputs:[],outputs:[],hitRegions:[],scene:[{id:'surface',kind:'rectangle',x:0,y:0,width:32,height:32,fill:'#ff0000'}],motions:[],sounds:[],assets:[]};}
export function clone(value){return structuredClone(value);}
export function wave(){const bytes=new Uint8Array(48),view=new DataView(bytes.buffer);for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)bytes[at+i]=text.charCodeAt(i);view.setUint32(4,40,true);view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,8000,true);view.setUint32(28,16000,true);view.setUint16(32,2,true);view.setUint16(34,16,true);view.setUint32(40,4,true);return bytes;}
import {deflateSync} from 'node:zlib';
export function rasterPng(){
  const crc=bytes=>{let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let bit=0;bit<8;bit++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return (crc^0xffffffff)>>>0;};
  const chunk=(type,data)=>{const out=Buffer.alloc(data.length+12);out.writeUInt32BE(data.length);out.write(type,4);Buffer.from(data).copy(out,8);out.writeUInt32BE(crc(out.subarray(4,out.length-4)),out.length-4);return out;};
  const header=Buffer.alloc(13);header.writeUInt32BE(1,0);header.writeUInt32BE(1,4);header[8]=8;header[9]=6;return Uint8Array.from(Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(Uint8Array.from([0,255,0,0,255]))),chunk('IEND',new Uint8Array())]));
}
