use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

pub const MAX_INTERFACES: usize = 256;
pub const MAX_EVDEV: usize = 1024;
pub const MAX_DESCRIPTOR: usize = 65_536;
pub const MAX_DESCRIPTOR_TOTAL: usize = 8 * 1024 * 1024;
pub const MAX_FAILURES: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: String,
    pub stage: String,
    pub message: String,
    pub os_code: Option<i32>,
}
impl Failure {
    pub(super) fn new(kind: &str, stage: &str, message: &str) -> Self {
        Self {
            kind: kind.into(),
            stage: stage.into(),
            message: message.into(),
            os_code: None,
        }
    }
    pub(super) fn io(stage: &str, error: std::io::Error, stale: bool) -> Self {
        let kind = match error.kind() {
            std::io::ErrorKind::PermissionDenied => "permission-denied",
            std::io::ErrorKind::NotFound if stale => "stale-generation",
            std::io::ErrorKind::NotFound => "unavailable",
            _ => "io",
        };
        Self {
            kind: kind.into(),
            stage: stage.into(),
            message: error.to_string(),
            os_code: error.raw_os_error(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    #[serde(serialize_with = "decimal")]
    pub device: u64,
    #[serde(serialize_with = "decimal")]
    pub inode: u64,
    #[serde(serialize_with = "decimal")]
    pub rdev: u64,
    #[serde(serialize_with = "decimal")]
    pub change_seconds: i64,
    #[serde(serialize_with = "decimal")]
    pub change_nanos: i64,
    pub character_device: bool,
    pub mode: u32,
}
fn decimal<T: std::fmt::Display, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationEvidence {
    pub node: Stamp,
    pub parent: Stamp,
    pub hid_parent: PathBuf,
    pub uevent_sha256: String,
    pub descriptor_sha256: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evdev {
    pub path: PathBuf,
    pub sysfs_path: PathBuf,
    pub hid_parent: PathBuf,
    pub joystick: bool,
    pub keyboard: bool,
    pub mouse: bool,
    pub system: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    pub interface_id: String,
    pub path: PathBuf,
    pub sysfs_path: PathBuf,
    pub hid_parent: PathBuf,
    pub hid_ancestry: Vec<PathBuf>,
    pub transport: String,
    pub vendor_id: Option<u32>,
    pub product_id: Option<u32>,
    pub name: Option<String>,
    pub physical: Option<String>,
    pub unique: Option<String>,
    pub udev_properties: BTreeMap<String, String>,
    pub evdev: Vec<Evdev>,
    pub evdev_paths: Vec<PathBuf>,
    pub restricted: bool,
    pub restriction_reasons: Vec<String>,
    pub read_access: Option<bool>,
    pub descriptor_sha256: Option<String>,
    pub failures: Vec<Failure>,
    #[serde(skip)]
    pub descriptor: Option<Vec<u8>>,
    #[serde(skip)]
    pub evidence: Option<GenerationEvidence>,
    #[serde(skip)]
    pub(super) roots: Roots,
}
impl Interface {
    pub(super) fn fail(&mut self, failure: Failure) {
        if self.failures.len() < MAX_FAILURES {
            self.failures.push(failure);
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub interfaces: Vec<Interface>,
    pub failures: Vec<Failure>,
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct Roots {
    pub sys: PathBuf,
    pub dev: PathBuf,
    pub udev_data: PathBuf,
    pub native_udev: bool,
}
impl Default for Roots {
    fn default() -> Self {
        Self {
            sys: "/sys".into(),
            dev: "/dev".into(),
            udev_data: "/run/udev/data".into(),
            native_udev: true,
        }
    }
}
