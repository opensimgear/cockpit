import type {CompileDiagnostic,ProfileCompilerClient} from './profile-compilation';
import type {DeepReadonly,ProfileDocument,ProfileLocalAssignment} from './profiles';
import {profileVehicleDocument} from './profile-vehicle-tabs';

const relevantCodes=new Set(['duplicatePhysicalSource','duplicatePhysicalInputRoutes','conflictingEndpointWriters','virtualDirectSimulatorConflict','virtualOutputFeedback','graphBudgetExceeded','invalidGraph','invalidVehicleTabs','virtualTargetLimit','inputUnbound','sourceResourceLimit','invalidExecutionProgram','missingPort','bindingContract','invalidOperation','ambiguousWriters','internalCycle','unknownLocalAssignment','unknownLocalPortExpectation']);
export function profileVehicleConflict(diagnostic:DeepReadonly<CompileDiagnostic>):boolean{return diagnostic.severity==='error'&&relevantCodes.has(diagnostic.code);}
export type VehicleConflictState={status:'idle'|'pending'|'checking'|'ready'|'error';diagnostics:readonly DeepReadonly<CompileDiagnostic>[];error:string};
type Schedule=(callback:()=>void,delay:number)=>()=>void;
const initial=():VehicleConflictState=>({status:'idle',diagnostics:[],error:''});
/** Read-only, debounced native previews. Editing, tab changes and hiding fence every late result. */
export class ProfileVehicleConflictController{
  private value=initial();private epoch=0;private signature='';private cancelTimer:(()=>void)|undefined;private disposed=false;
  private listeners=new Set<(state:DeepReadonly<VehicleConflictState>)=>void>();
  constructor(private compiler:Pick<ProfileCompilerClient,'compile'|'inspect'>,private schedule:Schedule=(callback,delay)=>{const timer=setTimeout(callback,delay);return()=>clearTimeout(timer);}){}
  get state():DeepReadonly<VehicleConflictState>{return Object.freeze(this.value);}
  subscribe(listener:(state:DeepReadonly<VehicleConflictState>)=>void):()=>void{this.listeners.add(listener);listener(this.state);return()=>this.listeners.delete(listener);}
  private set(value:VehicleConflictState):void{if(this.disposed)return;this.value=value;for(const listener of this.listeners)listener(this.state);}
  update(document:DeepReadonly<ProfileDocument>,assignments:readonly DeepReadonly<ProfileLocalAssignment>[],tabId='',inventoryRevision=0,active=true):void{
    if(this.disposed)return;
    if(!active){this.clear();return;}
    try{
      // The unified canvas checks every execution in one native compilation.
      // Explicit scope previews remain useful to callers inspecting one vehicle.
      const variant=tabId?profileVehicleDocument(document,tabId):JSON.parse(JSON.stringify(document)) as ProfileDocument,resources=new Set(variant.resources.map(resource=>resource.id));
      // An unnamed new profile is still a valid graph to inspect while typing.
      if(!variant.name.trim())variant.name='Profile draft';
      const pins=JSON.parse(JSON.stringify(assignments.filter(assignment=>resources.has(assignment.resourceId)))) as ProfileLocalAssignment[];
      const signature=JSON.stringify({document:{...variant,editor:undefined},pins,tabId,inventoryRevision});
      if(signature===this.signature)return;
      this.signature=signature;const epoch=++this.epoch;this.cancelTimer?.();this.set({status:'pending',diagnostics:[],error:''});
      this.cancelTimer=this.schedule(()=>{this.cancelTimer=undefined;void this.compile(variant,pins,epoch);},350);
    }catch(error){this.signature='';this.epoch++;this.cancelTimer?.();this.cancelTimer=undefined;this.set({status:'error',diagnostics:[],error:error instanceof Error?error.message:String(error)});}
  }
  private async compile(document:ProfileDocument,assignments:ProfileLocalAssignment[],epoch:number):Promise<void>{
    if(epoch!==this.epoch||this.disposed)return;
    this.set({status:'checking',diagnostics:[],error:''});
    try{
      const preview=await this.compiler.compile(document,assignments);if(epoch!==this.epoch||this.disposed)return;
      const diagnostics=[...preview.diagnostics];
      // Conflicts can follow unavailable-resource warnings beyond the summary's first page.
      while(diagnostics.length<preview.sectionTotals.diagnostics){
        const page=await this.compiler.inspect(preview,'diagnostics',diagnostics.length,64);if(epoch!==this.epoch||this.disposed)return;
        if(!page.items.length)throw Error('The conflict diagnostic page was incomplete.');
        diagnostics.push(...page.items);
      }
      const invalidCapture=diagnostics.find(diagnostic=>diagnostic.code==='invalidCapture');
      if(invalidCapture)throw Error(invalidCapture.message);
      const captured=JSON.parse(JSON.stringify(diagnostics.filter(profileVehicleConflict))) as CompileDiagnostic[];
      for(const diagnostic of captured){Object.freeze(diagnostic.location.edgeIds);Object.freeze(diagnostic.location);Object.freeze(diagnostic);}
      this.set({status:'ready',diagnostics:Object.freeze(captured),error:''});
    }catch(error){if(epoch===this.epoch&&!this.disposed)this.set({status:'error',diagnostics:[],error:error instanceof Error?error.message:String(error)});}
  }
  clear():void{this.epoch++;this.signature='';this.cancelTimer?.();this.cancelTimer=undefined;this.set(initial());}
  dispose():void{this.clear();this.disposed=true;this.listeners.clear();}
}
