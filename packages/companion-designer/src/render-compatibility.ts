import fonts from '../../companion-ui/assets/fonts/metrics.json' with { type: 'json' };
import { canonical, validateDesignerDocument } from './model.ts';
import { validateRenderProfile } from './render-profiles.ts';
import { resolveWidgetLayers, WIDGET_STATES, widgetRequiredFeatures } from './widget-model.ts';
import type { Bounds, DesignerDocument, DesignerWidget } from './types.ts';

export interface RenderCompatibilityIssue {
  widget?: string; definition?: string; layer?: string;
  field: string; required: number | string; available: number | string; message: string;
}
const glyphs = Object.fromEntries(Object.entries(fonts.fonts).map(([face,font]) => [face,new Set(font.advances.map(([code])=>code))]));
const bytes = (text:string) => new TextEncoder().encode(text).length;
const nodes = (value:unknown):number => 1 + (value && typeof value==='object' ? Object.values(value).reduce<number>((sum,child)=>sum+nodes(child),0) : 0);

/** Predictable target limits only. The device still checks actual free arenas,
 * font allocation and cache availability before adopting an immutable candidate.
 * The supplied source is never simplified or changed. Browser/Android authoring
 * does not call this with an invented ESP32 profile. */
export function renderCompatibility(input:DesignerDocument, report:unknown):RenderCompatibilityIssue[] {
  const issues:RenderCompatibilityIssue[]=[];
  let identity:{widget?:string;definition?:string;layer?:string}={};
  const add=(field:string,required:number|string,available:number|string,target='renderer')=>{
    const owner=identity.widget?`Widget ${identity.widget}`:identity.definition?`Definition ${identity.definition}`:'Panel';
    issues.push({...identity,field,required,available,message:`${owner}${identity.layer?` layer ${identity.layer}`:''}: ${field} requires ${required}; ${target} provides ${available}.`});
  };
  const profile=validateRenderProfile(report);
  if(!profile.ok){add('renderProfile',profile.error,'unavailable');return issues;}
  const validated=validateDesignerDocument(input);
  if(!validated.ok){add('source',validated.error,'invalid');return issues;}
  const document=validated.value, p=profile.value, limits=p.limits;
  const over=(field:string,required:number,available:number)=>{if(required>available)add(field,required,available,p.id);};
  const feature=(name:string)=>{if(!p.features.includes(name))add(name,'supported','unavailable',p.id);};
  const font=(text:string,face:'sans'|'mono',field:string)=>{
    const missing=[...new Set([...text].filter(character=>!glyphs[face].has(character.codePointAt(0)!)))];
    if(missing.length)add(field,missing.slice(0,8).map(character=>'U+'+character.codePointAt(0)!.toString(16).toUpperCase().padStart(4,'0')).join(', '),`${face} glyph set`,p.id);
  };
  feature(`panel.v${document.schemaVersion}`);
  over('documentBytes',bytes(canonical(document)),limits.documentBytes);
  over('json.nodes',nodes(document),limits.jsonNodes);
  over('pages',document.ui.pages.length,limits.pages);
  over('widgets',document.presentation.widgets.length,limits.widgets);
  over('controls',document.ui.controls.length,limits.widgets);
  over('outputs',document.ui.feedbackWidgets.length,limits.outputs);
  if(document.bindings.length!==document.ui.controls.length)add('bindings',document.ui.controls.length,document.bindings.length,p.id);
  for(const page of document.ui.pages)if(!document.presentation.widgets.some(widget=>widget.pageId===page.id))add(`page.${page.id}.widgets`,1,0,p.id);
  const definitions=document.presentation.definitions??[];
  over('definitions',definitions.length,limits.definitions);
  for(const definition of definitions){
    identity={definition:definition.id};
    // Even unused revisions undergo scratch compilation, then release their
    // counters. They do not consume the retained instance layer budget twice.
    over('layers',definition.layers.length,Math.min(limits.layersPerWidget,limits.totalLayers));
    over('inputPorts',definition.inputs.length,limits.portsPerWidget);
    over('outputPorts',definition.outputs.length,limits.portsPerWidget);
  }
  identity={};
  for(const viewport of document.presentation.viewports)if(viewport.backgroundAssetId)add('viewport.backgroundAssetId','supported','unavailable',p.id);
  if(document.presentation.groups.length)feature('group.v1');
  for(const group of document.presentation.groups){
    if(!document.presentation.widgets.some(widget=>widget.groupId===group.id))continue;
    identity={widget:group.id};font(group.label,'sans','group.label');
  }
  let totalLayers=0,components=0,imageReferences=0,imageBytes=0;
  const rasters=new Set<string>();
  const image=(assetId:string,width:number,height:number)=>{
    imageReferences++;
    over('imageReferences',imageReferences,limits.imageReferences);
    if(width<1||height<1)add('image.minimumPixels',1,Math.min(width,height),p.id);
    over('image.width',width,limits.imageWidth);over('image.height',height,limits.imageHeight);
    over('image.pixels',width*height,limits.imagePixels);
    const hash=document.ui.assets.find(asset=>asset.id===assetId)!.sha256;
    const key=`${hash}:${width}:${height}`;
    if(!rasters.has(key)){rasters.add(key);imageBytes+=width*height*3;over('imageBytes',imageBytes,limits.imageBytes);}
  };
  const touchAreas:{widget:DesignerWidget;bounds:Bounds;interactive:boolean}[]=[];
  for(const widget of document.presentation.widgets){
    identity={widget:widget.id};
    const viewport=document.presentation.viewports.find(viewport=>viewport.pageId===widget.pageId)!;
    const inset=viewport.safeInsets, scale=Math.min(p.screen.width/viewport.width,p.screen.height/viewport.height);
    let width=widget.bounds.width*(1-(inset?.left??0)-(inset?.right??0))*viewport.width*scale;
    let height=widget.bounds.height*(1-(inset?.top??0)-(inset?.bottom??0))*viewport.height*scale;
    let pixelWidth=Math.floor(width),pixelHeight=Math.floor(height);
    let artwork:Bounds={x:0,y:0,width:1,height:1};
    let hit:Bounds={x:0,y:0,width:1,height:1},interactive=widget.kind==='button';
    if(widget.kind==='component'){
      const definition=definitions.find(d=>d.id===widget.config.definitionId&&d.version===widget.config.definitionVersion)!;
      const fit=Math.min(width/definition.size.width,height/definition.size.height);
      const w=definition.size.width*fit,h=definition.size.height*fit;
      if(width>0&&height>0)artwork={x:(1-w/width)/2,y:(1-h/height)/2,width:w/width,height:h/height};
      width=w;height=h;pixelWidth=Math.floor(w);pixelHeight=Math.floor(h);
      hit=definition.hitArea;interactive=definition.interaction.kind!=='none';
      identity.definition=definition.id;components++;totalLayers+=definition.layers.length;
      over('components',components,limits.components);over('layers',totalLayers,limits.totalLayers);
      for(const name of widgetRequiredFeatures(definition))feature(name);
      if(pixelWidth<1||pixelHeight<1)add('geometry.minimumPixels',1,Math.min(pixelWidth,pixelHeight),p.id);
      over('scale',Math.ceil(Math.max(pixelWidth/definition.size.width,pixelHeight/definition.size.height)),limits.scale);
      if(definition.interaction.kind!=='none'){
        const touch=Math.min(Math.floor(width*definition.hitArea.width),Math.floor(height*definition.hitArea.height));
        if(touch<limits.touchPixels)add('hitArea.minimumPixels',limits.touchPixels,touch,p.id);
      }
      const states=WIDGET_STATES.map(state=>resolveWidgetLayers(definition,widget.config.properties,state));
      for(let index=0;index<definition.layers.length;index++){
        identity.layer=definition.layers[index].id;
        const layer=states[0][index];
        if(layer.kind==='image'){
          const assetId=layer.assetId as string;
          if(assetId)image(assetId,Math.round(pixelWidth*layer.bounds.width),Math.round(pixelHeight*layer.bounds.height));
        }
        for(const resolved of states.map(layers=>layers[index])){
          if(resolved.visible===false||resolved.opacity===0||resolved.kind!=='text')continue;
          over('fontSize',resolved.fontSize,limits.fontSize);
          // Output text is checked again when it arrives. Static text is used
          // only without an output binding; unknown/stale placeholders and a
          // numeric suffix use the very same pinned font coverage.
          if(resolved.outputPort){
            font('—STALE',resolved.font,'text.glyphs');
            const output=definition.outputs.find(output=>output.name===resolved.outputPort)!;
            if(output.kind==='gauge')font('0123456789.- '+(resolved.unit??''),resolved.font,'text.glyphs');
            if(output.kind==='indicator')font('truefalse',resolved.font,'text.glyphs');
          }else font(resolved.text as string,resolved.font,'text.glyphs');
        }
        delete identity.layer;
      }
    }else{
      feature(`legacy.${widget.kind}`);
      const config=widget.config as Record<string,unknown>;
      for(const [key,value] of Object.entries({label:widget.label,prefix:config.prefix,suffix:config.suffix,unit:config.unit})){
        if(typeof value!=='string')continue;
        if(!/^[A-Za-z0-9 .,:;\-_+=|/%!()?]*$/.test(value))add(`legacy.${key}.glyphs`,'supported text','basic device glyph set',p.id);
        if(key==='unit')over('legacy.unitBytes',bytes(value),32);
      }
      if(config.iconAssetId)add('button.iconAssetId','supported','unavailable',p.id);
      if(widget.kind==='button'){
        if(config.buttonMode!=='momentary'||config.pulseMs!==0||config.repeatMs!==0)add('button.interaction','momentary without pulse/repeat',String(config.buttonMode),p.id);
        const touch=Math.min(pixelWidth,pixelHeight);if(touch<limits.touchPixels)add('hitArea.minimumPixels',limits.touchPixels,touch,p.id);
      }
      if(widget.kind==='numeric'||widget.kind==='gauge'){
        over('legacy.precision',Number(config.precision),p.textFormat.fractionDigits);
        over('legacy.range',Math.ceil(Math.max(Math.abs(Number(config.min)),Math.abs(Number(config.max)))),1e9);
      }
      if(widget.kind==='image'){
        if(config.assetId)image(String(config.assetId),pixelWidth,pixelHeight);
        else add('image.assetId','selected asset','unavailable',p.id);
      }
    }
    // Match the firmware's single-pointer admission, including local hit areas.
    // Read-only component artwork may overlap; legacy button geometry retains
    // its existing stricter rule. Compare after the same safe-inset transform.
    hit={x:artwork.x+hit.x*artwork.width,y:artwork.y+hit.y*artwork.height,width:hit.width*artwork.width,height:hit.height*artwork.height};
    const usableX=1-(inset?.left??0)-(inset?.right??0),usableY=1-(inset?.top??0)-(inset?.bottom??0);
    const bounds={x:(inset?.left??0)+widget.bounds.x*usableX+widget.bounds.width*usableX*hit.x,y:(inset?.top??0)+widget.bounds.y*usableY+widget.bounds.height*usableY*hit.y,width:widget.bounds.width*usableX*hit.width,height:widget.bounds.height*usableY*hit.height};
    const overlap=touchAreas.find(other=>{
      const portable=widget.kind==='component'||other.widget.kind==='component';
      const constrained=portable?interactive&&other.interactive:widget.kind!=='image'&&other.widget.kind!=='image'&&(widget.kind==='button'||other.widget.kind==='button');
      const a=other.bounds;
      return widget.pageId===other.widget.pageId&&constrained&&bounds.x<a.x+a.width&&a.x<bounds.x+bounds.width&&bounds.y<a.y+a.height&&a.y<bounds.y+bounds.height;
    });
    if(overlap)add('hitArea.overlap','non-overlapping touch targets',overlap.widget.id,p.id);
    touchAreas.push({widget,bounds,interactive});
  }
  // Several states may expose the same unsupported glyph or size. Keep one
  // actionable diagnostic per exact identity/field/count rather than eight.
  return issues.filter((issue,index)=>issues.findIndex(other=>JSON.stringify(other)===JSON.stringify(issue))===index);
}
