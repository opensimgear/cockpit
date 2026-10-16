import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { newDocument, createDesignerHistory, applyDesignerCommand, normalizeToPortableUi, publishDocument, canonical,
  migrateDesignerV2ToV3, validateDesignerDocumentV3, normalizeCompositeToPortableUi, publishDesignerV3Document,
  compositeSemanticData, compareDesignerV3Publications, checkCompositeTarget, importAsset, validateAssets } from "../src/index.ts";
const clone = structuredClone;
const targets = ["browser-dom-v1", "mobile-dom-v1", "sunton-3248s035r-render-v1", "waveshare-lcd7-s3-render-v1"];
const hash = value => "sha256:" + createHash("sha256").update(canonical(value)).digest("hex");
const recipe = widget => ({ id: "fixture-" + widget.kind, version: 1, sha256: hash({ fixtureRecipe: widget.kind, version: 1 }), targetProfileIds: [...targets], requires: ["scene.rect", "scene.text"] });
const valid = doc => { const r=validateDesignerDocumentV3(doc);assert.equal(r.ok,true,r.error);return r.value; };
const invalid = (doc, pattern) => { const r=validateDesignerDocumentV3(doc);assert.equal(r.ok,false);if(pattern)assert.match(r.error,pattern); };
function legacy() {
  let h=createDesignerHistory(newDocument("Source panel"));
  for(const kind of ["numeric","text","indicator","icon","gauge"]) h=applyDesignerCommand(h,{type:"add-widget",kind});
  const first=h.present.presentation.widgets[0];
  h=applyDesignerCommand(h,{type:"add-group",pageId:first.pageId,label:"System",widgetIds:[first.id]});
  h=applyDesignerCommand(h,{type:"update-widget",id:first.id,patch:{style:{accent:"#abcdef"}}});return h.present;
}
const source=()=>migrateDesignerV2ToV3(legacy(),recipe);
const refresh=doc=>{doc.ui=normalizeCompositeToPortableUi(doc);return doc;};
function addAxes(doc,kind){
  const count=kind==="xy"?2:3,base=doc.targets[0].axes.length,axisMode=kind==="xy"?"bipolar":"unipolar",controlIds=[];
  for(let i=0;i<count;i++){
    const id=kind+"-axis-"+i;controlIds.push(id);doc.ui.controls.push({id,kind:"axis",axisMode,label:id,fallback:kind==="xy"?0:1/3});
    doc.targets[0].axes.push(axisMode);doc.bindings.push({controlId:id,targetId:doc.targets[0].id,kind:"axis",index:base+i});
  }
  const interaction=kind==="xy"?{kind,centering:"spring",deadzone:0.1,invertX:false,invertY:false,atomic:true,reset:"fallback"}
    :{kind,vertices:[[0.5,0],[1,1],[0,1]],projection:"closest-point",quantizationSteps:65535,atomic:true,reset:"fallback"};
  const id=kind+"-owner";doc.semantics.owners.push({id,controlIds,feedbackIds:[],interaction});
  doc.presentation.components.push({id:kind+"-component",pageId:doc.ui.pages[0].id,label:kind,bounds:{x:0.5,y:0.5,width:0.3,height:0.3},ownerIds:[id],recipe:recipe({kind}),
    references:controlIds.map((refId,i)=>({slot:"axis-"+i,kind:"control",refId})),parameters:{},assetIds:[]});return refresh(doc);
}
function addNavigation(doc){
  const id="navigation-owner";doc.semantics.owners.push({id,controlIds:[],feedbackIds:[],interaction:{kind:"navigation",destinationPage:doc.ui.pages[0].id}});
  doc.presentation.components.push({id:"navigation-component",pageId:doc.ui.pages[0].id,label:"Next",bounds:{x:0.9,y:0,width:0.1,height:0.1},ownerIds:[id],recipe:recipe({kind:"navigation"}),references:[],parameters:{},assetIds:[]});return refresh(doc);
}
test("explicit migration preserves IDs, targets, assets, fallbacks and schema 2 identity",async()=>{
  const old=legacy(),before=clone(old),publication=await publishDocument(old,[]),doc=migrateDesignerV2ToV3(old,recipe);
  assert.equal(doc.schemaVersion,3);assert.deepEqual(doc.ui,old.ui);assert.deepEqual(doc.targets,old.targets);assert.deepEqual(doc.bindings,old.bindings);
  assert.deepEqual(doc.presentation.components.map(c=>c.id),old.presentation.widgets.map(w=>w.id));assert.deepEqual(doc.presentation.groups,old.presentation.groups);
  assert.deepEqual(normalizeCompositeToPortableUi(doc),doc.ui);assert.deepEqual(migrateDesignerV2ToV3(old,recipe),doc);assert.deepEqual(old,before);
  assert.equal((await publishDocument(old,[])).publicationHash,publication.publicationHash);assert.notEqual((await publishDesignerV3Document(doc,[])).publicationHash,publication.publicationHash);
  assert.equal(doc.semantics.feedback.find(c=>c.valueType==="text").maxTextBytes,1024);
  const detached=valid(doc);detached.presentation.components[0].label="Detached";assert.notEqual(detached.presentation.components[0].label,doc.presentation.components[0].label);
});
test("unsupported legacy behaviors fail explicitly rather than disappearing",()=>{
  for(const kind of ["selector","hat","encoder","slider","xy"]){const old=applyDesignerCommand(createDesignerHistory(newDocument()),{type:"add-widget",kind}).present,before=clone(old);
    assert.throws(()=>migrateDesignerV2ToV3(old,recipe),new RegExp("Cannot migrate widget.*unsupported "+kind));assert.deepEqual(old,before);}
  for(const mode of ["pulse","repeat"]){const old=newDocument();old.presentation.widgets[0].config={buttonMode:mode,pulseMs:10,repeatMs:mode==="repeat"?100:0};assert.throws(()=>migrateDesignerV2ToV3(old,recipe),new RegExp("unsupported "+mode));}
  const toggle=newDocument();toggle.presentation.widgets[0].config.buttonMode="toggle";assert.equal(migrateDesignerV2ToV3(toggle,recipe).semantics.owners[0].interaction.kind,"toggle");
  assert.throws(()=>migrateDesignerV2ToV3({},recipe));assert.throws(()=>migrateDesignerV2ToV3(newDocument(),()=>({...recipe({kind:"button"}),sha256:"latest"})),/immutable/);
});
test("owner IDs resolve collisions while preserving existing primitive/widget IDs",()=>{
  const old=newDocument(),widget=old.presentation.widgets[0],before=widget.controlIds[0],collision="owner-"+widget.id;
  old.ui.controls[0].id=collision;widget.controlIds[0]=collision;old.bindings.find(b=>b.controlId===before).controlId=collision;old.ui=normalizeToPortableUi(old);
  const doc=migrateDesignerV2ToV3(old,recipe);assert.equal(doc.ui.controls[0].id,collision);assert.notEqual(doc.semantics.owners[0].id,collision);assert.equal(doc.presentation.components[0].id,widget.id);
});
test("read-only aliases reference feedback repeatedly without duplicate logical placements",()=>{
  const doc=source(),output=doc.ui.feedbackWidgets[0],baseline=clone(doc.ui.pages);
  doc.presentation.components.push({id:"status-repeat",pageId:doc.ui.pages[0].id,label:"Repeated status",bounds:{x:0.5,y:0,width:0.1,height:0.1},ownerIds:[],recipe:recipe({kind:"status"}),
    references:[{slot:"lamp",kind:"feedback",refId:output.id},{slot:"label",kind:"feedback",refId:output.id}],parameters:{glow:true,opacity:0.5,caption:"READY"},assetIds:[]});
  valid(doc);assert.deepEqual(normalizeCompositeToPortableUi(doc).pages,baseline);
  const duplicate=clone(doc);duplicate.presentation.components.at(-1).ownerIds=[doc.semantics.owners[1].id];invalid(duplicate,/multiply placed/);
  const stolen=clone(doc);stolen.presentation.components.at(-1).references=[{slot:"button",kind:"control",refId:doc.ui.controls[0].id}];invalid(stolen,/unowned recipe reference/);
});
test("immutable publication hashes distinguish visual changes from semantic behavior",async()=>{
  const doc=source(),published=await publishDesignerV3Document(doc,[]);assert.equal(published.publicationHash,hash(doc));assert.equal(published.capabilityFingerprint,hash(compositeSemanticData(doc)));
  assert.ok(Object.isFrozen(published)&&Object.isFrozen(published.document.semantics.owners[0].interaction));assert.equal(compareDesignerV3Publications(doc,clone(doc)),"identical");
  const visual=clone(doc);visual.ui.title="New title";visual.targets[0].label="New target label";visual.presentation.components[0].label="New button";
  visual.presentation.components[0].bounds.x=0.1;visual.presentation.components[0].recipe.version=2;visual.presentation.components[0].recipe.sha256=hash({artwork:2});visual.presentation.components[0].parameters.font="new-font";refresh(visual);
  assert.equal(compareDesignerV3Publications(doc,visual),"styling");assert.equal((await publishDesignerV3Document(visual,[])).capabilityFingerprint,published.capabilityFingerprint);
  for(const change of [d=>d.semantics.owners[0].interaction={kind:"toggle"},d=>d.semantics.feedback.find(c=>c.valueType==="number").unit="kW",
    d=>d.semantics.feedback.find(c=>c.valueType==="number").range.max=250,d=>d.targets[0].buttons++,d=>d.bindings[0].index=7,d=>d.ui.feedbackWidgets[0].channelId="new-channel",
    d=>{d.semantics.owners[0].id="new-owner";d.presentation.components[0].ownerIds=["new-owner"];}]){
    const semantic=clone(doc);change(semantic);assert.equal(compareDesignerV3Publications(doc,semantic),"capability");}
  const reordered=clone(doc);reordered.ui.feedbackWidgets.reverse();reordered.semantics.feedback.reverse();reordered.semantics.owners.reverse();assert.equal((await publishDesignerV3Document(reordered,[])).capabilityFingerprint,published.capabilityFingerprint);
  const unbound=clone(doc);unbound.bindings=[];valid(unbound);await assert.rejects(()=>publishDesignerV3Document(unbound,[]),/Every input/);
  await assert.rejects(()=>publishDesignerV3Document(doc,[{assetId:"unknown",base64:""}]),/payloads/);
  const previous=String.prototype.localeCompare;String.prototype.localeCompare=()=>{throw Error("Locale-dependent sorting");};try{await publishDesignerV3Document(doc,[]);}finally{String.prototype.localeCompare=previous;}
});
test("explicit advanced interactions have target and ABI lowering diagnostics",()=>{
  const doc=valid(addNavigation(addAxes(addAxes(source(),"xy"),"barycentric3"))),profile={id:targets[2],features:["scene.rect","scene.text","input.momentary","input.navigation"]};
  const errors=checkCompositeTarget(doc,profile);assert.deepEqual(new Set(errors.map(e=>e.requiredFeature)),new Set(["input.xy","input.barycentric3"]));assert.ok(errors.every(e=>e.componentId&&e.targetProfileId===profile.id));
  const browser={id:targets[0],features:[...profile.features,"input.xy","input.barycentric3"]};assert.deepEqual(checkCompositeTarget(doc,browser),[]);
  assert.ok(checkCompositeTarget(doc,{id:"unqualified",features:[]}).some(e=>e.message.includes("does not support target")));
  const guarded=source();guarded.semantics.owners[0].interaction={kind:"guardedMomentary",guardMs:3000};valid(guarded);assert.ok(checkCompositeTarget(guarded,profile).some(e=>e.requiredFeature==="input.guarded-momentary"));
  const embedded={...profile,rendererAbi:1,features:[...browser.features,"input.guarded-momentary"],limits:{dynamicStringBytes:120}};
  guarded.semantics.owners[0].interaction.guardMs=50000;valid(guarded);assert.ok(checkCompositeTarget(guarded,embedded).some(e=>e.message.includes("50..10000")));
  guarded.semantics.owners[0].interaction.guardMs=1;assert.ok(checkCompositeTarget(guarded,embedded).some(e=>e.message.includes("50..10000")));
  guarded.semantics.owners[0].interaction.guardMs=50;assert.ok(!checkCompositeTarget(guarded,embedded).some(e=>e.message.includes("guard interval")));
  let abiErrors=checkCompositeTarget(doc,embedded);assert.ok(abiErrors.some(e=>e.message.includes("32767")));assert.ok(abiErrors.some(e=>e.message.includes("XY transform")));assert.ok(abiErrors.some(e=>e.message.includes("requires 1024 text bytes")));
  const lower=clone(doc);lower.semantics.owners.find(o=>o.interaction.kind==="barycentric3").interaction.quantizationSteps=32767;abiErrors=checkCompositeTarget(lower,embedded);assert.ok(!abiErrors.some(e=>e.message.includes("quantization requires")));assert.ok(abiErrors.some(e=>e.message.includes("vertices/reset")));
  const toggle=source();toggle.semantics.owners[0].interaction={kind:"toggle"};valid(toggle);assert.ok(checkCompositeTarget(toggle,profile).some(e=>e.requiredFeature==="input.toggle"));
  const changed=clone(doc);changed.semantics.owners.find(o=>o.interaction.kind==="xy").interaction.invertX=true;assert.equal(compareDesignerV3Publications(doc,changed),"capability");
});
test("strict ownership, projection, immutable recipes and scalar visual data reject malformed sources",()=>{
  const seed=source(),cases=[
    [d=>d.schemaVersion=2,/schema/],[d=>d.semantics.version=2,/descriptor/],[d=>d.extra=true,/fields/],
    [d=>d.semantics.owners[0].id=d.ui.controls[0].id,/Duplicate/],[d=>d.semantics.owners[0].controlIds.push(d.ui.controls[0].id),/Duplicate/],
    [d=>d.semantics.owners[0].controlIds=["missing"],/Unknown/],[d=>d.semantics.owners.push({...d.semantics.owners[0],id:"second-owner"}),/multiply owned/],
    [d=>d.semantics.owners[1].feedbackIds=["missing"],/Unknown/],[d=>d.semantics.owners[1].feedbackIds=[d.semantics.owners[2].feedbackIds[0]],/multiply owned/],
    [d=>d.semantics.owners.splice(1,1),/Unowned/],[d=>d.semantics.owners[0].interaction={kind:"script",code:"run()"},/Unsupported/],
    [d=>d.semantics.owners[0].interaction={kind:"display"},/Display owner/],[d=>d.semantics.owners[1].interaction={kind:"momentary"},/one button/],
    [d=>d.semantics.owners[0].interaction={kind:"navigation",destinationPage:"missing"},/navigation/],[d=>d.semantics.owners[0].interaction={kind:"guardedMomentary",guardMs:0},/bounds/],
    [d=>d.presentation.components[0].bounds.width=2,/bounds/],[d=>d.presentation.components[0].bounds.x=0.99,/outside page/],
    [d=>d.presentation.components[0].pageId="missing",/page/],[d=>d.presentation.components[0].groupId="missing",/group/],
    [d=>d.presentation.components[0].style.foreground="red",/color/],[d=>d.presentation.components[0].ownerIds=["missing"],/owner/],
    [d=>d.presentation.components[0].ownerIds=[],/unowned recipe reference/],[d=>d.presentation.components[0].references=[],/missing a recipe/],
    [d=>d.presentation.components[0].references.push(clone(d.presentation.components[0].references[0])),/slot/],
    [d=>d.presentation.components[0].references[0].kind="script",/reference/],[d=>d.presentation.components[1].references[0].refId="missing",/reference/],
    [d=>d.presentation.components[0].recipe.sha256="latest",/immutable/],[d=>d.presentation.components[0].recipe.version="latest",/bounds/],
    [d=>d.presentation.components[0].recipe.targetProfileIds=[],/explicit compatible/],[d=>d.presentation.components[0].recipe.requires=["INVALID"],/feature/],
    [d=>d.presentation.components[0].recipe.requires=["scene.rect","scene.rect"],/feature/],
    [d=>d.presentation.components[0].parameters={object:{script:"run()"}},/scalar/],[d=>d.presentation.components[0].parameters={value:"x".repeat(257)},/scalar/],
    [d=>d.presentation.components[0].parameters={value:1e10},/bounds/],[d=>d.presentation.components[0].parameters=Object.fromEntries(Array.from({length:33},(_,i)=>["p"+i,i])),/Too many/],
    [d=>d.presentation.components[0].assetIds=["missing"],/asset/],[d=>{d.presentation.components[0].ownerIds=[];d.presentation.components[0].references=[];},/Unplaced/],
    [d=>d.presentation.components[0].bounds.x+=0.01,/projection differ/],[d=>d.presentation.groups[0].pageId="missing",/group page/],
    [d=>d.presentation.grid.snap=1,/grid/],[d=>d.presentation.viewports[0].pageId="missing",/viewport/],
    [d=>d.presentation.viewports[0].backgroundAssetId="missing",/background/],[d=>d.presentation.viewports[0].safeInsets={top:0.25,bottom:0.25,right:0,left:0},/safe insets/],
    [d=>d.targets[0].axes=["invalid"],/target axis/],[d=>{d.targets[0].axes=[];d.targets[0].buttons=0;d.targets[0].hats=0;},/Empty/],
    [d=>d.bindings[0].targetId="missing",/logical binding/],[d=>d.bindings[0].kind="axis",/logical binding/],[d=>d.bindings[0].index=100,/bounds/],
  ];
  for(const [mutate,pattern]of cases){const doc=clone(seed);mutate(doc);invalid(doc,pattern);}
  for(const value of [undefined,()=>{},Symbol("script"),NaN,Infinity]){const doc=clone(seed);doc.presentation.components[0].parameters.invalid=value;invalid(doc,/JSON|Nonfinite/);}
  const huge=clone(seed);huge.presentation.components[0].parameters.huge="x".repeat(1024*1024);invalid(huge,/1 MiB/);
  const deep=clone(seed);let nested={};deep.presentation.components[0].parameters.deep=nested;for(let i=0;i<40;i++){nested.child={};nested=nested.child;}invalid(deep,/nesting/);
});
test("feedback contracts reject incompatible types, ranges, text capacities and provenance",()=>{
  const seed=source();for(const mutate of [d=>d.semantics.feedback[0].outputId="missing",d=>d.semantics.feedback[1].outputId=d.semantics.feedback[0].outputId,
    d=>d.semantics.feedback[0].valueType="text",d=>d.semantics.feedback[0].range.max=d.semantics.feedback[0].range.min,d=>d.semantics.feedback[0].maxTextBytes=4,
    d=>d.semantics.feedback[1].maxTextBytes=0,d=>d.semantics.feedback[1].unit="m",d=>d.semantics.feedback[2].range={min:0,max:1},
    d=>d.semantics.feedback[2].maxTextBytes=20,d=>d.semantics.feedback[2].provenancePolicy="hide-source"]){const doc=clone(seed);mutate(doc);invalid(doc);}
});
test("advanced transform and target invariants are independent of visual recipe claims",()=>{
  const seed=addNavigation(addAxes(addAxes(source(),"xy"),"barycentric3")),xy=d=>d.semantics.owners.find(o=>o.interaction.kind==="xy"),tri=d=>d.semantics.owners.find(o=>o.interaction.kind==="barycentric3");
  for(const mutate of [d=>xy(d).controlIds.pop(),d=>xy(d).interaction.centering="invalid",d=>xy(d).interaction.atomic=false,d=>xy(d).interaction.reset="zero",
    d=>xy(d).interaction.deadzone=0.5,d=>tri(d).interaction.vertices=[[0,0],[0,0],[1,1]],d=>tri(d).interaction.vertices[0]=[2,0],d=>tri(d).interaction.projection="clamp-axes",
    d=>tri(d).interaction.quantizationSteps=0,d=>d.ui.controls.find(c=>c.id===tri(d).controlIds[0]).fallback=0,d=>d.targets[0].axes[2]="unipolar",
    d=>{const index=d.bindings.findIndex(b=>b.kind==="axis");d.bindings[index+1].index=d.bindings[index].index;},d=>d.semantics.owners.find(o=>o.interaction.kind==="navigation").interaction.destinationPage="missing"]){const doc=clone(seed);mutate(doc);invalid(doc);}
  assert.throws(()=>normalizeCompositeToPortableUi({...seed,semantics:{...seed.semantics,owners:[]}}),/Unknown component owner/);assert.throws(()=>compositeSemanticData({}),/JSON|fields/);
});
test("schema 2 and schema 3 publications return exactly the bytes verified before awaiting",async()=>{
  const bytes=Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="),c=>c.charCodeAt(0));
  const imported=await importAsset(newDocument(),bytes,"image/png");imported.document.presentation.widgets[0].config.iconAssetId=imported.asset.assetId;
  const expected=clone(imported.asset),expectedManifest=clone(imported.document.ui.assets[0]);
  for(const schema of [2,3]){
    const document=schema===2?clone(imported.document):migrateDesignerV2ToV3(imported.document,recipe),payloads=[clone(imported.asset)];
    const pending=schema===2?publishDocument(document,payloads):publishDesignerV3Document(document,payloads);
    payloads[0].base64="";payloads.push({assetId:"injected-after-verification",base64:""});document.ui.assets[0].sha256="f".repeat(64);
    const publication=await pending;assert.deepEqual(publication.assets,[expected]);assert.deepEqual(publication.document.ui.assets,[expectedManifest]);assert.ok(Object.isFrozen(publication.assets[0]));
    if(schema===3){assert.deepEqual(publication.document.presentation.components[0].assetIds,[expected.assetId]);assert.deepEqual(publication.document.ui.assets,imported.document.ui.assets);}
  }
  const document=clone(imported.document),payloads=[clone(imported.asset)],pending=validateAssets(document,payloads);
  document.ui.assets[0].sha256="0".repeat(64);document.ui.assets[0].sizeBytes=1;payloads[0].base64="";assert.deepEqual(await pending,[expected]);
});
test("ABI 1 preflight diagnoses numeric range conversion and UTF-8 unit capacity",()=>{
  const old=applyDesignerCommand(createDesignerHistory(newDocument()),{type:"add-widget",kind:"numeric"}).present,doc=migrateDesignerV2ToV3(old,recipe);
  const profile={id:targets[2],rendererAbi:1,features:["scene.rect","scene.text","input.momentary"]},contract=doc.semantics.feedback[0];
  assert.deepEqual(checkCompositeTarget(doc,profile),[]);
  contract.range.max=1e9;contract.unit="W".repeat(25);valid(doc);
  let errors=checkCompositeTarget(doc,profile);assert.ok(errors.some(e=>e.message.includes("Q16.16 limits")));assert.ok(errors.some(e=>e.message.includes("25 UTF-8 unit bytes")));
  assert.deepEqual(checkCompositeTarget(doc,{...profile,rendererAbi:undefined}),[]);
  contract.range={min:0,max:1e-9};contract.unit="μ".repeat(13);errors=checkCompositeTarget(doc,profile);
  assert.ok(errors.some(e=>e.message.includes("range collapses")));assert.ok(errors.some(e=>e.message.includes("26 UTF-8 unit bytes")));
  contract.range={min:-1e9,max:1};contract.unit="";assert.ok(checkCompositeTarget(doc,profile).some(e=>e.message.includes("Q16.16 limits")));
  contract.range={min:-32768,max:2147483647/65536};contract.unit="μ".repeat(12);assert.deepEqual(checkCompositeTarget(doc,profile),[]);
});
