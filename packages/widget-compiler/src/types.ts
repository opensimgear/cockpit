import type { BehaviorDefinition, Expression, Scalar } from '../../widget-runtime/src/behavior.ts';
import type { CompiledWidgetBundle, WidgetCapabilities } from '../../widget-runtime/src/artifact.ts';

export type VisualValue<T extends Scalar = Scalar> = T | Expression;
export interface VisualTransform {
  translateX?: VisualValue<number>; translateY?: VisualValue<number>;
  rotate?: VisualValue<number>; scaleX?: VisualValue<number>; scaleY?: VisualValue<number>;
  originX?: VisualValue<number>; originY?: VisualValue<number>;
}
export interface VisualBase {
  id: string; label?: string; visible?: VisualValue<boolean>; opacity?: VisualValue<number>;
  fill?: VisualValue<string>; stroke?: VisualValue<string>; strokeWidth?: VisualValue<number>;
  transform?: VisualTransform;
}
export type VisualNode =
  | (VisualBase & { kind: 'group'; children: VisualNode[] })
  | (VisualBase & { kind: 'rectangle'; x: VisualValue<number>; y: VisualValue<number>; width: VisualValue<number>; height: VisualValue<number>; radius?: VisualValue<number> })
  | (VisualBase & { kind: 'ellipse'; x: VisualValue<number>; y: VisualValue<number>; width: VisualValue<number>; height: VisualValue<number> })
  | (VisualBase & { kind: 'line'; x: VisualValue<number>; y: VisualValue<number>; x2: VisualValue<number>; y2: VisualValue<number> })
  | (VisualBase & { kind: 'path'; d: VisualValue<string> })
  | (VisualBase & { kind: 'text'; x: VisualValue<number>; y: VisualValue<number>; text: VisualValue<string>; font: 'sans' | 'mono'; fontSize: VisualValue<number>; align?: 'left' | 'center' | 'right' })
  | (VisualBase & { kind: 'image'; x: VisualValue<number>; y: VisualValue<number>; width: VisualValue<number>; height: VisualValue<number>; assetId: string });
export type WidgetParameter =
  | { id: string; label: string; type: 'text' | 'color'; default: string }
  | { id: string; label: string; type: 'boolean'; default: boolean }
  | { id: string; label: string; type: 'number'; default: number; min: number; max: number };
export interface VisualDomain {
  signal: string; values?: Scalar[]; min?: number; max?: number; samples?: number;
  /** Author accepts this conservative geometric error in native target pixels. */
  maxErrorPixels?: number;
  /** Maximum displacement over the whole numeric range, in source pixels. */
  travelPixels?: number;
  smoothingMs?: number;
}
export interface WidgetPort {
  id: string; label: string; type: 'boolean' | 'number' | 'enum';
  signal: string; min?: number; max?: number; values?: string[];
}
export interface WidgetHitRegion {
  id: string; x: number; y: number; width: number; height: number;
  events: Partial<Record<'press' | 'release' | 'activate' | 'cancel', string>>;
}
export interface MotionTrack {
  node: string; property: string;
  keyframes: { at: number; value: number }[];
  easing?: 'linear' | 'ease-in' | 'ease-out' | 'ease-in-out';
}
export interface WidgetMotion {
  id: string; from: Record<string, Scalar>; to: Record<string, Scalar>;
  fromRegions?: Record<string, string>; toRegions?: Record<string, string>;
  durationMs: number; fps: number; tracks: MotionTrack[];
  interruption?: 'reverse' | 'restart' | 'snap';
}
export interface WidgetSound {
  id: string; cue: string; assetId: string; cooldownMs?: number;
  stopOnExit?: boolean; volume?: number;
}
export interface WidgetSourceAsset {
  id: string; mime: 'image/png' | 'image/jpeg' | 'image/webp' | 'audio/wav'; base64: string;
}
export interface WidgetSource {
  id: string; title: string;
  size: { width: number; height: number }; parameters: WidgetParameter[];
  behavior: BehaviorDefinition; visualDomains: VisualDomain[];
  inputs: WidgetPort[]; outputs: WidgetPort[]; hitRegions: WidgetHitRegion[];
  scene: VisualNode[]; motions?: WidgetMotion[]; sounds?: WidgetSound[];
  assets?: WidgetSourceAsset[];
  /** Optional authoring graph. Runtime behavior/scene are produced by lowering. */
  design?: WidgetDesign;
}
export type SignalDomain = {type:'boolean'} | {type:'number';min:number;max:number} | {type:'enum';values:string[]};
/** References to other nodes are typed value wires; event wires are transitions. */
export interface SignalNode { id:string; label:string; domain:SignalDomain; expression:Expression }
export interface StateOwner { region:string; signal:string; values:Record<string,Scalar>; port?:string }
export interface StateAppearance { id:string; regions:Record<string,string>; layers:Record<string,Record<string,VisualValue>> }
export interface TransitionEffect {
  region:string; transition:string; durationMs:number; fps:number;
  easing?:MotionTrack['easing']; interruption?:WidgetMotion['interruption'];
  properties:{node:string;property:string}[]; cue?:string;
}
export interface WidgetDesign {
  nodes:SignalNode[]; owners:StateOwner[]; appearances:StateAppearance[]; effects:TransitionEffect[];
}
export interface CompileOptions {
  parameters?: Record<string, Scalar>; width?: number; height?: number;
  capabilities?: WidgetCapabilities; motion?: boolean; audio?: boolean;
  signal?: AbortSignal; onProgress?: (progress: { completed: number; total: number; phase: string }) => void;
}
export type CompileResult = CompiledWidgetBundle;
