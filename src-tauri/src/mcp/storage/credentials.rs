//! Machine-local MCP settings contain credential hashes, never bearer tokens.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 54438;
const LIMIT: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Credential {
    pub id: String,
    pub name: String,
    pub scope: String,
    pub created_at: u64,
    pub hash: String,
}
impl Credential {
    pub fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"id":self.id,"name":self.name,"scope":self.scope,"createdAt":self.created_at})
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub port: u16,
    pub credentials: Vec<Credential>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            credentials: vec![],
        }
    }
}

pub struct Store {
    repository:Option<osg_persistence::Repository<Settings>>,
    pub settings:Settings,
    pub revision:u64,
    pub error:Option<String>,
}
impl Store {
    pub fn open(path:Option<PathBuf>)->Self {
        let mut store=Self{repository:None,settings:Settings::default(),revision:0,error:None};
        if let Some(path)=path {
            let opened=(||{
                let repository=osg_persistence::Repository::new(osg_persistence::Database::for_namespace(&path)?,"security","mcp",LIMIT,validate)?;
                let(revision,settings)=repository.load_snapshot()?;
                Ok::<_,String>((repository,revision,settings.unwrap_or_default()))
            })();
            match opened {Ok((repository,revision,settings))=>{store.repository=Some(repository);store.revision=revision;store.settings=settings;},Err(error)=>store.error=Some(error)}
        }
        store
    }
    pub fn refresh(&mut self)->Result<(),String>{
        if let Some(repository)=&self.repository {
            match repository.load_snapshot(){
                Ok((revision,settings))=>{self.settings=settings.unwrap_or_default();self.revision=revision;self.error=None;},
                Err(error)=>{self.error=Some(error.clone());return Err(error);}
            }
        }else if let Some(error)=&self.error{return Err(error.clone());}
        Ok(())
    }
    pub fn check_revision(&self,expected:u64)->Result<(),String>{
        if expected!=self.revision{return Err("MCP settings revision conflict. Reload before saving.".into());}Ok(())
    }
    pub fn save(&mut self,settings:Settings)->Result<(),String>{
        if self.error.is_some(){return Err("Repair the existing SQLite MCP settings before changing them.".into());}
        validate(&settings)?;
        if serde_json::to_vec(&settings).map_err(|_|"Cannot encode MCP settings.")?.len()>LIMIT{return Err("MCP settings exceed 64 KiB.".into());}
        let next=self.revision.checked_add(1).ok_or("MCP settings revision exhausted.")?;
        if let Some(repository)=&self.repository{repository.save(&settings)?;}
        self.settings=settings;self.revision=next;Ok(())
    }
}
pub fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
pub fn matches(token: &str, hash: &str) -> bool {
    let actual = digest(token);
    if actual.len() != hash.len() {
        return false;
    }
    actual
        .bytes()
        .zip(hash.bytes())
        .fold(0u8, |different, (a, b)| different | (a ^ b))
        == 0
}
fn validate(settings: &Settings) -> Result<(), String> {
    if settings.port < 1024 || settings.credentials.len() > 32 {
        return Err("Unsupported or invalid MCP settings.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut hashes = std::collections::BTreeSet::new();
    for credential in &settings.credentials {
        if uuid::Uuid::parse_str(&credential.id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(credential.id.as_str())
            || !ids.insert(&credential.id)
            || !hashes.insert(&credential.hash)
            || credential.name.trim().is_empty()
            || credential.name.len() > 80
            || credential.name.chars().any(char::is_control)
            || !matches!(credential.scope.as_str(), "read" | "write")
            || credential.hash.len() != 64
            || !credential.hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("Invalid MCP credential metadata.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path="credential_tests.rs"]
mod tests;
