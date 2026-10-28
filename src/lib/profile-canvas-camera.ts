import type { CanvasViewport } from './profile-canvas';

export type CanvasCameraOptions = {
  request:(callback:(time:number)=>void)=>number;
  cancel:(id:number)=>void;
  now:()=>number;
  reducedMotion?:()=>boolean;
  activity?:(active:boolean)=>void;
  duration?:number;
};

function capture(view:CanvasViewport):CanvasViewport {
  if(![view.x,view.y,view.zoom].every(Number.isFinite)||view.zoom<=0)
    throw new Error('Use finite canvas coordinates and a positive zoom.');
  return{x:view.x,y:view.y,zoom:view.zoom};
}

/** One camera owns the rendered viewport, including while navigation is in flight. */
export function createCanvasCamera(read:()=>CanvasViewport,write:(view:CanvasViewport)=>void,options:CanvasCameraOptions) {
  const duration=options.duration??240;
  if(!Number.isFinite(duration)||duration<0)throw new Error('Use a finite nonnegative camera duration.');
  let frame:number|null=null,epoch=0,destroyed=false,target:CanvasViewport|null=null;
  let finish:((completed:boolean)=>void)|null=null;

  function stop(completed:boolean):void {
    epoch++;
    if(frame!==null)options.cancel(frame);
    frame=null;
    target=null;
    const resolve=finish;
    finish=null;
    if(resolve){options.activity?.(false);resolve(completed);}
  }

  return {
    get destination():CanvasViewport|null{return target?{...target}:null;},
    move(next:CanvasViewport,animate=true):Promise<boolean> {
      if(destroyed)return Promise.resolve(false);
      const destination=capture(next),start=capture(read());
      stop(false);
      const equal=start.x===destination.x&&start.y===destination.y&&start.zoom===destination.zoom;
      if(equal||!animate||!duration||options.reducedMotion?.()){
        if(!equal)write({...destination});
        return Promise.resolve(true);
      }
      target=destination;
      const activeEpoch=epoch,started=options.now();
      const completion=new Promise<boolean>(resolve=>{finish=resolve;});
      options.activity?.(true);
      function tick(time:number):void {
        if(destroyed||activeEpoch!==epoch)return;
        frame=null;
        const progress=Math.min(1,Math.max(0,(time-started)/duration));
        const eased=progress<.5?4*progress**3:1-(-2*progress+2)**3/2;
        write(progress===1?{...destination}:{
          x:start.x*(1-eased)+destination.x*eased,
          y:start.y*(1-eased)+destination.y*eased,
          zoom:start.zoom*(1-eased)+destination.zoom*eased,
        });
        // A viewport writer may itself cancel or replace navigation.
        if(destroyed||activeEpoch!==epoch)return;
        if(progress===1)stop(true);
        else frame=options.request(tick);
      }
      frame=options.request(tick);
      return completion;
    },
    cancel():void{stop(false);},
    destroy():void{destroyed=true;stop(false);},
  };
}
