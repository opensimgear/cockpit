use super::GetKind;

/// Linux UAPI asm-generic/ioctl.h plus architecture overrides. In particular,
/// PowerPC/MIPS use a different READ bit/size limit; SPARC overlaps its NONE bit
/// with an extra size bit. Unknown ABIs are never treated as x86 implicitly.
#[derive(Clone, Copy, Debug)]
pub(super) struct Abi {
    size_bits: u32,
    direction_shift: u32,
    read: u32,
    write: u32,
}
impl Abi {
    const GENERIC: Self = Self {
        size_bits: 14,
        direction_shift: 30,
        read: 2,
        write: 1,
    };
    const ALTERNATE: Self = Self {
        size_bits: 13,
        direction_shift: 29,
        read: 2,
        write: 4,
    };
    const SPARC: Self = Self {
        size_bits: 14,
        direction_shift: 29,
        read: 2,
        write: 4,
    };
    fn request(self, number: u8, size: usize, write: bool) -> Option<u32> {
        if size >= (1usize << self.size_bits) {
            return None;
        }
        let direction = self.read | if write { self.write } else { 0 };
        Some(
            (direction << self.direction_shift)
                | ((size as u32) << 16)
                | (u32::from(b'H') << 8)
                | u32::from(number),
        )
    }
    pub(super) fn read(self, number: u8, size: usize) -> Option<u32> {
        self.request(number, size, false)
    }
    pub(super) fn get(self, kind: GetKind, size: usize) -> Option<u32> {
        self.request(
            match kind {
                GetKind::Feature => 7,
                GetKind::Input => 10,
            },
            size,
            true,
        )
    }
}
pub(super) fn native() -> Option<Abi> {
    for_arch(std::env::consts::ARCH)
}
fn for_arch(arch: &str) -> Option<Abi> {
    match arch {
        "x86" | "x86_64" | "arm" | "aarch64" | "riscv32" | "riscv64" | "s390x" | "loongarch64"
        | "m68k" | "hexagon" => Some(Abi::GENERIC),
        "powerpc" | "powerpc64" | "mips" | "mips64" | "mips32r6" | "mips64r6" => {
            Some(Abi::ALTERNATE)
        }
        "sparc" | "sparc64" => Some(Abi::SPARC),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_uapi_layouts_differ_for_read_only_metadata_and_enforce_size_bits() {
        let x86 = for_arch("x86_64").unwrap();
        let ppc = for_arch("powerpc64").unwrap();
        let sparc = for_arch("sparc64").unwrap();
        assert_eq!(x86.read(1, 4), Some(0x80044801));
        assert_eq!(ppc.read(1, 4), Some(0x40044801));
        assert_eq!(sparc.read(1, 4), Some(0x40044801));
        assert_eq!(x86.get(GetKind::Feature, 64), Some(0xc0404807));
        assert_eq!(ppc.get(GetKind::Input, 64), Some(0xc040480a));
        assert!(ppc.get(GetKind::Feature, 8192).is_none());
        assert!(sparc.get(GetKind::Feature, 8192).is_some());
        assert!(x86.get(GetKind::Feature, 16384).is_none());
        assert!(for_arch("unverified-abi").is_none());
    }
}
