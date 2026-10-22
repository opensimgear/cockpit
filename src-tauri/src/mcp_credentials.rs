//! Machine-local MCP settings contain credential hashes, never bearer tokens.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const DEFAULT_PORT: u16 = 54438;
const FILE_LIMIT: u64 = 65_536;

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    pub enabled: bool,
    pub port: u16,
    pub credentials: Vec<Credential>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            enabled: false,
            port: DEFAULT_PORT,
            credentials: vec![],
        }
    }
}

pub struct Store {
    path: Option<PathBuf>,
    pub settings: Settings,
    pub error: Option<String>,
}
impl Store {
    pub fn open(path: Option<PathBuf>) -> Self {
        match path
            .as_ref()
            .map_or_else(|| Ok(Settings::default()), |p| load(p))
        {
            Ok(settings) => Self {
                path,
                settings,
                error: None,
            },
            Err(error) => Self {
                path,
                settings: Settings::default(),
                error: Some(error),
            },
        }
    }
    pub fn save(&mut self, settings: Settings) -> Result<(), String> {
        if self.error.is_some() {
            return Err("Repair the existing MCP settings file before changing it.".into());
        }
        validate(&settings)?;
        let bytes = serde_json::to_vec(&settings).map_err(|e| e.to_string())?;
        if bytes.len() > FILE_LIMIT as usize {
            return Err("MCP settings exceed 64 KiB.".into());
        }
        if let Some(path) = &self.path {
            safe_path(path)?;
            let parent = path.parent().ok_or("Missing MCP settings directory.")?;
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot create MCP settings directory: {e}"))?;
            safe_path(path)?;
            let mut temporary =
                tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temporary
                    .as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
            temporary.write_all(&bytes).map_err(|e| e.to_string())?;
            temporary.as_file().sync_all().map_err(|e| e.to_string())?;
            temporary
                .persist(path)
                .map_err(|e| format!("Cannot save MCP settings: {e}"))?;
        }
        self.settings = settings;
        Ok(())
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
    if settings.schema != 1 || settings.port < 1024 || settings.credentials.len() > 32 {
        return Err("Unsupported or invalid MCP settings.".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    for credential in &settings.credentials {
        if uuid::Uuid::parse_str(&credential.id)
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(credential.id.as_str())
            || !ids.insert(&credential.id)
            || credential.name.trim().is_empty()
            || credential.name.len() > 80
            || !matches!(credential.scope.as_str(), "read" | "write")
            || credential.hash.len() != 64
            || !credential.hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("Invalid MCP credential metadata.".into());
        }
    }
    Ok(())
}
fn safe_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("MCP settings require an absolute local path.".into());
    }
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    meta.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let reparse = false;
                if meta.file_type().is_symlink()
                    || reparse
                    || (ancestor == path && !meta.is_file())
                    || (ancestor != path && !meta.is_dir())
                {
                    return Err("MCP settings require a regular file and directories without links or reparse points.".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
fn load(path: &Path) -> Result<Settings, String> {
    safe_path(path)?;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = vec![];
    (&mut file)
        .take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > FILE_LIMIT as usize {
        return Err("MCP settings exceed 64 KiB.".into());
    }
    let settings =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid MCP settings: {e}"))?;
    validate(&settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_hashed_and_corrupt_configuration_is_preserved() {
        let root = crate::test_directory();
        let path = root.path().join("mcp.json");
        let mut store = Store::open(Some(path.clone()));
        assert!(!store.settings.enabled);
        let mut next = store.settings.clone();
        next.credentials.push(Credential {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Assistant".into(),
            scope: "read".into(),
            created_at: 1,
            hash: digest("secret-token"),
        });
        store.save(next).unwrap();
        assert!(!std::fs::read_to_string(&path)
            .unwrap()
            .contains("secret-token"));
        assert!(matches("secret-token", &store.settings.credentials[0].hash));
        assert!(!matches("different", &store.settings.credentials[0].hash));
        assert_eq!(
            Store::open(Some(path.clone())).settings.credentials.len(),
            1
        );
        std::fs::write(&path, "invalid").unwrap();
        let mut damaged = Store::open(Some(path.clone()));
        assert!(damaged.error.is_some());
        assert!(damaged.save(Settings::default()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid");
    }
    #[test]
    fn rejects_duplicate_credentials_and_invalid_metadata() {
        let mut store = Store::open(None);
        let credential = Credential {
            id: uuid::Uuid::new_v4().to_string(),
            name: "x".into(),
            scope: "write".into(),
            created_at: 0,
            hash: digest("x"),
        };
        let mut next = store.settings.clone();
        next.credentials = vec![credential.clone(), credential];
        assert!(store.save(next).is_err());
        assert!(store.settings.credentials.is_empty());
    }
}
