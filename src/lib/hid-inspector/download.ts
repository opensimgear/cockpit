export const MAX_EXPORT_BYTES=64*1024*1024;
const alphabet='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
/** Native export chunks can end midway through a UTF-8 code point. Keep them as bytes. */
export function exportChunk(value:Record<string,unknown>,offset:number,total:number|null){
  const {chunk,nextOffset,totalBytes}=value;
  if(value.offset!==offset||!Number.isSafeInteger(totalBytes)||Number(totalBytes)<1||Number(totalBytes)>MAX_EXPORT_BYTES
    ||total!==null&&totalBytes!==total||!Number.isSafeInteger(nextOffset)||Number(nextOffset)<=offset||Number(nextOffset)>Number(totalBytes)
    ||typeof chunk!=='string'||chunk.length>43692||! /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(chunk))throw Error('Invalid or oversized HID export chunk.');
  const padding=chunk.endsWith('==')?2:chunk.endsWith('=')?1:0;
  const bytes=new Uint8Array(chunk.length/4*3-padding);
  if(bytes.length!==Number(nextOffset)-offset||bytes.length>32768)throw Error('HID export byte boundaries do not match the payload.');
  for(let index=0,out=0;index<chunk.length;index+=4){
    const bits=(alphabet.indexOf(chunk[index])<<18)|(alphabet.indexOf(chunk[index+1])<<12)|((alphabet.indexOf(chunk[index+2])&63)<<6)|(alphabet.indexOf(chunk[index+3])&63);
    for(const shift of [16,8,0])if(out<bytes.length)bytes[out++]=(bits>>shift)&255;
  }
  return {bytes,next:Number(nextOffset),total:Number(totalBytes)};
}
