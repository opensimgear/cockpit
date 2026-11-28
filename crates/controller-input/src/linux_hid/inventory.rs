//! Metadata-only hidraw discovery. Input/event capture remains a separate policy.
use std::fs::File;
mod types;
pub use types::*;
mod evidence;
use crate::hid::restrictions;
mod scan;

pub fn enumerate() -> Result<Inventory, Failure> {
    enumerate_at(&Roots::default())
}
pub fn enumerate_at(roots: &Roots) -> Result<Inventory, Failure> {
    scan::enumerate(roots)
}

/// The service supplies descriptor bytes acquired from this same fd via the
/// descriptor ioctl. This method never reads an input, feature or output report.
/// It fences discovery->open, pre-GET, and periodic checks against both the live
/// fd and current namespace/sysfs evidence; inability to prove identity is fatal.
pub fn verify_opened(
    interface: &Interface,
    file: &File,
    descriptor: &[u8],
) -> Result<GenerationEvidence, Failure> {
    evidence::verify(interface, file, descriptor)
}

#[cfg(test)]
pub(super) mod tests;
