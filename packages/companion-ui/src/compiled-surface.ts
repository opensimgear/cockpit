import {createCompiledPixelDecoder,verifyCompiledBundle,type CompiledWidgetBundle} from '../../widget-runtime/src/artifact.ts';
import {CompiledWidgetPlayer} from '../../widget-runtime/src/player.ts';
import {bundleForWidget,type CompiledPanelPublication} from '../../widget-runtime/src/panel.ts';

export interface PreparedCompiledSurface {
  artifactHash:string;canvas:HTMLCanvasElement;player:CompiledWidgetPlayer;
  pixels:ReturnType<typeof createCompiledPixelDecoder>;frameId:string;dispose():void;
}
export type PreparedCompiledSurfaces=Record<string,PreparedCompiledSurface>;
interface PreparationOptions {signal?:AbortSignal;nowMs?:number;createCanvas?:()=>HTMLCanvasElement}
function abort(signal?:AbortSignal):void{if(signal?.aborted)throw new DOMException('Compiled surface preparation cancelled','AbortError');}

/** Allocate and paint the actual candidate surface before replacing any active DOM. */
export async function prepareCompiledSurface(bundle:CompiledWidgetBundle,options:PreparationOptions={}):Promise<PreparedCompiledSurface>{
  abort(options.signal);const checked=await verifyCompiledBundle(bundle);abort(options.signal);
  const pixels=createCompiledPixelDecoder(checked.artifact,checked.assets);let canvas:HTMLCanvasElement|undefined;
  try{
    const player=new CompiledWidgetPlayer(checked,options.nowMs??Math.floor(performance.now()));
    canvas=(options.createCanvas??(()=>document.createElement('canvas')))();canvas.width=checked.artifact.width;canvas.height=checked.artifact.height;
    const context=canvas.getContext('2d');if(!context)throw new Error('A pixel canvas is unavailable.');
    context.putImageData(new ImageData(pixels.decode(player.frameId),checked.artifact.width,checked.artifact.height),0,0);abort(options.signal);
    let disposed=false;const surface=canvas;
    return {artifactHash:checked.artifactHash,canvas:surface,player,pixels,frameId:player.frameId,dispose(){if(disposed)return;disposed=true;pixels.dispose();surface.width=surface.height=0;}};
  }catch(error){pixels.dispose();if(canvas)canvas.width=canvas.height=0;throw error;}
}
export function disposeCompiledSurfaces(surfaces:PreparedCompiledSurfaces|undefined):void{if(surfaces)for(const surface of Object.values(surfaces))surface.dispose();}
/** All instances/pages reserve their admitted surfaces; partial candidates roll back. */
export async function prepareCompiledSurfaces(publication:CompiledPanelPublication,options:PreparationOptions={}):Promise<PreparedCompiledSurfaces>{
  const surfaces:PreparedCompiledSurfaces={};
  try{for(const widget of publication.document.presentation.widgets){abort(options.signal);surfaces[widget.id]=await prepareCompiledSurface(bundleForWidget(publication,widget.id),options);}return surfaces;}
  catch(error){disposeCompiledSurfaces(surfaces);throw error;}
}
