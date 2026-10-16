import { publishDocument } from "../../companion-designer/src/assets.ts";
import type { Publication } from "../../companion-designer/src/types.ts";
import { EXPR, INTERACTION, NONE, OP, PROPERTY, SECTION } from "./abi.ts";
import { packFonts, packImages } from "./assets.ts";
import type { PreparedFont, PreparedImage } from "./assets.ts";
import { inspectBinaryObject, packBinaryObject, view } from "./binary.ts";
import { inspectContent } from "./content.ts";
import { expressionRange, expressionStackDepth, Q, toFixed } from "./fixed.ts";
import type { Instruction, LookupPoint } from "./fixed.ts";
import { packPackage } from "./package.ts";
import type { DeviceProfile, ObjectKind, PackageSource, PackedPackage, Rect } from "./types.ts";
import { assert, canonical, digest, id, uint } from "./validation.ts";

export interface SemanticControl { id: string; kind: "button" | "unipolar" | "bipolar"; targetId: string; targetIndex: number; fallback: number }
export interface SemanticOutput { id: string; channelId: string; kind: "boolean" | "number" | "text"; min: number; max: number; unit: string; maxTextBytes: number }
export interface SemanticInteraction { id: string; kind: "momentary" | "navigation"; controlIds: string[]; destinationPage?: string }
export interface SemanticDescriptor { version: 1; controls: SemanticControl[]; outputs: SemanticOutput[]; interactions: SemanticInteraction[] }
export type VariableRecipe =
  | { id: string; source: "feedback" | "available" | "fresh"; outputId: string }
  | { id: string; source: "pressed"; interactionId: string }
  | { id: string; source: "connected" };
export type Expression =
  | { kind: "constant"; value: number }
  | { kind: "variable"; id: string }
  | { kind: "lookup"; value: Expression; points: LookupPoint[] }
  | { kind: "op"; op: Exclude<keyof typeof EXPR, "constant" | "variable" | "lookup">; args: Expression[] };
export const scalar = (value: number): Expression => ({ kind: "constant", value: toFixed(value) });
export const raw = (value: number): Expression => ({ kind: "constant", value });
export const variable = (idValue: string): Expression => ({ kind: "variable", id: idValue });
export type PropertyName = keyof typeof PROPERTY;
export interface NodeRecipe {
  id: string; kind: keyof typeof OP; bounds: Rect; clip?: Rect; visible?: boolean; color?: number;
  imageId?: string; maskId?: string; fontId?: string; text?: string; maxTextBytes?: number; precision?: number;
  source?: readonly [number, number]; points?: readonly (readonly [number, number])[]; lineWidth?: number;
  bindings?: Partial<Record<PropertyName, Expression>>;
  timelines?: { property: PropertyName; interpolation: "step" | "linear"; repeat: boolean; keyframes: readonly (readonly [number, number])[] }[];
}
export interface ResolvedRecipe {
  recipeVersion: 1; id: string; images: PreparedImage[]; fonts: PreparedFont[]; variables: VariableRecipe[];
  pages: { id: string; background: number; nodes: NodeRecipe[] }[];
  hits: { interactionId: string; nodeId: string; bounds?: Rect }[];
}
export interface CompiledPanel extends PackedPackage { semantics: SemanticDescriptor; recipeHash: string }

