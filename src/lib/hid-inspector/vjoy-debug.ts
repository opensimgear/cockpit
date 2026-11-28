export type VjoyDebugCommand = 'controller_vjoy_debug_inspect' | 'controller_vjoy_debug_open'
  | 'controller_vjoy_debug_read' | 'controller_vjoy_debug_close' | 'controller_vjoy_debug_replay'
  | 'controller_vjoy_debug_record_start' | 'controller_vjoy_debug_record_stop' | 'controller_vjoy_debug_export';
export type VjoyRecording = {
  format:'osg-vjoy-ffb';slot:number;
  records:{sequence:string;command:number;wireHex:string}[];
  capabilities?:unknown;reason?:string;startingSequence?:string;
};
export type VjoyDebugResult = {
  slot?:number;generation?:string;capabilities?:Record<string,unknown>;
  records?:{sequence:string;command:number;wireHex:string;event:unknown}[];
  effects?:unknown;oldest?:string;latest?:string;next?:string;gap?:unknown;hasMore?:boolean;status?:string;error?:unknown;
  recording?:Record<string,unknown>|VjoyRecording;
};
