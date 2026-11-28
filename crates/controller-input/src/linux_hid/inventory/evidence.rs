use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
};

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(super) fn read(
    path: &Path,
    maximum: usize,
    stage: &str,
    stale: bool,
) -> Result<Vec<u8>, Failure> {
    let file = File::open(path).map_err(|e| Failure::io(stage, e, stale))?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Failure::io(stage, e, stale))?;
    if bytes.len() > maximum {
        return Err(Failure::new(
            "capacity",
            stage,
            "Metadata file exceeds its bounded size",
        ));
    }
    Ok(bytes)
}
pub(super) fn stamp(meta: &Metadata) -> Stamp {
    Stamp {
        device: meta.dev(),
        inode: meta.ino(),
        rdev: meta.rdev(),
        change_seconds: meta.ctime(),
        change_nanos: meta.ctime_nsec(),
        character_device: meta.file_type().is_char_device(),
        mode: meta.mode(),
    }
}
pub(super) fn metadata(path: &Path, stage: &str, stale: bool) -> Result<Stamp, Failure> {
    fs::symlink_metadata(path)
        .map(|m| stamp(&m))
        .map_err(|e| Failure::io(stage, e, stale))
}
pub(super) fn hid_parent(start: &Path, roots: &Roots, stale: bool) -> Result<PathBuf, Failure> {
    let sys = fs::canonicalize(&roots.sys).map_err(|e| Failure::io("sysfs-root", e, stale))?;
    let path = fs::canonicalize(start).map_err(|e| Failure::io("hid-parent", e, stale))?;
    if !path.starts_with(&sys) {
        return Err(Failure::new(
            if stale {
                "stale-generation"
            } else {
                "malformed"
            },
            "hid-parent",
            "Device ancestry escapes the canonical sysfs root",
        ));
    }
    for parent in path.ancestors().take(128) {
        if let Ok(subsystem) = fs::canonicalize(parent.join("subsystem")) {
            if subsystem == sys.join("bus/hid") {
                return Ok(parent.to_path_buf());
            }
        }
        if parent == sys {
            break;
        }
    }
    Err(Failure::new(
        if stale {
            "stale-generation"
        } else {
            "unavailable"
        },
        "hid-parent",
        "No canonical HID subsystem parent could be established",
    ))
}
pub(super) fn current(
    interface: &Interface,
    descriptor: &[u8],
    stale: bool,
) -> Result<GenerationEvidence, Failure> {
    if descriptor.is_empty() || descriptor.len() > MAX_DESCRIPTOR {
        return Err(Failure::new(
            "malformed",
            "descriptor",
            "Descriptor identity is unavailable or exceeds its bound",
        ));
    }
    let parent = hid_parent(
        &interface.sysfs_path.join("device"),
        &interface.roots,
        stale,
    )?;
    let uevent = read(&parent.join("uevent"), 16_384, "hid-uevent", stale)?;
    let node = metadata(&interface.path, "device-node", stale)?;
    let declared = read(&interface.sysfs_path.join("dev"), 64, "sysfs-dev", stale)?;
    let declared = std::str::from_utf8(&declared)
        .ok()
        .and_then(|s| s.trim().split_once(':'))
        .and_then(|(major, minor)| Some((major.parse::<u32>().ok()?, minor.parse::<u32>().ok()?)))
        .map(|(major, minor)| libc::makedev(major, minor));
    if declared != Some(node.rdev) {
        return Err(Failure::new(
            if stale {
                "stale-generation"
            } else {
                "malformed"
            },
            "sysfs-dev",
            "Device node major/minor differs from its canonical sysfs class",
        ));
    }
    Ok(GenerationEvidence {
        node,
        parent: metadata(&parent, "hid-parent-metadata", stale)?,
        hid_parent: parent,
        uevent_sha256: digest(&uevent),
        descriptor_sha256: digest(descriptor),
    })
}

pub(super) fn compare(
    expected: &GenerationEvidence,
    current: &GenerationEvidence,
    opened: &Stamp,
    descriptor: &[u8],
) -> Result<(), Failure> {
    if !opened.character_device
        || *opened != expected.node
        || current != expected
        || digest(descriptor) != expected.descriptor_sha256
    {
        return Err(Failure::new(
            "stale-generation",
            "verify-opened",
            "Opened fd, device namespace, HID parent or descriptor differs from discovery",
        ));
    }
    Ok(())
}

pub(super) fn verify(
    interface: &Interface,
    file: &File,
    descriptor: &[u8],
) -> Result<GenerationEvidence, Failure> {
    let expected = interface.evidence.as_ref().ok_or_else(|| {
        Failure::new(
            "unavailable",
            "verify-opened",
            "Discovery did not establish a complete descriptor and namespace identity",
        )
    })?;
    verify_access(super::scan::access(&interface.path))?;
    let opened = stamp(
        &file
            .metadata()
            .map_err(|e| Failure::io("opened-fd", e, true))?,
    );
    let parent = hid_parent(&interface.sysfs_path.join("device"), &interface.roots, true)?;
    let fresh = read(
        &parent.join("report_descriptor"),
        MAX_DESCRIPTOR,
        "verify-descriptor",
        true,
    )?;
    let current = current(interface, &fresh, true)?;
    compare(expected, &current, &opened, descriptor)?;
    // Recheck fd/path after sysfs reads, fencing replacement during verification.
    let after = stamp(
        &file
            .metadata()
            .map_err(|e| Failure::io("opened-fd", e, true))?,
    );
    if after != opened || metadata(&interface.path, "device-node", true)? != opened {
        return Err(Failure::new(
            "stale-generation",
            "verify-opened",
            "Device namespace changed while verifying the opened fd",
        ));
    }
    verify_access(super::scan::access(&interface.path))?;
    Ok(current)
}

pub(super) fn verify_access(access: Result<bool, Failure>) -> Result<(), Failure> {
    if !access? {
        return Err(Failure::new("permission-denied","verify-permission","Current effective credentials no longer have read access, even if an existing fd remains readable"));
    }
    Ok(())
}
