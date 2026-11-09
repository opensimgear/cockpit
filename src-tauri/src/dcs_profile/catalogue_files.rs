use super::SOURCE_CHANGED;
use crate::dcs_bios;
use ring::digest::{digest, Digest, SHA256};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CatalogueFile {
    pub(super) length: u64,
    pub(super) modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

#[cfg(test)]
#[test]
fn content_proof_accepts_equivalent_json_with_preserved_metadata() {
    let fixture = crate::test_directory();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/dcs-bios");
    for name in [
        "A-10C.json",
        "AircraftAliases.json",
        "MetadataStart.json",
        "MetadataEnd.json",
    ] {
        fs::copy(source.join(name), fixture.path().join(name)).unwrap();
    }
    let changed = fixture.path().join("A-10C.json");
    let mut bytes = vec![b' '];
    bytes.extend(fs::read(&changed).unwrap());
    fs::write(&changed, &bytes).unwrap();
    let guard = super::TelemetryCatalogue::load(fixture.path(), "A-10C")
        .unwrap()
        .guard();
    let before = catalogue_file(&changed).unwrap().unwrap();

    bytes[0] = b'\n';
    fs::write(&changed, &bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(&changed)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified.unwrap()))
        .unwrap();

    assert_eq!(catalogue_file(&changed).unwrap(), Some(before));
    assert!(!guard.files.matches_bytes().unwrap());
    guard.validate_content().unwrap();
}

pub(super) fn catalogue_file(path: &Path) -> Result<Option<CatalogueFile>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("DCS source catalogues must be regular files, not symbolic links.".into());
    }
    Ok(Some(CatalogueFile {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    }))
}

#[derive(Clone)]
struct CapturedFile {
    metadata: CatalogueFile,
    digest: Digest,
}

#[derive(Clone, Default)]
pub(super) struct CatalogueFiles(Vec<(PathBuf, Option<CapturedFile>)>);

impl CatalogueFiles {
    pub(super) fn read(&mut self, path: PathBuf, optional: bool) -> Result<Option<Value>, String> {
        let before = catalogue_file(&path)?;
        if before.is_none() && optional {
            self.0.push((path, None));
            return Ok(None);
        }
        let bytes = dcs_bios::read_document(&path, dcs_bios::MAX_DOCUMENT)?;
        let value = dcs_bios::parse_json(&path, &bytes)?;
        if catalogue_file(&path)? != before {
            return Err(SOURCE_CHANGED.into());
        }
        let metadata = before.ok_or(SOURCE_CHANGED)?;
        self.0.push((
            path,
            Some(CapturedFile {
                metadata,
                digest: digest(&SHA256, &bytes),
            }),
        ));
        Ok(Some(value))
    }

    pub(super) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.iter().map(|(path, _)| path)
    }

    pub(super) fn validate_metadata(&self) -> Result<(), String> {
        for (path, captured) in &self.0 {
            if catalogue_file(path)?.as_ref() != captured.as_ref().map(|file| &file.metadata) {
                return Err(SOURCE_CHANGED.into());
            }
        }
        Ok(())
    }

    /// A byte match proves the exact content parsed during capture. Callers
    /// check metadata before and after the read, including absent optional files.
    pub(super) fn matches_bytes(&self) -> Result<bool, String> {
        for (path, captured) in &self.0 {
            if let Some(file) = captured {
                let bytes = dcs_bios::read_document(path, dcs_bios::MAX_DOCUMENT)?;
                if digest(&SHA256, &bytes).as_ref() != file.digest.as_ref() {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