type Field = readonly [number, "u8" | "u16" | "i16" | "u32" | "i32", number];
function record(size: number, fields: readonly Field[]): Uint8Array {
  const bytes = new Uint8Array(size), data = view(bytes);
  for (const [offset, type, value] of fields) {
    const signed = type[0] === "i", bits = Number(type.slice(1));
    assert(Number.isInteger(value) && value >= (signed ? -(2 ** (bits - 1)) : 0) && value <= (signed ? 2 ** (bits - 1) - 1 : 2 ** bits - 1), "Compiled integer outside " + type + " bounds");
    if (type === "u8") data.setUint8(offset, value); else if (type === "u16") data.setUint16(offset, value, true); else if (type === "i16") data.setInt16(offset, value, true); else if (type === "u32") data.setUint32(offset, value, true); else data.setInt32(offset, value, true);
  }
  return bytes;
}
function concat(rows: Uint8Array[]): Uint8Array { const bytes = new Uint8Array(rows.reduce((sum, row) => sum + row.length, 0)); let offset = 0; for (const row of rows) { bytes.set(row, offset); offset += row.length; } return bytes; }
class Strings {
  chunks: Uint8Array[] = []; offsets = new Map<string, readonly [number, number]>(); length = 0;
  add(value: string): readonly [number, number] {
    assert(typeof value === "string" && !/[\u0000-\u001f\u007f]/.test(value), "Invalid compiled string");
    const existing = this.offsets.get(value); if (existing) return existing;
    const bytes = new TextEncoder().encode(value), ref = [this.length, bytes.length] as const; this.chunks.push(bytes); this.length += bytes.length; this.offsets.set(value, ref); return ref;
  }
  bytes(): Uint8Array { return concat(this.chunks); }
}
function stringFields(strings: Strings, value: string, offset = 0, lengthOffset = 4): Field[] { const ref = strings.add(value); return [[offset, "u32", ref[0]], [lengthOffset, "u16", ref[1]]]; }
function geometry(rect: Rect, offset: number): Field[] { assert(Array.isArray(rect) && rect.length === 4, "Expected physical rectangle"); return [[offset, "i16", rect[0]], [offset + 2, "i16", rect[1]], [offset + 4, "u16", rect[2]], [offset + 6, "u16", rect[3]]]; }
function uniqueIndex(values: { id: string }[]): Map<string, number> {
  const result = new Map<string, number>(); for (const [index, value] of values.entries()) { id(value.id); assert(!result.has(value.id), "Duplicate compiled ID"); result.set(value.id, index); } return result;
}
function indexOf(indexes: Map<string, number>, value: string | undefined, name: string): number { assert(value !== undefined && indexes.has(value), "Unknown " + name + " reference: " + value); return indexes.get(value)!; }

