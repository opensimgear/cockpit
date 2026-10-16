import { NONE, OP, PROPERTY, SECTION } from "./abi.ts";
import { inspectBinaryObject, records, section, view } from "./binary.ts";
import { evaluate, formatFixed, Q, toFixed } from "./fixed.ts";
import type { Instruction, LookupPoint } from "./fixed.ts";
import { verifyPackage } from "./package.ts";
import type { DeviceProfile, PackedPackage, Rect } from "./types.ts";
import { assert } from "./validation.ts";

export interface ReferenceFeedback { type: "boolean" | "number" | "text"; available: boolean; fresh: boolean; value?: boolean | number | string }
export interface ReferenceState { page: number; connected: boolean; timeMs: number; feedback: ReferenceFeedback[]; pressed: boolean[] }
export interface NodeProperties { visible: boolean; x: number; y: number; color: number; image: number; number: number | null }
export function blend565(source: number, destination: number, alpha: number): number {
  const blend = (s: number, d: number) => Math.floor((s * alpha + d * (15 - alpha) + 7) / 15);
  return (blend(source >> 11, destination >> 11) << 11) | (blend((source >> 5) & 63, (destination >> 5) & 63) << 5) | blend(source & 31, destination & 31);
}
function intersect(a: Rect, b: Rect): Rect { const x = Math.max(a[0],b[0]), y = Math.max(a[1],b[1]); return [x,y,Math.max(0,Math.min(a[0]+a[2],b[0]+b[2])-x),Math.max(0,Math.min(a[1]+a[3],b[1]+b[3])-y)]; }
function rectangle(node: DataView, offset: number): Rect { return [node.getInt16(offset,true),node.getInt16(offset+2,true),node.getUint16(offset+4,true),node.getUint16(offset+6,true)]; }
export function rgb565Bytes(frame: Uint16Array): Uint8Array { const bytes=new Uint8Array(frame.length*2),data=view(bytes);for(let i=0;i<frame.length;++i)data.setUint16(i*2,frame[i],true);return bytes; }
/** Independent host reference. It deliberately reconstructs complete frames to check C dirty-tile output. */
export async function createReferenceRenderer(input: PackedPackage, profileInput: DeviceProfile) {
  const profile=structuredClone(profileInput),packed=await verifyPackage(input.manifestBytes,input.objects,profile,input.packageHash);
  const scene=inspectBinaryObject("scene",packed.objects.get("scene")!),semantics=inspectBinaryObject("semantics",packed.objects.get("semantics")!);
  const pages=records(scene,1),nodes=records(scene,2),variables=records(scene,3),bindings=records(scene,4),timelines=records(scene,6),keys=records(scene,7),hits=records(scene,8),points=records(scene,9);
  const expressions:Instruction[]=records(scene,5).map(data=>({op:data.getUint8(0),operand:data.getUint16(2,true),value:data.getInt32(4,true)}));
  const lookup:LookupPoint[]=records(scene,13).map(data=>[data.getInt32(0,true),data.getInt32(4,true)]);
  const imageObject=packed.objects.has("images")?inspectBinaryObject("images",packed.objects.get("images")!):undefined;
  const fontObject=packed.objects.has("fonts")?inspectBinaryObject("fonts",packed.objects.get("fonts")!):undefined;
  const images=imageObject?records(imageObject,1):[],pixels=imageObject?section(imageObject,2).data:new Uint8Array();
  const fonts=fontObject?records(fontObject,1):[],glyphs=fontObject?records(fontObject,2):[],fontPixels=fontObject?section(fontObject,3).data:new Uint8Array();
  const textBytes=section(scene,10).data,decoder=new TextDecoder("utf-8",{fatal:true,ignoreBOM:true});
  let previous:ReferenceState|undefined,startMs=0,lastProperties:NodeProperties[]=[];
  function alpha(bytes:Uint8Array,stride:number,offset:number,x:number,y:number):number { const value=bytes[offset+y*stride+(x>>1)];return x%2?value&15:value>>4; }
  function update(stateInput:ReferenceState):{frame:Uint16Array;properties:NodeProperties[];elapsedMs:number}{
    const state=structuredClone(stateInput);assert(Number.isSafeInteger(state.timeMs)&&state.timeMs>=0&&state.page>=0&&state.page<pages.length,"Invalid reference state");
    assert(!previous||state.timeMs>=previous.timeMs,"Reference clock must be monotonic");
    if(!previous||state.page!==previous.page||state.connected&&!previous.connected)startMs=state.timeMs;
    const elapsedMs=state.connected?state.timeMs-startMs:0;
    const values:(number|null)[]=variables.map(variable=>{
      const ref=variable.getUint16(0,true),source=variable.getUint8(3),kind=variable.getUint8(2),feedback=state.feedback[ref];
      if(source===1)return state.connected&&state.pressed[ref]?Q:0;
      if(source===4)return state.connected?Q:0;
      if(source===2||source===3)return state.connected&&feedback?.available&&(source===2||feedback.fresh)?Q:0;
      if(!state.connected||!feedback?.available)return null;
      try{
        let value:number;
        if(kind===1){if(feedback.type!=="boolean"||typeof feedback.value!=="boolean")return null;value=feedback.value?Q:0;}
        else {if(kind!==2||feedback.type!=="number"||typeof feedback.value!=="number")return null;value=toFixed(feedback.value);}
        return value>=variable.getInt32(8,true)&&value<=variable.getInt32(12,true)?value:null;
      }catch{return null;}
    });
    const properties:NodeProperties[]=nodes.map(node=>({visible:!!(node.getUint8(3)&1),x:0,y:0,color:node.getUint16(36,true),image:node.getUint16(28,true),number:null}));
    const forcedHidden=new Set<number>();
    function apply(index:number,property:number,value:number|null,min=-2147483648,max=2147483647):void{
      if(value===null||value<min||value>max){forcedHidden.add(index);return;}
      const p=properties[index];
      if(property===PROPERTY.visible){if(value!==0&&value!==Q)forcedHidden.add(index);else p.visible=value===Q;}
      else if(property===PROPERTY.translateX)p.x=Math.trunc(value/Q);
      else if(property===PROPERTY.translateY)p.y=Math.trunc(value/Q);
      else if(property===PROPERTY.color)p.color=value;
      else if(property===PROPERTY.image)p.image=value;
      else p.number=value;
    }
    for(const binding of bindings){const first=binding.getUint16(4,true),count=binding.getUint16(6,true);apply(binding.getUint16(0,true),binding.getUint8(2),evaluate(expressions.slice(first,first+count),values,lookup),binding.getInt32(8,true),binding.getInt32(12,true));}
    for(const timeline of timelines){
      const first=timeline.getUint16(4,true),count=timeline.getUint16(6,true),duration=timeline.getUint16(8,true),time=timeline.getUint16(10,true)&1?elapsedMs%duration:Math.min(elapsedMs,duration);
      let value=keys[first].getInt32(4,true);
      for(let i=1;i<count;++i){const before=keys[first+i-1],after=keys[first+i],t0=before.getUint16(0,true),t1=after.getUint16(0,true);
        if(time>=t1)value=after.getInt32(4,true);
        else {if(timeline.getUint8(3))value=before.getInt32(4,true)+Number(BigInt(after.getInt32(4,true)-before.getInt32(4,true))*BigInt(time-t0)/BigInt(t1-t0));break;}
      }
      apply(timeline.getUint16(0,true),timeline.getUint8(2),value);
    }
    for(const index of forcedHidden)properties[index].visible=false;
    const frame=new Uint16Array(profile.width*profile.height),[cx,cy,cw,ch]=profile.content,page=pages[state.page];
    for(let y=cy;y<cy+ch;++y)frame.fill(page.getUint16(10,true),y*profile.width+cx,y*profile.width+cx+cw);
    function pixel(x:number,y:number,color:number,coverage:number,clip:Rect):void {if(x<clip[0]||y<clip[1]||x>=clip[0]+clip[2]||y>=clip[1]+clip[3]||coverage===0)return;const index=y*profile.width+x;frame[index]=coverage===15?color:blend565(color,frame[index],coverage);}
    const first=page.getUint16(6,true),count=page.getUint16(8,true);
    for(let i=first;i<first+count;++i){
      const node=nodes[i],p=properties[i];if(!p.visible)continue;
      const op=node.getUint8(2),base=rectangle(node,4),bounds:Rect=[base[0]+p.x,base[1]+p.y,base[2],base[3]],clip=intersect(intersect(bounds,rectangle(node,12)),profile.content);
      if(!clip[2]||!clip[3])continue;
      if(op===OP.rect){for(let y=clip[1];y<clip[1]+clip[3];++y)frame.fill(p.color,y*profile.width+clip[0],y*profile.width+clip[0]+clip[2]);}
      else if(op===OP.image565||op===OP.mask4||op===OP.image565Mask4){
        const image=images[p.image],mask=images[node.getUint16(30,true)],sx=node.getUint16(52,true),sy=node.getUint16(54,true);
        for(let y=clip[1];y<clip[1]+clip[3];++y)for(let x=clip[0];x<clip[0]+clip[2];++x){
          const sourceX=sx+x-bounds[0],sourceY=sy+y-bounds[1];
          const color=op===OP.mask4?p.color:view(pixels).getUint16(image.getUint32(8,true)+sourceY*image.getUint32(4,true)+sourceX*2,true);
          const coverage=op===OP.image565?15:alpha(pixels,mask.getUint32(4,true),mask.getUint32(8,true),sourceX,sourceY);
          pixel(x,y,color,coverage,clip);
        }
      }else if(op===OP.text){
        const font=fonts[node.getUint16(32,true)],text=p.number===null?decoder.decode(textBytes.subarray(node.getUint32(44,true),node.getUint32(44,true)+node.getUint16(48,true))):formatFixed(p.number,node.getUint16(42,true));
        let advance=0;
        for(const character of text){
          const codepoint=character.codePointAt(0)!,firstGlyph=font.getUint16(0,true),glyphCount=font.getUint16(2,true);
          let glyph=glyphs[font.getUint16(4,true)];for(let j=firstGlyph;j<firstGlyph+glyphCount;++j)if(glyphs[j].getUint32(0,true)===codepoint){glyph=glyphs[j];break;}
          const width=glyph.getUint16(4,true),height=glyph.getUint16(6,true),x=bounds[0]+advance+glyph.getInt16(8,true),y=bounds[1]+font.getInt16(8,true)+glyph.getInt16(10,true),stride=Math.ceil(width/2),offset=glyph.getUint32(16,true);
          for(let gy=0;gy<height;++gy)for(let gx=0;gx<width;++gx)pixel(x+gx,y+gy,p.color,alpha(fontPixels,stride,offset,gx,gy),clip);
          advance+=glyph.getInt16(12,true);
        }
      }else{
        const firstPoint=node.getUint16(38,true),pointCount=node.getUint16(40,true),width=node.getUint16(34,true),bias=-Math.floor(width/2);
        for(let j=1;j<pointCount;++j){
          let x0=points[firstPoint+j-1].getInt16(0,true)+p.x,y0=points[firstPoint+j-1].getInt16(2,true)+p.y;
          const x1=points[firstPoint+j].getInt16(0,true)+p.x,y1=points[firstPoint+j].getInt16(2,true)+p.y,dx=Math.abs(x1-x0),sx=x0<x1?1:-1,dy=-Math.abs(y1-y0),sy=y0<y1?1:-1;
          let error=dx+dy;
          for(;;){for(let py=0;py<width;++py)for(let px=0;px<width;++px)pixel(x0+bias+px,y0+bias+py,p.color,15,clip);if(x0===x1&&y0===y1)break;const twice=2*error;if(twice>=dy){error+=dy;x0+=sx;}if(twice<=dx){error+=dx;y0+=sy;}}
        }
      }
    }
    previous=state;lastProperties=properties;return{frame,properties:structuredClone(properties),elapsedMs};
  }
  function hitTest(x:number,y:number):number|null{
    if(!previous?.connected)return null;
    for(let i=hits.length-1;i>=0;--i){const hit=hits[i],nodeIndex=hit.getUint16(12,true),p=lastProperties[nodeIndex];if(hit.getUint16(2,true)!==previous.page||!p.visible)continue;
      const region=intersect(intersect(rectangle(hit,4),rectangle(nodes[nodeIndex],12)),profile.content);
      if(x>=region[0]&&y>=region[1]&&x<region[0]+region[2]&&y<region[1]+region[3])return hit.getUint16(0,true);
    }return null;
  }
  return {update,hitTest,semantics};
}
