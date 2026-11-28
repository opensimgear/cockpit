use super::*;
const DESCRIPTOR_BYTES: usize = 4096;

// ioctl structs are kernel-native-endian. Aligned storage also satisfies int
// pointer alignment on Linux architectures that reject unaligned user access.
struct Aligned {
    words: Vec<u64>,
    length: usize,
}
impl Aligned {
    fn new(length: usize) -> Self {
        Self {
            words: vec![0; length.div_ceil(8)],
            length,
        }
    }
    fn bytes(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.words.as_mut_ptr().cast(), self.length) }
    }
}
fn query(calls: &dyn Calls, fd: i32, number: u8, length: usize) -> Result<Aligned, IoError> {
    let request = abi::native()
        .and_then(|abi| abi.read(number, length))
        .ok_or_else(|| {
            error(
                ErrorKind::Unsupported,
                "Linux ioctl ABI or metadata size unsupported",
            )
        })?;
    let mut buffer = Aligned::new(length);
    calls
        .ioctl(fd, request, buffer.bytes())
        .map_err(|e| os("Read hidraw metadata", e))?;
    Ok(buffer)
}
fn text(calls: &dyn Calls, fd: i32, number: u8) -> Option<String> {
    let mut buffer = query(calls, fd, number, 512).ok()?;
    let bytes = buffer.bytes();
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end])
        .ok()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
pub(super) fn read(fd: i32, calls: &dyn Calls) -> Result<Metadata, IoError> {
    let mut size = query(calls, fd, 1, 4)?;
    let size = i32::from_ne_bytes(size.bytes().try_into().unwrap());
    if size <= 0 || size as usize > DESCRIPTOR_BYTES {
        return Err(error(
            ErrorKind::Capacity,
            "Invalid hidraw descriptor byte count",
        ));
    }
    let request = abi::native()
        .and_then(|abi| abi.read(2, DESCRIPTOR_BYTES + 4))
        .ok_or_else(|| {
            error(
                ErrorKind::Unsupported,
                "Linux descriptor ioctl ABI unsupported",
            )
        })?;
    let mut descriptor = Aligned::new(DESCRIPTOR_BYTES + 4);
    descriptor.bytes()[..4].copy_from_slice(&size.to_ne_bytes());
    calls
        .ioctl(fd, request, descriptor.bytes())
        .map_err(|e| os("Read opened hidraw descriptor", e))?;
    let returned = u32::from_ne_bytes(descriptor.bytes()[..4].try_into().unwrap()) as usize;
    if returned != size as usize {
        return Err(error(
            ErrorKind::Removed,
            "hidraw descriptor length changed while inspecting the opened fd",
        ));
    }
    // HIDIOCGRDESC does not return a copied-byte count or update its input size.
    // Recheck the actual descriptor length so a shorter replacement cannot be
    // mistaken for zero-filled descriptor bytes from the allocation.
    let mut after = query(calls, fd, 1, 4)?;
    if i32::from_ne_bytes(after.bytes().try_into().unwrap()) != size {
        return Err(error(
            ErrorKind::Removed,
            "hidraw descriptor size changed during the metadata transaction",
        ));
    }
    let descriptor = descriptor.bytes()[4..4 + returned].to_vec();
    let mut info = query(calls, fd, 3, 8)?;
    let info = info.bytes();
    Ok(Metadata {
        descriptor,
        bus: u32::from_ne_bytes(info[..4].try_into().unwrap()),
        vendor_id: u16::from_ne_bytes(info[4..6].try_into().unwrap()),
        product_id: u16::from_ne_bytes(info[6..8].try_into().unwrap()),
        name: text(calls, fd, 4),
        physical: text(calls, fd, 5),
        unique: text(calls, fd, 8),
    })
}