function packSemantics(semantics: SemanticDescriptor, pages: Map<string, number>): Uint8Array {
  assert(semantics.version === 1, "Unsupported prepared semantics version");
  const strings = new Strings(), controls = uniqueIndex(semantics.controls), owners = uniqueIndex(semantics.interactions); uniqueIndex(semantics.outputs);
  const controlRows = semantics.controls.map(control => {
    const owner = semantics.interactions.findIndex(interaction => interaction.controlIds.includes(control.id));
    assert(owner >= 0, "Unowned prepared control"); const kind = control.kind === "button" ? 1 : control.kind === "unipolar" ? 2 : 3;
    return record(32, [...stringFields(strings, control.id), [6, "u8", kind], ...stringFields(strings, control.targetId, 8, 12), [14, "u16", control.targetIndex], [16, "i32", kind === 3 ? -Q : 0], [20, "i32", Q], [24, "i32", toFixed(control.fallback)], [28, "u16", owner], [30, "u16", NONE]]);
  });
  const outputRows = semantics.outputs.map(output => record(32, [...stringFields(strings, output.id), [6, "u8", output.kind === "boolean" ? 1 : output.kind === "number" ? 2 : 3], ...stringFields(strings, output.channelId, 8, 12), [14, "u16", output.maxTextBytes], [16, "i32", toFixed(output.min)], [20, "i32", toFixed(output.max)], ...stringFields(strings, output.unit, 24, 28)]));
  const interactionRows = semantics.interactions.map(interaction => {
    assert(interaction.kind === "momentary" || interaction.kind === "navigation", "Unsupported prepared interaction");
    assert(interaction.controlIds.length === (interaction.kind === "momentary" ? 1 : 0), "Prepared interaction arity mismatch");
    return record(24, [...stringFields(strings, interaction.id), [6, "u8", INTERACTION[interaction.kind]], [8, "u16", interaction.controlIds.length ? indexOf(controls, interaction.controlIds[0], "control") : NONE], [10, "u16", NONE], [12, "u16", NONE], [14, "u16", interaction.kind === "navigation" ? indexOf(pages, interaction.destinationPage, "page") : NONE]]);
  });
  assert(owners.size === interactionRows.length, "Duplicate prepared owner");
  return packBinaryObject("semantics", [{kind:1,data:concat(controlRows)},{kind:2,data:concat(outputRows)},{kind:3,data:concat(interactionRows)},{kind:4,data:strings.bytes()}]);
}
/** Resolves an already prepared, declarative scene. Native publication admission remains independent. */
export async function compileResolvedPanel(sourceInput: PackageSource, semanticInput: SemanticDescriptor, recipeInput: ResolvedRecipe, profileInput: DeviceProfile): Promise<CompiledPanel> {
  const source = structuredClone(sourceInput), semantics = structuredClone(semanticInput), recipe = structuredClone(recipeInput), profile = structuredClone(profileInput);
  assert(recipe.recipeVersion === 1, "Unsupported resolved recipe version"); id(recipe.id);
  assert(recipe.pages.length > 0 && recipe.pages.length <= profile.limits.pages && recipe.variables.length <= profile.limits.variables, "Recipe exceeds profile table limits");
  const pages = uniqueIndex(recipe.pages), outputs = uniqueIndex(semantics.outputs), owners = uniqueIndex(semantics.interactions), variables = uniqueIndex(recipe.variables);
  const imagePack = await packImages(recipe.images), fontPack = packFonts(recipe.fonts), strings = new Strings();
  const tables = new Map<number, Uint8Array[]>(), add = (section: number, bytes: Uint8Array) => { const rows = tables.get(section) ?? []; rows.push(bytes); tables.set(section, rows); };
  const ranges: (readonly [number, number])[] = [];
  for (const recipeVariable of recipe.variables) {
    let ref: number, kind = 1, sourceKind = 0, min = 0, max = Q, textBytes = 0;
    if (recipeVariable.source === "feedback" || recipeVariable.source === "available" || recipeVariable.source === "fresh") {
      ref = indexOf(outputs, recipeVariable.outputId, "output");
      sourceKind = recipeVariable.source === "feedback" ? 0 : recipeVariable.source === "available" ? 2 : 3;
      if (sourceKind === 0) { const output = semantics.outputs[ref]; kind = output.kind === "boolean" ? 1 : output.kind === "number" ? 2 : 3; min = toFixed(output.min); max = toFixed(output.max); textBytes = output.maxTextBytes; }
    } else if (recipeVariable.source === "pressed") { ref = indexOf(owners, recipeVariable.interactionId, "interaction"); sourceKind = 1; }
    else { assert(recipeVariable.source === "connected", "Unknown variable source"); ref = NONE; sourceKind = 4; }
    ranges.push([min, max]); add(3, record(24, [[0,"u16",ref],[2,"u8",kind],[3,"u8",sourceKind],[4,"i32",Math.max(min,Math.min(max,0))],[8,"i32",min],[12,"i32",max],[16,"u16",textBytes]]));
  }
  let instructionCount = 0, lookupCount = 0, pointCount = 0, nodeCount = 0, keyCount = 0;
  const nodeIds = new Map<string, number>(), physicalNodes: { page: number; bounds: Rect; reach: Rect; clip: Rect }[] = [];
  function lower(expression: Expression, depth = 0): Instruction[] {
    assert(depth <= 8, "Recipe expression depth exceeded");
    if (expression.kind === "constant") return [{op:EXPR.constant,value:expression.value}];
    if (expression.kind === "variable") return [{op:EXPR.variable,operand:indexOf(variables,expression.id,"variable")}];
    if (expression.kind === "lookup") {
      assert(expression.points.length >= 2 && expression.points.length <= 256, "Recipe lookup count exceeded");
      const first = lookupCount; for (const point of expression.points) { add(13,record(8,[[0,"i32",point[0]],[4,"i32",point[1]]])); ++lookupCount; }
      return [...lower(expression.value,depth+1),{op:EXPR.lookup,operand:first,value:expression.points.length}];
    }
    assert(expression.kind === "op" && Object.hasOwn(EXPR,expression.op) && !["constant","variable","lookup"].includes(expression.op), "Unknown recipe expression");
    const arity = expression.op === "not" ? 1 : expression.op === "select" || expression.op === "clamp" ? 3 : 2;
    assert(expression.args.length === arity, "Recipe expression arity mismatch");
    return [...expression.args.flatMap(arg=>lower(arg,depth+1)),{op:EXPR[expression.op]}];
  }
  // Lower one binding at a time so expression and lookup ranges are immutable, contiguous and bounded.
  const lookupPoints: LookupPoint[] = [];
  const allNodes = recipe.pages.flatMap(page=>page.nodes);
  assert(allNodes.length <= profile.limits.nodes, "Recipe node count exceeded");
  for (const [pageIndex,page] of recipe.pages.entries()) {
    const firstNode = nodeCount;
    for (const node of page.nodes) {
      id(node.id); assert(!nodeIds.has(node.id) && Object.hasOwn(OP,node.kind), "Duplicate/unknown recipe node"); nodeIds.set(node.id,nodeCount);
      const minShift=[0,0],maxShift=[0,0];
      for (const [property,expression] of Object.entries(node.bindings ?? {}) as [PropertyName,Expression][]) {
        assert(Object.hasOwn(PROPERTY,property), "Unknown recipe property");
        const firstLookup = lookupCount, instructions=lower(expression); expressionStackDepth(instructions);
        const encodedLookups=tables.get(13)??[]; for(let i=firstLookup;i<lookupCount;++i){const d=view(encodedLookups[i]);lookupPoints.push([d.getInt32(0,true),d.getInt32(4,true)]);}
        const inferred=expressionRange(instructions,ranges,lookupPoints); assert(inferred,"Unprovable recipe expression bounds");
        for(const instruction of instructions) add(5,record(16,[[0,"u8",instruction.op],[2,"u16",instruction.operand??0],[4,"i32",instruction.value??0]]));
        const declared=property==="translateX"||property==="translateY"?[Math.floor(inferred[0]/Q)*Q,Math.ceil(inferred[1]/Q)*Q]:inferred;
        add(4,record(16,[[0,"u16",nodeCount],[2,"u8",PROPERTY[property]],[4,"u16",instructionCount],[6,"u16",instructions.length],[8,"i32",declared[0]],[12,"i32",declared[1]]])); instructionCount+=instructions.length;
        if(property==="translateX"||property==="translateY"){const axis=property==="translateX"?0:1;minShift[axis]=Math.floor(inferred[0]/Q);maxShift[axis]=Math.ceil(inferred[1]/Q);}
      }
      for(const timeline of node.timelines??[]){
        assert(Object.hasOwn(PROPERTY,timeline.property)&&timeline.keyframes.length>=2&&timeline.keyframes.length<=8,"Invalid recipe timeline");
        const first=keyCount;
        for(const [time,value]of timeline.keyframes){add(7,record(8,[[0,"u16",time],[4,"i32",value]]));++keyCount;}
        add(6,record(24,[[0,"u16",nodeCount],[2,"u8",PROPERTY[timeline.property]],[3,"u8",timeline.interpolation==="step"?0:1],[4,"u16",first],[6,"u16",timeline.keyframes.length],[8,"u16",timeline.keyframes.at(-1)![0]],[10,"u16",timeline.repeat?1:0]]));
        if(timeline.property==="translateX"||timeline.property==="translateY"){const axis=timeline.property==="translateX"?0:1,values=timeline.keyframes.map(key=>key[1]);minShift[axis]=Math.floor(Math.min(...values)/Q);maxShift[axis]=Math.ceil(Math.max(...values)/Q);}
      }
      const reach:Rect=[node.bounds[0]+minShift[0],node.bounds[1]+minShift[1],node.bounds[2]+maxShift[0]-minShift[0],node.bounds[3]+maxShift[1]-minShift[1]],clip=node.clip??profile.content;
      const textRef=strings.add(node.text??""),firstPoint=pointCount;
      for(const point of node.points??[]){add(9,record(4,[[0,"i16",point[0]],[2,"i16",point[1]]]));++pointCount;}
      add(2,record(64,[[0,"u16",pageIndex],[2,"u8",OP[node.kind]],[3,"u8",node.visible===false?0:1],...geometry(node.bounds,4),...geometry(clip,12),...geometry(reach,20),
        [28,"u16",node.imageId===undefined?NONE:indexOf(imagePack.indexes,node.imageId,"image")],[30,"u16",node.maskId===undefined?NONE:indexOf(imagePack.indexes,node.maskId,"mask")],[32,"u16",node.fontId===undefined?NONE:indexOf(fontPack.indexes,node.fontId,"font")],
        [34,"u16",node.lineWidth??0],[36,"u16",node.color??0],[38,"u16",node.points?.length?firstPoint:0],[40,"u16",node.points?.length??0],[42,"u16",node.precision??0],[44,"u32",node.text?.length?textRef[0]:0],[48,"u16",textRef[1]],[50,"u16",node.kind==="text"?(node.maxTextBytes??Math.max(1,textRef[1])):0],[52,"u16",node.source?.[0]??0],[54,"u16",node.source?.[1]??0]]));
      physicalNodes.push({page:pageIndex,bounds:node.bounds,reach,clip});++nodeCount;
    }
    add(1,record(16,[...stringFields(strings,page.id),[6,"u16",firstNode],[8,"u16",nodeCount-firstNode],[10,"u16",page.background]]));
  }
  for(const hit of recipe.hits){const node=indexOf(nodeIds,hit.nodeId,"hit node"),geometryValue=hit.bounds??physicalNodes[node].bounds;add(8,record(24,[[0,"u16",indexOf(owners,hit.interactionId,"interaction")],[2,"u16",physicalNodes[node].page],...geometry(geometryValue,4),[12,"u16",node]]));}
  let tileNodeCount=0;const [tw,th]=profile.tile,[ox,oy,cw,ch]=profile.content;
  for(let page=0;page<recipe.pages.length;++page)for(let ty=0;ty<Math.ceil(ch/th);++ty)for(let tx=0;tx<Math.ceil(cw/tw);++tx){
    const x=ox+tx*tw,y=oy+ty*th,w=Math.min(tw,cw-tx*tw),h=Math.min(th,ch-ty*th);
    const members:number[]=[];
    for(const [index,node]of physicalNodes.entries()){
      const r=node.reach,c=node.clip,left=Math.max(r[0],c[0]),top=Math.max(r[1],c[1]),right=Math.min(r[0]+r[2],c[0]+c[2]),bottom=Math.min(r[1]+r[3],c[1]+c[3]);
      if(node.page===page&&left<right&&top<bottom&&left<x+w&&x<right&&top<y+h&&y<bottom)members.push(index);
    }
    if(members.length){add(11,record(12,[[0,"u16",page],[2,"u16",tx],[4,"u16",ty],[6,"u16",tileNodeCount],[8,"u16",members.length]]));for(const member of members){add(12,record(2,[[0,"u16",member]]));++tileNodeCount;}}
  }
  add(10,strings.bytes());
  const objectBytes=new Map<ObjectKind,Uint8Array>([["semantics",packSemantics(semantics,pages)],["scene",packBinaryObject("scene",[...tables].map(([kind,rows])=>({kind,data:concat(rows)})))]]);
  if(imagePack.bytes)objectBytes.set("images",imagePack.bytes);if(fontPack.bytes)objectBytes.set("fonts",fontPack.bytes);
  const facts=inspectContent(new Map([...objectBytes].map(([kind,bytes])=>[kind,inspectBinaryObject(kind,bytes)])),profile);
  const snapshotBytes=Math.max(8192+recipe.variables.length*32,Math.ceil(facts.minimumSnapshotBytes/256)*256),internalRamBytes=profile.limits.internalRamBytes,sceneRamBytes=internalRamBytes-snapshotBytes-1024-2048;
  const packed=await packPackage(source,objectBytes,profile,{internalRamBytes,psramBytes:0,sceneRamBytes,snapshotBytes,tileBytes:1024,transferBytes:2048,cacheBytes:0});
  const imageIdentity=await Promise.all(recipe.images.map(async image=>({...image,pixels:await digest(image.pixels)})));
  const fontIdentity=await Promise.all(recipe.fonts.map(async font=>({...font,glyphs:await Promise.all(font.glyphs.map(async glyph=>({...glyph,coverage:await digest(glyph.coverage)})))})));
  return {...packed,semantics,recipeHash:await digest(new TextEncoder().encode(canonical({...recipe,images:imageIdentity,fonts:fontIdentity})))};
}
/** Schema 2 first-delivery adapter: semantics come only from the verified publication. */
export async function compileLegacyPublication(publicationInput: Publication, recipeInput: ResolvedRecipe, profileInput: DeviceProfile): Promise<CompiledPanel> {
  const publication=structuredClone(publicationInput),recipe=structuredClone(recipeInput),profile=structuredClone(profileInput);
  const verified=await publishDocument(publication.document,publication.assets);
  assert(verified.publicationHash===publication.publicationHash&&verified.capabilityFingerprint===publication.capabilityFingerprint,"Source publication identity mismatch");
  const doc=verified.document,interactions:SemanticInteraction[]=[];
  for(const widget of doc.presentation.widgets)if(widget.controlIds.length){assert(widget.kind==="button"&&widget.config.buttonMode==="momentary","Embedded legacy adapter supports momentary buttons only");interactions.push({id:widget.id,kind:"momentary",controlIds:[...widget.controlIds]});}
  const semantics:SemanticDescriptor={version:1,controls:doc.ui.controls.map(control=>{assert(control.kind==="button","Unsupported legacy control");const binding=doc.bindings.find(binding=>binding.controlId===control.id)!;return{id:control.id,kind:"button",targetId:binding.targetId,targetIndex:binding.index,fallback:0};}),
    outputs:doc.ui.feedbackWidgets.map(output=>{assert(output.kind!=="text","Legacy text feedback needs an explicit compact text contract");const owner=doc.presentation.widgets.find(widget=>widget.feedbackIds.includes(output.id))!;const numeric=owner.kind==="numeric"||owner.kind==="gauge";if(numeric)assert(Number.isInteger(owner.config.min*Q)&&Number.isInteger(owner.config.max*Q),"Embedded legacy numeric bounds must be exactly representable as Q16.16");return{id:output.id,channelId:output.channelId,kind:output.kind==="indicator"?"boolean":"number",min:numeric?owner.config.min:0,max:numeric?owner.config.max:1,unit:numeric?owner.config.unit:"",maxTextBytes:0};}),interactions};
  return compileResolvedPanel({sourceUiId:doc.ui.id,sourcePublicationHash:verified.publicationHash as PackageSource["sourcePublicationHash"],semanticFingerprint:verified.capabilityFingerprint as PackageSource["semanticFingerprint"],compilerId:"osg-render-compiler",compilerVersion:"0.2.0"},semantics,recipe,profile);
}
