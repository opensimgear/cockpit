//! Read-only Battle.net discovery from the launcher's local protobuf database.
use super::{Candidate, Scan, canonical_text, clean_name, executable_candidates, regular};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_DATABASE_BYTES: u64 = 1024 * 1024;
const MAX_PRODUCTS: usize = 512;

#[path = "battlenet_artwork.rs"]
mod artwork;

#[derive(Default)]
struct Product {
    uid: String,
    code: String,
    install_path: String,
    installed: Option<bool>,
}

/// Scan the standard ProgramData location and common product database roots.
pub(super) fn scan(scan: &mut Scan) {
    let mut paths = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .into_iter()
        .map(|root| root.join("Battle.net/Agent/product.db"))
        .collect::<Vec<_>>();
    paths.push(PathBuf::from(r"C:\ProgramData\Battle.net\Agent\product.db"));
    let first = scan.candidates.len();
    scan_paths(&paths, scan);
    artwork::assign(&mut scan.candidates[first..], &artwork::cache_roots());
}

/// Scan product database files. Kept path-based so tests can exercise real
/// protobuf fixtures without depending on process environment or the client.
pub(super) fn scan_paths(paths: &[PathBuf], scan: &mut Scan) {
    let mut seen = BTreeSet::new();
    for path in paths.iter().take(8) {
        if !regular(path, false) {
            continue;
        }
        let Some(bytes) = read_database(path) else {
            scan.diagnostic(format!(
                "Battle.net product database could not be read: {}",
                path.display()
            ));
            continue;
        };
        let Some(products) = decode_database(&bytes) else {
            scan.diagnostic(format!(
                "Battle.net product database is malformed: {}",
                path.display()
            ));
            continue;
        };
        for product in products.into_iter().take(MAX_PRODUCTS) {
            if scan.candidates.len() >= MAX_PRODUCTS {
                return;
            }
            let code = product.code.trim();
            if !valid_id(code)
                || matches!(code.to_ascii_lowercase().as_str(), "agent" | "bna")
                || product.installed != Some(true)
            {
                continue;
            }
            let install = PathBuf::from(product.install_path.trim());
            if !install.is_absolute() || !regular(&install, true) {
                continue;
            }
            let Some(install_path) = canonical_text(&install) else {
                continue;
            };
            if !seen.insert(code.to_owned()) {
                continue;
            }
            let known_name = known_name(code);
            let name = known_name
                .and_then(clean_name)
                .or_else(|| {
                    install
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(clean_name)
                })
                .unwrap_or_else(|| code.to_owned());
            scan.candidates.push(Candidate {
                id: format!("battlenet:{code}"),
                client: "battlenet".into(),
                name,
                install_path,
                executable_candidates: executable_candidates(&install, None),
                preferred_executable: None,
                game_id: None,
                kind: if known_name.is_some() {
                    "game"
                } else {
                    "unknown"
                }
                .into(),
                app_id: code.to_owned(),
                launcher: None,
                artwork: None,
            });
            let _ = product.uid; // Retained as schema metadata; product code is the stable public id.
        }
    }
}

fn read_database(path: &Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_DATABASE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_DATABASE_BYTES).then_some(bytes)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}

fn known_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "s1" => "StarCraft",
        "s2" => "StarCraft II",
        "wow" => "World of Warcraft",
        "wow_classic" => "World of Warcraft Classic",
        "wow_classic_era" => "World of Warcraft Classic Era",
        "pro" | "prometheus" => "Overwatch 2",
        "w1" => "Warcraft: Orcs & Humans",
        "w1r" => "Warcraft I: Remastered",
        "w2bn" => "Warcraft II: Battle.net Edition",
        "w2r" => "Warcraft II: Remastered",
        "w3" => "Warcraft III: Reforged",
        "gryphon" => "Warcraft Rumble",
        "hsb" | "WTCG" => "Hearthstone",
        "hero" => "Heroes of the Storm",
        "d3cn" => "Diablo III",
        "d3" => "Diablo III",
        "fenris" => "Diablo IV",
        "viper" => "Call of Duty: Black Ops 4",
        "odin" => "Call of Duty: Modern Warfare",
        "lazarus" => "Call of Duty: MW2 Campaign Remastered",
        "zeus" => "Call of Duty: Black Ops Cold War",
        "auks" => "Call of Duty: Modern Warfare II",
        "codhq" => "Call of Duty HQ",
        "rtro" => "Blizzard Arcade Collection",
        "wlby" => "Crash Bandicoot 4: It's About Time",
        "osi" => "Diablo II: Resurrected",
        "fore" => "Call of Duty: Vanguard",
        "d2" => "Diablo II",
        "d2LOD" => "Diablo II: Lord of Destruction",
        "w3ROC" => "Warcraft III: Reign of Chaos",
        "w3tft" => "Warcraft III: The Frozen Throne",
        "sca" => "StarCraft Anthology",
        "anbs" => "Diablo Immortal",
        _ => return None,
    })
}

