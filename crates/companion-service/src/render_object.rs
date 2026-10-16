//! Structural reader for render ABI 1 objects. This is deliberately separate
//! from publication admission: successful inspection does not validate record
//! contents, authorize a package, or create an input session.
use std::ops::Range;

const HEADER_BYTES: usize = 32;
const DIRECTORY_BYTES: usize = 16;
pub const MAX_RENDER_OBJECT_BYTES: usize = 2560 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderObjectKind {
    Semantics,
    Scene,
    Images,
    Fonts,
}

impl RenderObjectKind {
    fn magic(self) -> &'static [u8; 4] {
        match self {
            Self::Semantics => b"OSGC",
            Self::Scene => b"OSGS",
            Self::Images => b"OSGI",
            Self::Fonts => b"OSGF",
        }
    }

    fn records(self) -> &'static [u16] {
        match self {
            Self::Semantics => &[32, 32, 24, 1],
            Self::Scene => &[16, 64, 24, 16, 16, 24, 8, 24, 4, 1, 12, 2, 8],
            Self::Images => &[24, 1],
            Self::Fonts => &[16, 24, 1],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderSection {
    pub kind: u16,
    pub record_bytes: u16,
    pub count: u32,
    pub bytes: Range<usize>,
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Inspect a bounded, canonical ABI container. At most thirteen directory
/// records are allocated, independent of declared record counts. Callers must
/// subsequently check object digests, record contents and cross-object/source
/// references before any package can be used by a renderer or input session.
pub fn inspect_render_object(
    kind: RenderObjectKind,
    bytes: &[u8],
    max_bytes: usize,
) -> Result<Vec<RenderSection>, String> {
    let limit = max_bytes.min(MAX_RENDER_OBJECT_BYTES);
    let ensure = |condition: bool, message: &str| {
        if condition {
            Ok(())
        } else {
            Err(message.to_owned())
        }
    };
    ensure(
        bytes.len() >= HEADER_BYTES && bytes.len() <= limit,
        "Render object exceeds byte budget.",
    )?;
    ensure(&bytes[..4] == kind.magic(), "Render object magic mismatch.")?;
    ensure(
        u16_at(bytes, 4) == 1 && usize::from(u16_at(bytes, 6)) == HEADER_BYTES,
        "Unsupported render object version/header.",
    )?;
    ensure(
        u32_at(bytes, 8) as usize == bytes.len() && u32_at(bytes, 12) as usize == HEADER_BYTES,
        "Render object length/directory mismatch.",
    )?;
    let entries = usize::from(u16_at(bytes, 16));
    ensure(
        entries > 0 && entries <= kind.records().len(),
        "Invalid render directory count.",
    )?;
    ensure(
        bytes[18..HEADER_BYTES].iter().all(|byte| *byte == 0),
        "Unknown render flags/reserved bytes.",
    )?;
    let mut cursor = HEADER_BYTES + entries * DIRECTORY_BYTES;
    ensure(cursor <= bytes.len(), "Truncated render directory.")?;
    let mut sections = Vec::with_capacity(entries);
    let mut previous_kind = 0;
    for index in 0..entries {
        let base = HEADER_BYTES + index * DIRECTORY_BYTES;
        let section_kind = u16_at(bytes, base);
        let record_bytes = u16_at(bytes, base + 2);
        let offset = u32_at(bytes, base + 4) as usize;
        let count = u32_at(bytes, base + 8);
        let length = u32_at(bytes, base + 12) as usize;
        let layout = section_kind
            .checked_sub(1)
            .and_then(|id| kind.records().get(usize::from(id)));
        ensure(
            section_kind > previous_kind && layout == Some(&record_bytes),
            "Unknown, unsorted or incompatible render section.",
        )?;
        ensure(
            count > 0 && u64::from(count) * u64::from(record_bytes) == length as u64,
            "Invalid render section count/length.",
        )?;
        // cursor is bounded by the small global object limit before each step.
        let aligned = (cursor + 3) & !3;
        ensure(
            offset == aligned,
            "Overlapping or noncanonical render section.",
        )?;
        ensure(
            offset <= bytes.len() && length <= bytes.len() - offset,
            "Render section is out of bounds.",
        )?;
        ensure(
            bytes[cursor..offset].iter().all(|byte| *byte == 0),
            "Nonzero render alignment padding.",
        )?;
        cursor = offset + length;
        previous_kind = section_kind;
        sections.push(RenderSection {
            kind: section_kind,
            record_bytes,
            count,
            bytes: offset..cursor,
        });
    }
    ensure(cursor == bytes.len(), "Unexpected trailing render bytes.")?;
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture(kind: RenderObjectKind, ids: &[u16]) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_BYTES + ids.len() * DIRECTORY_BYTES];
        bytes[..4].copy_from_slice(kind.magic());
        put16(&mut bytes, 4, 1);
        put16(&mut bytes, 6, HEADER_BYTES as u16);
        put32(&mut bytes, 12, HEADER_BYTES as u32);
        put16(&mut bytes, 16, ids.len() as u16);
        for (i, id) in ids.iter().copied().enumerate() {
            let record_bytes = kind.records()[usize::from(id - 1)];
            let offset = (bytes.len() + 3) & !3;
            let base = HEADER_BYTES + i * DIRECTORY_BYTES;
            bytes.resize(offset + usize::from(record_bytes), 0);
            put16(&mut bytes, base, id);
            put16(&mut bytes, base + 2, record_bytes);
            put32(&mut bytes, base + 4, offset as u32);
            put32(&mut bytes, base + 8, 1);
            put32(&mut bytes, base + 12, u32::from(record_bytes));
        }
        let length = bytes.len() as u32;
        put32(&mut bytes, 8, length);
        bytes
    }

    #[test]
    fn reads_all_abi_layouts_and_exact_ranges() {
        for kind in [
            RenderObjectKind::Semantics,
            RenderObjectKind::Scene,
            RenderObjectKind::Images,
            RenderObjectKind::Fonts,
        ] {
            let ids: Vec<_> = (1..=kind.records().len() as u16).collect();
            let bytes = fixture(kind, &ids);
            let sections = inspect_render_object(kind, &bytes, bytes.len()).unwrap();
            assert_eq!(sections.len(), ids.len());
            for (section, stride) in sections.iter().zip(kind.records()) {
                assert_eq!(section.count, 1);
                assert_eq!(section.bytes.len(), usize::from(*stride));
                assert_eq!(section.record_bytes, *stride);
            }
            assert!(inspect_render_object(kind, &bytes, bytes.len() - 1).is_err());
        }
    }

    #[test]
    fn rejects_truncation_and_header_mutations_without_panics() {
        let good = fixture(RenderObjectKind::Scene, &[1, 10]);
        for length in 0..good.len() {
            assert!(inspect_render_object(RenderObjectKind::Scene, &good[..length], 4096).is_err());
        }
        for offset in 0..HEADER_BYTES {
            let mut bad = good.clone();
            bad[offset] ^= 0x80;
            assert!(
                inspect_render_object(RenderObjectKind::Scene, &bad, 4096).is_err(),
                "header {offset}"
            );
        }
        let mut excessive = good.clone();
        put16(&mut excessive, 16, 14);
        assert!(inspect_render_object(RenderObjectKind::Scene, &excessive, 4096).is_err());
        put16(&mut excessive, 16, 0);
        assert!(inspect_render_object(RenderObjectKind::Scene, &excessive, 4096).is_err());
        assert!(inspect_render_object(RenderObjectKind::Images, &good, 4096).is_err());
        assert!(inspect_render_object(
            RenderObjectKind::Scene,
            &vec![0; MAX_RENDER_OBJECT_BYTES + 1],
            usize::MAX
        )
        .is_err());
    }

    #[test]
    fn rejects_section_corruption_overlap_overflow_and_trailing_bytes() {
        let kind = RenderObjectKind::Scene;
        let good = fixture(kind, &[1, 10]);
        for (offset, value) in [(32, 0), (32, 14), (34, 2), (48, 1)] {
            let mut bad = good.clone();
            put16(&mut bad, offset, value);
            assert!(inspect_render_object(kind, &bad, 4096).is_err());
        }
        for (offset, value) in [
            (36, 0),
            (36, 65),
            (36, u32::MAX),
            (40, 0),
            (40, u32::MAX),
            (44, 0),
            (44, u32::MAX),
            (52, 64),
            (52, u32::MAX),
            (60, u32::MAX),
        ] {
            let mut bad = good.clone();
            put32(&mut bad, offset, value);
            assert!(
                inspect_render_object(kind, &bad, 4096).is_err(),
                "section {offset}={value}"
            );
        }
        let mut bad = good;
        bad.push(0);
        let length = bad.len() as u32;
        put32(&mut bad, 8, length);
        assert!(inspect_render_object(kind, &bad, 4096).is_err());
    }

    #[test]
    fn checks_padding_and_does_not_admit_record_semantics() {
        let kind = RenderObjectKind::Scene;
        let mut bytes = fixture(kind, &[10, 11]);
        let sections = inspect_render_object(kind, &bytes, 4096).unwrap();
        let padding = sections[0].bytes.end;
        bytes[padding] = 1;
        assert!(inspect_render_object(kind, &bytes, 4096).is_err());
        bytes[padding] = 0;
        // Contents are intentionally invalid as a scene. This lower-level
        // inspection must not be confused with semantic/authority admission.
        bytes[sections[0].bytes.start] = 0xff;
        assert!(inspect_render_object(kind, &bytes, 4096).is_ok());
    }
}
