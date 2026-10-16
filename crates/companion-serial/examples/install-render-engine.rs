//! One-time bench migration. Read and validate the existing table before writing
//! exactly bootloader/table/application. Never erase NVS or write any eFuse.
use espflash::{
    connection::{Connection, ResetAfterOperation, ResetBeforeOperation},
    flasher::Flasher,
    image_format::Segment,
    target::{Chip, ProgressCallbacks},
};
use md5::Md5;
use osg_companion_serial::{validate_firmware_for, FirmwareSegment, FirmwareTarget};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{borrow::Cow, collections::BTreeSet, fs, path::PathBuf, time::Duration};

struct Progress;
impl ProgressCallbacks for Progress {
    fn init(&mut self, _: u32, _: usize) {}
    fn update(&mut self, _: usize) {}
    fn verifying(&mut self) {}
    fn finish(&mut self, _: bool) {}
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Partition {
    kind: u8,
    subtype: u8,
    offset: u32,
    size: u32,
    name: String,
}
impl Partition {
    fn new(kind: u8, subtype: u8, offset: u32, size: u32, name: &str) -> Self {
        Self {
            kind,
            subtype,
            offset,
            size,
            name: name.into(),
        }
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
struct Mode {
    manual: bool,
    read_only: bool,
}
fn mode(flags: &[String]) -> Result<Mode, String> {
    let mut mode = Mode::default();
    for flag in flags {
        match flag.as_str() {
            "--manual-boot" if !mode.manual => mode.manual = true,
            "--read-only" if !mode.read_only => mode.read_only = true,
            _ => return Err(
                "Unknown or repeated option; no port opened. Use --manual-boot and/or --read-only."
                    .into(),
            ),
        }
    }
    Ok(mode)
}
fn partitions(bytes: &[u8]) -> Result<Vec<Partition>, String> {
    if bytes.len() < 96 || bytes.len() > 4096 || bytes.len() % 32 != 0 {
        return Err("Truncated or oversized partition table; migration stopped.".into());
    }
    let mut entries: Vec<Partition> = Vec::new();
    let mut names = BTreeSet::new();
    let mut md5_marker = [255u8; 16];
    md5_marker[..2].copy_from_slice(&[0xeb, 0xeb]);
    for (index, r) in bytes.chunks_exact(32).enumerate() {
        if r[..16] == md5_marker {
            let after = &bytes[(index + 1) * 32..];
            if entries.is_empty()
                || Md5::digest(&bytes[..index * 32]).as_slice() != &r[16..]
                || after.len() < 32
                || after.iter().any(|b| *b != 255)
            {
                return Err("Invalid partition checksum/terminator; migration stopped.".into());
            }
            return Ok(entries);
        }
        if r[..2] != [0xaa, 0x50] {
            return Err("Unknown partition record or missing MD5; migration stopped.".into());
        }
        let offset = u32::from_le_bytes(r[4..8].try_into().unwrap());
        let size = u32::from_le_bytes(r[8..12].try_into().unwrap());
        let end = r[12..28].iter().position(|b| *b == 0).unwrap_or(16);
        let name = std::str::from_utf8(&r[12..12 + end])
            .map_err(|_| "Invalid partition label")?
            .to_owned();
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || r[12 + end..28].iter().any(|b| *b != 0)
            || !names.insert(name.clone())
        {
            return Err("Invalid, hidden or duplicate partition label; migration stopped.".into());
        }
        if u32::from_le_bytes(r[28..32].try_into().unwrap()) != 0 {
            return Err("Encrypted/flagged partition is ineligible for this migration.".into());
        }
        let limit = offset.checked_add(size).ok_or("Partition range overflow")?;
        if offset < 0x9000
            || offset % 4096 != 0
            || size == 0
            || size % 4096 != 0
            || (r[2] == 0 && offset % 0x10000 != 0)
            || entries
                .iter()
                .any(|p| offset < p.offset + p.size && p.offset < limit)
        {
            return Err("Invalid or overlapping partition range; migration stopped.".into());
        }
        entries.push(Partition::new(r[2], r[3], offset, size, &name));
    }
    Err("Missing partition checksum/terminator; migration stopped.".into())
}
fn required(store_size: u32) -> Vec<Partition> {
    vec![
        Partition::new(1, 2, 0x9000, 0x6000, "nvs"),
        Partition::new(1, 1, 0xf000, 0x1000, "phy_init"),
        Partition::new(0, 0, 0x10000, 0x180000, "factory"),
        Partition::new(1, 0x40, 0x190000, store_size, "ui_store"),
    ]
}
fn migration(actual: &[Partition], candidate: &[Partition], store_size: u32) -> Result<(), String> {
    let approved = required(store_size);
    let flash_end = if store_size == 0x200000 {
        0x400000
    } else {
        0x800000
    };
    if candidate.len() != approved.len() || !approved.iter().all(|p| candidate.contains(p)) {
        return Err("Candidate partition table differs from the approved migration.".into());
    }
    if actual.iter().any(|p| {
        p.offset
            .checked_add(p.size)
            .is_none_or(|end| end > flash_end)
    }) {
        return Err("Existing partition exceeds physical flash; no writes performed.".into());
    }
    if !approved[..2].iter().all(|p| actual.contains(p))
        || actual
            .iter()
            .filter(|p| p.kind == 0 && p.subtype == 0 && p.offset == 0x10000 && p.name == "factory")
            .count()
            != 1
    {
        return Err("Existing NVS/PHY/factory layout is incompatible; no writes performed.".into());
    }
    if actual.iter().any(|p| {
        p != &approved[0]
            && p != &approved[1]
            && p != &approved[3]
            && !(p.kind == 0 && p.subtype == 0 && p.offset == 0x10000 && p.name == "factory")
    }) {
        return Err("Existing additional or relabeled data requires a separate migration; no writes performed.".into());
    }
    Ok(())
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn verify_readback(expected: &[u8], actual: &[u8]) -> Result<String, String> {
    let actual_sha = sha256(actual);
    if expected.len() != actual.len() || sha256(expected) != actual_sha {
        return Err(
            "Firmware segment readback verification failed; board left in flasher stub.".into(),
        );
    }
    Ok(actual_sha)
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 5 || args.len() > 7 {
        return Err("Expected PORT BOARD_PROFILE BUILD_DIRECTORY EVIDENCE_DIRECTORY [--manual-boot] [--read-only]".into());
    }
    // A typo must never convert a requested read-only run into flash writes.
    let mode = mode(&args[5..])?;
    let (target, chip, boot, store_size) = match args[2].as_str() {
        "sunton-3248s035r" => (FirmwareTarget::Esp32, Chip::Esp32, 0x1000, 0x200000),
        "waveshare-lcd7-s3" => (FirmwareTarget::Esp32s3, Chip::Esp32s3, 0, 0x600000),
        _ => return Err("Unknown board profile".into()),
    };
    let build = PathBuf::from(&args[3]);
    let evidence = PathBuf::from(&args[4]);
    let segments = [
        (boot, "bootloader/bootloader.bin"),
        (0x8000, "partition_table/partition-table.bin"),
        (0x10000, "opensimgear_companion_esp32.bin"),
    ]
    .into_iter()
    .map(|(offset, name)| {
        Ok(FirmwareSegment {
            offset,
            bytes: Box::leak(
                fs::read(build.join(name))
                    .map_err(error)?
                    .into_boxed_slice(),
            ),
        })
    })
    .collect::<Result<Vec<_>, String>>()?;
    validate_firmware_for(target, &segments)?;
    if segments[1].bytes.len() > 4096 || segments[2].bytes.len() > 0x180000 {
        return Err("Candidate engine image exceeds the approved partition.".into());
    }
    let candidate = partitions(segments[1].bytes)?;
    // Candidate validation is also completed before opening a device.
    migration(&required(store_size), &candidate, store_size)?;
    fs::create_dir_all(&evidence).map_err(error)?;
    let port_info = serialport::available_ports()
        .map_err(error)?
        .into_iter()
        .find(|p| p.port_name == args[1])
        .ok_or("USB board is not connected")?;
    let serialport::SerialPortType::UsbPort(info) = port_info.port_type else {
        return Err("Expected a USB bridge".into());
    };
    let port = serialport::new(&args[1], 115200)
        .timeout(Duration::from_secs(3))
        .open_native()
        .map_err(error)?;
    let connection = Connection::new(
        port,
        info,
        ResetAfterOperation::NoResetNoStub,
        if mode.manual {
            ResetBeforeOperation::NoReset
        } else {
            ResetBeforeOperation::DefaultReset
        },
        115200,
    );
    let mut flasher = Flasher::connect(connection, true, true, false, Some(chip), Some(460800))
        .map_err(|e| format!("bootloader-required:{e}"))?;
    // These read-only security checks are equivalent to production flash.rs.
    let secured = match target {
        FirmwareTarget::Esp32 => {
            let crypt = flasher.connection().read_reg(0x3ff5a000).map_err(error)?;
            let boot = flasher.connection().read_reg(0x3ff5a018).map_err(error)?;
            crypt & (0x7f << 20) != 0 || boot & (3 << 4) != 0
        }
        FirmwareTarget::Esp32s3 => {
            let s = flasher.security_info().map_err(error)?;
            s.flags & 5 != 0 || s.flash_crypt_cnt != 0
        }
    };
    if secured {
        return Err("This migration is unavailable on a secured board.".into());
    }
    let table_path = evidence.join(format!("{}-partition-before.bin", args[2]));
    flasher
        .read_flash(0x8000, 4096, 1024, 1, table_path.clone())
        .map_err(error)?;
    let actual = partitions(&fs::read(table_path).map_err(error)?)?;
    migration(&actual, &candidate, store_size)?;
    // Hash NVS/PHY in the flasher; never write their plaintext bytes to a file.
    let nvs_before = flasher.checksum_md5(0x9000, 0x6000).map_err(error)?;
    let phy_before = flasher.checksum_md5(0xf000, 0x1000).map_err(error)?;
    if !mode.read_only {
        let writes = segments
            .iter()
            .map(|s| Segment {
                addr: s.offset,
                data: Cow::Borrowed(s.bytes),
            })
            .collect::<Vec<_>>();
        flasher
            .write_bins_to_flash(&writes, &mut Progress)
            .map_err(error)?;
    }
    // NoResetNoStub intentionally retains the same live stub after write_bins.
    // Verify every written range before any reset; failure leaves the stub alive.
    let mut verified = Vec::new();
    for (segment, name) in segments
        .iter()
        .zip(["bootloader", "partition", "application"])
    {
        let path = evidence.join(format!("{}-{name}-readback.bin", args[2]));
        flasher
            .read_flash(
                segment.offset,
                segment.bytes.len() as u32,
                1024,
                1,
                path.clone(),
            )
            .map_err(error)?;
        let actual = fs::read(path).map_err(error)?;
        let hash = if mode.read_only {
            sha256(&actual)
        } else {
            verify_readback(segment.bytes, &actual)?
        };
        verified.push(json!({"name":name,"offset":segment.offset,"bytes":actual.len(),"sha256":hash,"expectedSha256":sha256(segment.bytes)}));
    }
    let nvs_after = flasher.checksum_md5(0x9000, 0x6000).map_err(error)?;
    let phy_after = flasher.checksum_md5(0xf000, 0x1000).map_err(error)?;
    if nvs_before != nvs_after || phy_before != phy_after {
        return Err("NVS/PHY integrity changed; board left in flasher stub.".into());
    }
    flasher.connection().reset().map_err(error)?;
    println!(
        "{}",
        json!({"port":args[1],"profile":args[2],"readOnly":mode.read_only,"segments":verified,"applicationSha256":verified[2]["sha256"],"expectedApplicationSha256":verified[2]["expectedSha256"],"applicationBytes":verified[2]["bytes"],"nvsAndPhyPreserved":true,"nvsMd5":format!("{nvs_after:032x}"),"phyMd5":format!("{phy_after:032x}"),"writtenOffsets":if mode.read_only {Vec::<u32>::new()}else{segments.iter().map(|s|s.offset).collect()}})
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(parts: &[Partition]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for p in parts {
            let mut record = [0u8; 32];
            record[..2].copy_from_slice(&[0xaa, 0x50]);
            record[2] = p.kind;
            record[3] = p.subtype;
            record[4..8].copy_from_slice(&p.offset.to_le_bytes());
            record[8..12].copy_from_slice(&p.size.to_le_bytes());
            record[12..12 + p.name.len()].copy_from_slice(p.name.as_bytes());
            bytes.extend(record);
        }
        let digest = Md5::digest(&bytes);
        bytes.extend([0xeb, 0xeb]);
        bytes.extend([255; 14]);
        bytes.extend(digest);
        bytes.resize(4096, 255);
        bytes
    }
    #[test]
    fn flags_fail_closed_before_device_access() {
        let f = |flags: &[&str]| flags.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(mode(&[]).unwrap(), Mode::default());
        assert!(mode(&f(&["--read-only"])).unwrap().read_only);
        assert_eq!(
            mode(&f(&["--manual-boot", "--read-only"])).unwrap(),
            Mode {
                manual: true,
                read_only: true
            }
        );
        for flags in [
            vec!["--read-onyl"],
            vec!["--manual-boot", "--manual-boot"],
            vec!["--read-only", "--read-only"],
            vec!["--help"],
        ] {
            assert!(mode(&f(&flags)).is_err());
        }
    }
    #[test]
    fn accepts_only_exact_new_or_known_single_factory_layouts() {
        for (size, end) in [(0x200000, 0x400000), (0x600000, 0x800000)] {
            let candidate = required(size);
            let parsed = partitions(&table(&candidate)).unwrap();
            assert_eq!(parsed, candidate);
            assert!(migration(&parsed, &candidate, size).is_ok());
            let mut old = candidate[..3].to_vec();
            old[2].size = end - 0x10000;
            let old = partitions(&table(&old)).unwrap();
            assert!(migration(&old, &candidate, size).is_ok());
            let mut wrong = candidate.clone();
            wrong[3].size -= 4096;
            assert!(migration(&wrong, &candidate, size).is_err());
            assert!(migration(&candidate, &wrong, size).is_err());
            let mut outside = old.clone();
            outside[2].size += 4096;
            assert!(migration(&outside, &candidate, size).is_err());
        }
    }
    #[test]
    fn rejects_hidden_or_relabeled_existing_data_and_credential_changes() {
        let candidate = required(0x200000);
        let mut old = candidate[..3].to_vec();
        old.push(Partition::new(1, 0x81, 0x390000, 4096, "factory-data"));
        assert!(migration(&partitions(&table(&old)).unwrap(), &candidate, 0x200000).is_err());
        old.pop();
        old[2] = Partition::new(1, 0x81, 0x10000, 0x180000, "factory");
        assert!(migration(&partitions(&table(&old)).unwrap(), &candidate, 0x200000).is_err());
        for i in 0..2 {
            let mut old = candidate.clone();
            old[i].subtype ^= 1;
            assert!(migration(&old, &candidate, 0x200000).is_err());
        }
        let mut old = candidate.clone();
        old[3].subtype = 0x81;
        assert!(migration(&old, &candidate, 0x200000).is_err());
    }
    #[test]
    fn rejects_corrupt_incomplete_or_ambiguous_partition_bytes() {
        let valid = table(&required(0x200000));
        for n in [0, 32, 64, 127, 128, 160, 4095] {
            assert!(partitions(&valid[..n]).is_err());
        }
        let mut corrupt = valid.clone();
        corrupt[4] ^= 1;
        assert!(partitions(&corrupt).is_err());
        let mut corrupt = valid.clone();
        corrupt[128 + 2] = 0;
        assert!(partitions(&corrupt).is_err());
        let mut corrupt = valid.clone();
        corrupt[200] = 0;
        assert!(partitions(&corrupt).is_err());
        let mut corrupt = valid.clone();
        corrupt[128..160].fill(255);
        assert!(partitions(&corrupt).is_err());
        let mut duplicate = required(0x200000);
        duplicate[3].name = "nvs".into();
        assert!(partitions(&table(&duplicate)).is_err());
        let mut overlap = required(0x200000);
        overlap[3].offset = 0x180000;
        assert!(partitions(&table(&overlap)).is_err());
        let mut overflow = required(0x200000);
        overflow[3].offset = 0xfffff000;
        assert!(partitions(&table(&overflow)).is_err());
        let mut flags = valid.clone();
        flags[28] = 1;
        assert!(partitions(&flags).is_err());
        let mut hidden = valid;
        hidden[27] = 1;
        assert!(partitions(&hidden).is_err());
    }
    #[test]
    fn readback_requires_exact_length_and_digest() {
        assert_eq!(
            verify_readback(b"firmware", b"firmware").unwrap(),
            sha256(b"firmware")
        );
        assert!(verify_readback(b"firmware", b"firmwarE").is_err());
        assert!(verify_readback(b"firmware", b"firm").is_err());
    }
}