pub(super) fn artwork_choices(code: &str, roots: &[PathBuf]) -> Vec<(String, PathBuf)> {
    artwork::choices(roots, code)
}

pub(super) fn artwork_cache_roots() -> Vec<PathBuf> {
    artwork::cache_roots()
}

/// Minimal protobuf wire reader: supports varint and length-delimited fields,
/// skips all legal scalar wire kinds, rejects malformed/truncated messages.
#[derive(Clone, Copy)]
struct Field<'a> {
    number: u32,
    wire: u8,
    value: FieldValue<'a>,
}
#[derive(Clone, Copy)]
enum FieldValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed,
}

fn varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        if shift == 63 && byte > 1 {
            return None;
        }
        result |= u64::from(byte & 0x7f).checked_shl(shift)?;
        if byte < 0x80 {
            return Some(result);
        }
    }
    None
}

fn fields(bytes: &[u8]) -> Option<Vec<Field<'_>>> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < bytes.len() {
        let key = varint(bytes, &mut pos)?;
        let number = u32::try_from(key >> 3).ok()?;
        let wire = (key & 7) as u8;
        if number == 0 {
            return None;
        }
        let value = match wire {
            0 => FieldValue::Varint(varint(bytes, &mut pos)?),
            1 => {
                pos = pos.checked_add(8)?;
                if pos > bytes.len() {
                    return None;
                }
                FieldValue::Fixed
            }
            2 => {
                let len = usize::try_from(varint(bytes, &mut pos)?).ok()?;
                let end = pos.checked_add(len)?;
                let value = bytes.get(pos..end)?;
                pos = end;
                FieldValue::Bytes(value)
            }
            5 => {
                pos = pos.checked_add(4)?;
                if pos > bytes.len() {
                    return None;
                }
                FieldValue::Fixed
            }
            _ => return None,
        };
        if out.len() >= 100_000 {
            return None;
        }
        out.push(Field {
            number,
            wire,
            value,
        });
    }
    Some(out)
}

fn one_bytes<'a>(fields: &[Field<'a>], number: u32) -> Option<&'a [u8]> {
    let mut found = fields.iter().filter(|field| field.number == number);
    let field = found.next()?;
    if found.next().is_some() || field.wire != 2 {
        return None;
    }
    match field.value {
        FieldValue::Bytes(value) => Some(value),
        _ => None,
    }
}

fn one_bool(fields: &[Field<'_>], number: u32) -> Option<bool> {
    let mut found = fields.iter().filter(|field| field.number == number);
    let field = found.next()?;
    if found.next().is_some() || field.wire != 0 {
        return None;
    }
    match field.value {
        FieldValue::Varint(value) if value <= 1 => Some(value == 1),
        _ => None,
    }
}

fn utf8(value: &[u8]) -> Option<String> {
    let value = std::str::from_utf8(value).ok()?.trim();
    (!value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn decode_database(bytes: &[u8]) -> Option<Vec<Product>> {
    let top = fields(bytes)?;
    let mut products = Vec::new();
    for field in top.iter().filter(|field| field.number == 1) {
        if field.wire != 2 {
            return None;
        }
        let FieldValue::Bytes(record) = field.value else {
            return None;
        };
        if products.len() >= MAX_PRODUCTS {
            break;
        }
        let record = fields(record)?;
        let uid = one_bytes(&record, 1).and_then(utf8).unwrap_or_default();
        let code = one_bytes(&record, 2).and_then(utf8).unwrap_or_default();
        let install_path = one_bytes(&record, 3)
            .and_then(fields)
            .and_then(|settings| one_bytes(&settings, 1).and_then(utf8))
            .unwrap_or_default();
        let installed = one_bytes(&record, 4)
            .and_then(fields)
            .and_then(|cached| one_bytes(&cached, 1).and_then(fields))
            .and_then(|state| one_bool(&state, 1));
        products.push(Product {
            uid,
            code,
            install_path,
            installed,
        });
    }
    Some(products)
}

#[cfg(test)]
#[path = "battlenet_tests.rs"]
mod tests;
