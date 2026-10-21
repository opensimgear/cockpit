import type { Bounds, PortableUi, Style } from "../../companion-contracts/src/index.ts";
import type { WidgetDefinition, WidgetInstance } from "./widget-model.ts";
export type { Bounds, PortableUi, Style };
export const DESIGNER_LIMITS = Object.freeze({ history: 50, drafts: 20, widgets: 192, groups: 64, viewportRegions: 32, targets: 8, assetPixels: 16777216, totalAssetPixels: 33554432, assetDimension: 8192, bundleBytes: 13 * 1024 * 1024 });
export type WidgetKind = "button" | "selector" | "hat" | "encoder" | "slider" | "xy" | "indicator" | "text" | "numeric" | "icon" | "gauge" | "image" | "component";
interface WidgetBase {
  id: string; pageId: string; label: string; bounds: Bounds; controlIds: string[];
  feedbackIds: string[]; groupId?: string; style?: Style;
}
export type DesignerWidget =
  | (WidgetBase & { kind: "component"; config: WidgetInstance })
  | (WidgetBase & { kind: "image"; config: { assetId?: string } })
  | (WidgetBase & { kind: "button"; config: { buttonMode: "momentary" | "toggle" | "pulse" | "repeat"; pulseMs: number; repeatMs: number; iconAssetId?: string } })
  | (WidgetBase & { kind: "selector"; config: { options: { label: string; value: number }[] } })
  | (WidgetBase & { kind: "hat"; config: Record<string, never> })
  | (WidgetBase & { kind: "encoder"; config: { pulseMs: number; repeatMs: number } })
  | (WidgetBase & { kind: "slider"; config: { centering: "spring" | "latched"; deadzone: number; inverted: boolean; orientation: "horizontal" | "vertical" } })
  | (WidgetBase & { kind: "xy"; config: { centering: "spring" | "latched"; deadzone: number; invertX: boolean; invertY: boolean } })
  | (WidgetBase & { kind: "indicator"; config: Record<string, never> })
  | (WidgetBase & { kind: "text"; config: { prefix: string; suffix: string } })
  | (WidgetBase & { kind: "numeric" | "gauge"; config: { min: number; max: number; unit: string; precision: number } })
  | (WidgetBase & { kind: "icon"; config: { symbol: "power" | "warning" | "gear" | "check"; iconAssetId?: string } });
export interface DesignerGroup { id: string; pageId: string; label: string }
export interface ViewportRegion { id: string; label: string; bounds: Bounds }
export interface Viewport { pageId: string; width: number; height: number; safeInsets?: { top: number; right: number; bottom: number; left: number }; backgroundAssetId?: string; regions?: ViewportRegion[] }
export interface LogicalTarget { id: string; label: string; axes: ("unipolar" | "bipolar")[]; buttons: number; hats: number }
export interface LogicalBinding { controlId: string; targetId: string; kind: "axis" | "button" | "hat"; index: number }
export interface DesignerDocument {
  schemaVersion: 2 | 3; ui: PortableUi;
  presentation: { widgets: DesignerWidget[]; groups: DesignerGroup[]; viewports: Viewport[]; grid: { columns: number; rows: number; snap: boolean }; definitions?: WidgetDefinition[] };
  targets: LogicalTarget[]; bindings: LogicalBinding[];
}
export interface AssetPayload { assetId: string; base64: string }
export interface PortableBundle { bundleVersion: 1; document: DesignerDocument; assets: AssetPayload[] }
export interface DesignerHistory { past: DesignerDocument[]; present: DesignerDocument; future: DesignerDocument[] }
export interface DraftEntry { id: string; name: string; document: DesignerDocument; assets: AssetPayload[] }
export interface DesignerLibrary { schemaVersion: 1; drafts: DraftEntry[] }
export interface Publication { document: DesignerDocument; assets: AssetPayload[]; publicationHash: string; capabilityFingerprint: string }

export type PublishedRevision = Publication;
