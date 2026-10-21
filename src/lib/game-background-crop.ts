import type {GameCrop} from './games';
export const TILE_ASPECT=16/9;
export const MAX_CROP_ZOOM=8;
const clamp=(value:number,min:number,max:number)=>Math.max(min,Math.min(max,value));
export function maximumCrop(width:number,height:number):GameCrop {
  if(!Number.isFinite(width)||!Number.isFinite(height)||width<=0||height<=0)throw new Error('The image has no usable dimensions.');
  const w=Math.min(1,TILE_ASPECT*height/width),h=w*width/height/TILE_ASPECT;
  return{x:(1-w)/2,y:(1-h)/2,width:w,height:h};
}
export function boundedCrop(crop:GameCrop,width:number,height:number):GameCrop {
  const max=maximumCrop(width,height);
  if(!Object.values(crop).every(Number.isFinite))return max;
  const w=clamp(crop.width,max.width/MAX_CROP_ZOOM,max.width),h=w*width/height/TILE_ASPECT;
  return{x:clamp(crop.x,0,1-w),y:clamp(crop.y,0,1-h),width:w,height:h};
}
export function positionedCrop(width:number,height:number,x=50,y=50):GameCrop {
  const max=maximumCrop(width,height);
  return{...max,x:(1-max.width)*clamp(x,0,100)/100,y:(1-max.height)*clamp(y,0,100)/100};
}
export function moveCrop(crop:GameCrop,dx:number,dy:number,width:number,height:number):GameCrop {
  return boundedCrop({...crop,x:crop.x+dx,y:crop.y+dy},width,height);
}
export function zoomCrop(crop:GameCrop,zoom:number,width:number,height:number):GameCrop {
  const max=maximumCrop(width,height),w=max.width/clamp(zoom,1,MAX_CROP_ZOOM),h=w*width/height/TILE_ASPECT;
  return boundedCrop({x:crop.x+(crop.width-w)/2,y:crop.y+(crop.height-h)/2,width:w,height:h},width,height);
}
export function resizeCrop(crop:GameCrop,dx:number,dy:number,width:number,height:number):GameCrop {
  const max=maximumCrop(width,height),ratio=width/height/TILE_ASPECT;
  const limit=Math.min(max.width,1-crop.x,(1-crop.y)/ratio);
  const w=clamp(crop.width+(dx+dy*ratio)/(1+ratio*ratio),max.width/MAX_CROP_ZOOM,limit);
  return{x:crop.x,y:crop.y,width:w,height:w*ratio};
}
/** The source image retains its own aspect ratio; only its viewport is cropped. */
export function cropImageStyle(crop?:GameCrop):string {
  return crop?`position:absolute;width:${100/crop.width}%;height:auto;max-width:none;left:${-100*crop.x/crop.width}%;top:${-100*crop.y/crop.height}%;`:'';
}
