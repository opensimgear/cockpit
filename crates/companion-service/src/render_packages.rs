//! Immutable render content and source-semantic authority. Device scene admission
//! is an additional gate; integrity or a cache hit never creates an input source.
use crate::publication::{digest_id, ensure, hash};
use crate::render_object::{inspect_render_object, RenderObjectKind};
use crate::Publication;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::{fs,io::Read,path::{Path,PathBuf}};

pub const CHUNK_BYTES: usize = 2048;
pub const CACHE_BYTES: usize = 16 * 1024 * 1024;
pub const PACKAGE_BYTES: usize = 2560 * 1024;
pub const OBJECT_KINDS: [&str; 4] = ["semantics", "scene", "images", "fonts"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderCounts {
    pub pages: u16, pub nodes: u16, pub variables: u16, pub bindings: u16,
    pub hit_regions: u16, pub glyphs: u16, pub timelines: u16,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderResources {
    pub object_bytes: u32, pub internal_ram_bytes: u32, pub psram_bytes: u32,
    pub scene_ram_bytes: u32, pub snapshot_bytes: u32, pub tile_bytes: u32,
    pub transfer_bytes: u32, pub cache_bytes: u32, pub max_page_draw_ops: u32,
    pub max_update_instructions: u32, pub max_dirty_pixels: u32,
    pub max_layers_per_tile: u32, pub max_blend_pixel_layers: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderViewport { pub width: u16, pub height: u16, pub content: [u16; 4] }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderObjectDescriptor { pub kind: String, pub version: u16, pub bytes: u32, pub sha256: String }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderManifest {
    pub format_version: u16, pub renderer_abi: u16, pub source_ui_id: String,
    pub source_publication_hash: String, pub semantic_fingerprint: String,
    pub semantic_descriptor_hash: String, pub target_profile_id: String,
    pub target_profile_hash: String, pub compiler_id: String, pub compiler_version: String,
    pub viewport: RenderViewport, pub requires: Vec<String>, pub counts: RenderCounts,
    pub resources: RenderResources, pub objects: Vec<RenderObjectDescriptor>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderPackageUpload {
    pub manifest: String, pub package_hash: String, pub objects: BTreeMap<String, String>,
}
#[derive(Clone)]
pub struct RenderPackage {
    pub manifest: RenderManifest, pub manifest_bytes: Arc<[u8]>, pub package_hash: String,
    pub objects: BTreeMap<String, Arc<[u8]>>, pub bytes: usize, pub cache_bytes: usize,
    pub source_document: Arc<Value>,
}

/// Rebuildable native cache. Content and source semantics are revalidated on
/// load. Neither this index nor a filename supplies source/input authority.
pub type RenderSelections = BTreeMap<(String,String),String>;
#[derive(Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct SelectionEntry { publication_hash:String, profile_hash:String, package_hash:String }
#[derive(Deserialize,Serialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct SelectionIndex { version:u8, entries:Vec<SelectionEntry> }
pub struct RenderDiskCache { root: PathBuf }
impl RenderDiskCache {
    pub fn new(root: impl AsRef<Path>) -> Self { Self { root: root.as_ref().to_owned() } }
    fn path(&self, package: &str) -> Result<PathBuf,String> {
        ensure(digest_id(package),"Invalid cached package identity.")?;
        Ok(self.root.join(format!("{}.json",&package[7..])))
    }
    fn read(path:&Path,limit:usize)->Result<Vec<u8>,String>{
        let metadata=fs::symlink_metadata(path).map_err(|_|"Render cache file unavailable.")?;
        ensure(metadata.is_file()&&!metadata.file_type().is_symlink()&&metadata.len()<=limit as u64,"Invalid render cache file/budget.")?;
        let mut bytes=Vec::new();
        fs::File::open(path).map_err(|_|"Cannot open render cache.")?.take(limit as u64+1).read_to_end(&mut bytes).map_err(|_|"Cannot read render cache.")?;
        ensure(bytes.len()<=limit,"Cached render byte budget.")?;Ok(bytes)
    }
    pub fn save(&self, package: &RenderPackage) -> Result<PathBuf,String> {
        let path=self.path(&package.package_hash)?;
        let bytes=serde_json::to_vec(&package.upload()).map_err(|_|"Cannot encode render cache.")?;
        ensure(bytes.len()==package.cache_bytes&&bytes.len()<=CACHE_BYTES,"Cached encoding budget.")?;
        crate::persistence::atomic_write(&path,&bytes)?;Ok(path)
    }
    pub fn load(&self, package: &str, source: &Publication) -> Result<RenderPackage,String> {
        const ENCODED_LIMIT:usize=PACKAGE_BYTES*4/3+32768;
        let bytes=Self::read(&self.path(package)?,ENCODED_LIMIT)?;
        let upload:RenderPackageUpload=serde_json::from_slice(&bytes).map_err(|_|"Malformed render cache.")?;
        ensure(upload.package_hash==package,"Cached render identity mismatch.")?;
        RenderPackage::validate(upload,source)
    }
    pub fn selections(&self)->Result<RenderSelections,String>{
        let path=self.root.join("selected.json");
        if !path.exists(){return Ok(BTreeMap::new());}
        let index:SelectionIndex=serde_json::from_slice(&Self::read(&path,32768)?).map_err(|_|"Malformed render selection index.")?;
        ensure(index.version==1&&index.entries.len()<=64,"Render selection index budget/version.")?;
        let mut result=BTreeMap::new();
        for e in index.entries{
            ensure(digest_id(&e.publication_hash)&&digest_id(&e.profile_hash)&&digest_id(&e.package_hash),"Invalid cached render selection.")?;
            ensure(result.insert((e.publication_hash,e.profile_hash),e.package_hash).is_none(),"Duplicate cached render selection.")?;
        }Ok(result)
    }
    pub fn save_selections(&self,selections:&RenderSelections)->Result<(),String>{
        ensure(selections.len()<=64,"Render selection count budget.")?;
        let entries=selections.iter().map(|((publication,profile),package)|SelectionEntry{publication_hash:publication.clone(),profile_hash:profile.clone(),package_hash:package.clone()}).collect();
        let bytes=serde_json::to_vec(&SelectionIndex{version:1,entries}).map_err(|_|"Cannot encode render selection index.")?;
        ensure(bytes.len()<=32768,"Render selection index byte budget.")?;
        crate::persistence::atomic_write(&self.root.join("selected.json"),&bytes)
    }
    /// Only owned hash-named regular files are candidates; no recursive scan or
    /// indirect path is followed. An externally flooded directory is rejected.
    pub fn inventory(&self)->Result<BTreeMap<String,usize>,String>{
        let entries=match fs::read_dir(&self.root){Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(BTreeMap::new()),Err(_)=>return Err("Cannot inspect render cache.".into())};
        let mut result=BTreeMap::new();
        for (n,entry) in entries.enumerate(){
            ensure(n<512,"Render cache directory entry budget.")?;
            let entry=entry.map_err(|_|"Cannot inspect cached content.")?;
            let name=entry.file_name();let Some(name)=name.to_str() else{continue};
            let Some(stem)=name.strip_suffix(".json") else{continue};
            let package=format!("sha256:{stem}");if !digest_id(&package){continue;}
            let metadata=fs::symlink_metadata(entry.path()).map_err(|_|"Cannot inspect cached file.")?;
            ensure(metadata.is_file()&&!metadata.file_type().is_symlink(),"Indirect cached content is not allowed.")?;
            result.insert(package,metadata.len().min(CACHE_BYTES as u64+1) as usize);
        }Ok(result)
    }
    pub fn remove(&self,package:&str)->Result<(),String>{
        let path=self.path(package)?;
        let metadata=match fs::symlink_metadata(&path){Ok(v)=>v,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(()),Err(_)=>return Err("Cannot inspect cached content for eviction.".into())};
        ensure(metadata.is_file()&&!metadata.file_type().is_symlink(),"Refusing indirect cache eviction.")?;
        fs::remove_file(path).map_err(|_|"Cannot evict cached render content.".into())
    }
}

pub fn profile_bytes(id: &str) -> Result<&'static [u8], String> {
    match id {
        "sunton-3248s035r-render-v1" => Ok(include_bytes!("../../../packages/companion-renderer/test/fixtures/sunton-profile.json")),
        "waveshare-lcd7-s3-render-v1" => Ok(include_bytes!("../../../packages/companion-renderer/test/fixtures/waveshare-profile.json")),
        _ => Err("Unknown qualified render profile.".into()),
    }
}
fn fixed(value: &Value) -> Result<i32, String> {
    let n = value.as_f64().filter(|n| n.is_finite()).ok_or("Invalid source number.")? * 65536.;
    ensure(n.trunc() >= i32::MIN as f64 && n.trunc() <= i32::MAX as f64, "Source range exceeds Q16.16.")?;
    ensure(n.fract()==0.0,"Initial render adapter numeric bounds must be exactly representable as Q16.16.")?;
    Ok(n.trunc() as i32)
}
fn u16_at(b: &[u8], n: usize) -> u16 { u16::from_le_bytes(b[n..n+2].try_into().unwrap()) }
fn u32_at(b: &[u8], n: usize) -> u32 { u32::from_le_bytes(b[n..n+4].try_into().unwrap()) }
fn i32_at(b: &[u8], n: usize) -> i32 { i32::from_le_bytes(b[n..n+4].try_into().unwrap()) }

/// Compare every gameplay field with a separately validated immutable source.
/// Initial lowering supports source schema 2 momentary buttons and numeric/bool
/// feedback. Unsupported source contracts fail instead of being narrowed.
pub fn verify_source_semantics(bytes: &[u8], source: &Publication) -> Result<(), String> {
    let sections = inspect_render_object(RenderObjectKind::Semantics, bytes, 16 * 1024)?;
    let section = |kind| sections.iter().find(|s| s.kind == kind).map(|s| &bytes[s.bytes.clone()]).unwrap_or(&[]);
    let strings = section(4);
    let name = |record: &[u8], at: usize| -> Result<&str, String> {
        let offset = u32_at(record, at) as usize; let length = u16_at(record, at+4) as usize;
        ensure(offset <= strings.len() && length <= strings.len()-offset, "Semantic string bounds.")?;
        std::str::from_utf8(&strings[offset..offset+length]).map_err(|_| "Semantic UTF-8.".into())
    };
    let doc: Value = serde_json::from_str(&source.canonical_document).map_err(|_| "Invalid trusted source.")?;
    ensure(doc["schemaVersion"] == 2, "Native render lowering currently requires source schema 2.")?;
    let controls = doc["ui"]["controls"].as_array().ok_or("Missing source controls.")?;
    let outputs = doc["ui"]["feedbackWidgets"].as_array().ok_or("Missing source outputs.")?;
    let widgets = doc["presentation"]["widgets"].as_array().ok_or("Missing source widgets.")?;
    let bindings = doc["bindings"].as_array().ok_or("Missing source bindings.")?;
    let records = section(1); let output_records = section(2); let interactions = section(3);
    ensure(records.len() == controls.len()*32 && output_records.len() == outputs.len()*32, "Compiled logical graph differs from source.")?;
    let mut owners = BTreeSet::new();
    for (index, c) in controls.iter().enumerate() {
        let r = &records[index*32..index*32+32];
        let binding = bindings.iter().find(|b| b["controlId"] == c["id"]).ok_or("Source target missing.")?;
        let owner = widgets.iter().find(|w| w["controlIds"].as_array().is_some_and(|ids| ids.contains(&c["id"]))).ok_or("Source owner missing.")?;
        ensure(c["kind"] == "button" && owner["kind"] == "button" && owner["config"]["buttonMode"] == "momentary", "Input transform is not qualified for this renderer.")?;
        ensure(name(r, 0)? == c["id"].as_str().unwrap_or("") && r[6] == 1 && r[7] == 0 &&
            name(r, 8)? == binding["targetId"].as_str().unwrap_or("") && Some(u16_at(r,14) as u64) == binding["index"].as_u64() &&
            i32_at(r,16) == 0 && i32_at(r,20) == 65536 && i32_at(r,24) == 0 && u16_at(r,30) == u16::MAX,
            "Compiled control differs from trusted source.")?;
        let oi = u16_at(r,28) as usize;
        ensure(oi < interactions.len()/24 && owners.insert(oi), "Invalid interaction ownership.")?;
        let ir = &interactions[oi*24..oi*24+24];
        ensure(name(ir,0)? == owner["id"].as_str().unwrap_or("") && ir[6] == 1 && ir[7] == 0 &&
            u16_at(ir,8) as usize == index && u16_at(ir,10) == u16::MAX && u16_at(ir,12) == u16::MAX &&
            u16_at(ir,14) == u16::MAX && ir[16..].iter().all(|x| *x == 0), "Interaction differs from source.")?;
    }
    ensure(owners.len() == interactions.len()/24, "Untrusted extra interaction.")?;
    for (index, output) in outputs.iter().enumerate() {
        let r = &output_records[index*32..index*32+32];
        let widget = widgets.iter().find(|w| w["feedbackIds"].as_array().is_some_and(|ids| ids.contains(&output["id"]))).ok_or("Source output owner missing.")?;
        let (kind, min, max, unit) = match output["kind"].as_str() {
            Some("indicator") => (1,0,65536,""),
            Some("gauge") => (2,fixed(&widget["config"]["min"])?,fixed(&widget["config"]["max"])?,widget["config"]["unit"].as_str().unwrap_or("")),
            _ => return Err("Source text capacity requires an explicit compact feedback contract.".into()),
        };
        ensure(name(r,0)? == output["id"].as_str().unwrap_or("") && r[6] == kind && r[7] == 0 &&
            name(r,8)? == output["channelId"].as_str().unwrap_or("") && u16_at(r,14) == 0 &&
            i32_at(r,16) == min && i32_at(r,20) == max && name(r,24)? == unit && u16_at(r,30) == 0,
            "Compiled feedback contract differs from source.")?;
    }
    Ok(())
}

impl RenderPackage {
    pub fn validate(upload: RenderPackageUpload, source: &Publication) -> Result<Self, String> {
        ensure(upload.manifest.len() <= 8192 && digest_id(&upload.package_hash), "Render manifest budget/hash.")?;
        let manifest: RenderManifest = serde_json::from_str(&upload.manifest).map_err(|_| "Invalid render manifest.")?;
        ensure(serde_json_canonicalizer::to_vec(&manifest).map_err(|_| "Manifest encoding.")? == upload.manifest.as_bytes(), "Render manifest must be exact canonical JSON.")?;
        ensure(format!("sha256:{}",hash(upload.manifest.as_bytes())) == upload.package_hash, "Package hash mismatch.")?;
        ensure(manifest.format_version == 1 && manifest.renderer_abi == 1, "Unsupported render ABI.")?;
        ensure(manifest.source_ui_id == source.ui_id && manifest.source_publication_hash == source.publication_hash && manifest.semantic_fingerprint == source.capability_fingerprint, "Render package source authority mismatch.")?;
        let profile_raw = profile_bytes(&manifest.target_profile_id)?;
        ensure(manifest.target_profile_hash == format!("sha256:{}",hash(profile_raw)), "Profile hash mismatch.")?;
        let profile: Value = serde_json::from_slice(profile_raw).map_err(|_| "Invalid compiled profile.")?;
        let limits = &profile["limits"];
        let cap = |name: &str, value: u64| ensure(limits[name].as_u64().is_some_and(|n| value <= n), &format!("Render {name} budget exceeded."));
        ensure(manifest.viewport.width as u64 == profile["width"].as_u64().unwrap_or(0) && manifest.viewport.height as u64 == profile["height"].as_u64().unwrap_or(0) && serde_json::to_value(manifest.viewport.content).unwrap() == profile["content"], "Render viewport mismatch.")?;
        let mut previous = "";
        for feature in &manifest.requires {
            ensure(feature.as_str() > previous && profile["features"].as_array().is_some_and(|v|v.contains(&Value::String(feature.clone()))), "Unsupported/unsorted render feature.")?; previous = feature;
        }
        ensure(manifest.requires.len() <= 32 && (2..=4).contains(&manifest.objects.len()) && upload.objects.len() == manifest.objects.len(), "Render object/feature count.")?;
        let counts = serde_json::to_value(&manifest.counts).unwrap();
        for (name, value) in counts.as_object().unwrap() { cap(name, value.as_u64().unwrap())?; }
        let resources = serde_json::to_value(&manifest.resources).unwrap();
        for name in ["internalRamBytes","psramBytes","transferBytes","maxUpdateInstructions","maxDirtyPixels","maxLayersPerTile","maxBlendPixelLayers"] { cap(name, resources[name].as_u64().unwrap())?; }
        let memory = &manifest.resources;
        ensure(memory.transfer_bytes == 2048 && memory.tile_bytes >= 1024 && memory.internal_ram_bytes as u64 >= memory.snapshot_bytes as u64 + memory.tile_bytes as u64 + memory.transfer_bytes as u64 && memory.internal_ram_bytes as u64 + memory.psram_bytes as u64 >= memory.scene_ram_bytes as u64 + memory.snapshot_bytes as u64 + memory.tile_bytes as u64 + memory.transfer_bytes as u64 + memory.cache_bytes as u64, "Render memory accounting.")?;
        let mut objects = BTreeMap::new(); let mut total = upload.manifest.len(); let mut prior = None;
        for (i, d) in manifest.objects.iter().enumerate() {
            let kind = OBJECT_KINDS.iter().position(|k| *k == d.kind).ok_or("Unknown render object.")?;
            ensure((i > 1 || kind == i) && prior.is_none_or(|p| kind > p) && d.version == 1 && digest_id(&d.sha256), "Render object ordering/version/hash.")?;
            prior = Some(kind);
            ensure(d.bytes as usize <= PACKAGE_BYTES && d.bytes >= 32, "Render object byte limit.")?;
            if kind == 0 { cap("semanticBytes", d.bytes as u64)?; }
            if kind == 1 { cap("sceneBytes", d.bytes as u64)?; }
            let encoded = upload.objects.get(&d.kind).ok_or("Missing render object.")?;
            ensure(encoded.len() == (d.bytes as usize).div_ceil(3)*4, "Encoded render object limit.")?;
            let bytes = STANDARD.decode(encoded).map_err(|_| "Invalid render object encoding.")?;
            ensure(bytes.len() == d.bytes as usize && STANDARD.encode(&bytes) == *encoded && format!("sha256:{}", hash(&bytes)) == d.sha256, "Render object integrity mismatch.")?;
            inspect_render_object([RenderObjectKind::Semantics,RenderObjectKind::Scene,RenderObjectKind::Images,RenderObjectKind::Fonts][kind], &bytes, d.bytes as usize)?;
            total += bytes.len(); ensure(total <= PACKAGE_BYTES, "Package total limit.")?;
            objects.insert(d.kind.clone(), Arc::<[u8]>::from(bytes));
        }
        cap("manifestBytes",upload.manifest.len() as u64)?; cap("packageBytes",total as u64)?;
        ensure(total-upload.manifest.len() == memory.object_bytes as usize && manifest.semantic_descriptor_hash == manifest.objects[0].sha256, "Render object accounting/semantic hash.")?;
        verify_source_semantics(&objects["semantics"], source)?;
        let cache_bytes=serde_json::to_vec(&upload).map_err(|_|"Cannot encode checked render package.")?.len();
        Ok(Self { manifest,manifest_bytes:Arc::from(upload.manifest.into_bytes()),package_hash:upload.package_hash,objects,bytes:total,cache_bytes,source_document:Arc::new(serde_json::from_str(&source.canonical_document).map_err(|_|"Invalid trusted source document.")?) })
    }
    fn upload(&self)->RenderPackageUpload{
        RenderPackageUpload{manifest:String::from_utf8_lossy(&self.manifest_bytes).into_owned(),package_hash:self.package_hash.clone(),objects:self.objects.iter().map(|(k,v)|(k.clone(),STANDARD.encode(v))).collect()}
    }
    pub fn chunk(&self, kind: &str, offset: usize, requested: usize) -> Result<Value, String> {
        ensure(requested > 0 && requested <= CHUNK_BYTES, "Content chunk byte budget.")?;
        let bytes: &[u8] = if kind == "manifest" { &self.manifest_bytes } else { self.objects.get(kind).ok_or("Object is not in prepared package.")? };
        ensure(offset < bytes.len() && offset % CHUNK_BYTES == 0, "Content offset outside object/alignment.")?;
        let count = requested.min(bytes.len()-offset);
        Ok(serde_json::json!({"kind":kind,"offset":offset,"bytes":count,"totalBytes":bytes.len(),"base64":STANDARD.encode(&bytes[offset..offset+count])}))
    }
}

#[cfg(test)]
mod tests {
    use crate as osg_companion_service;
    include!("../tests/render_delivery.rs");
}
