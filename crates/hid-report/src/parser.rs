use crate::model::*;
use std::collections::BTreeMap;
mod locals;
use locals::LocalState;

fn error(offset: usize, reason: &str) -> Diagnostic {
    Diagnostic {
        offset,
        reason: reason.into(),
    }
}
fn signed(value: u32, width: usize) -> i64 {
    let shift = 64 - width * 8;
    ((value as i64) << shift) >> shift
}
struct Parser {
    data: DescriptorData,
    globals: Globals,
    stack: Vec<Globals>,
    local: LocalState,
    collections: Vec<usize>,
    layouts: BTreeMap<(ReportKind, Option<u8>), usize>,
    elements: usize,
}
impl Parser {
    fn diagnose(&mut self, offset: usize, reason: &str) {
        if self.data.diagnostics.len() < 64 {
            self.data.diagnostics.push(error(offset, reason));
        }
    }
    fn malformed(&mut self, index: usize, reason: &str) {
        self.data.items[index].disposition = Disposition::Malformed;
        self.diagnose(self.data.items[index].offset, reason);
    }
    fn global(
        &mut self,
        index: usize,
        tag: u8,
        value: u32,
        width: usize,
    ) -> Result<(), Diagnostic> {
        let offset = self.data.items[index].offset;
        if tag > 11 {
            self.data.items[index].disposition = Disposition::Preserved;
            return Ok(());
        }
        if matches!(tag, 10 | 11) {
            if width != 0 {
                self.malformed(index, "Push and Pop require zero item data bytes");
                return Ok(());
            }
            if tag == 10 {
                if self.stack.len() == MAX_DEPTH {
                    return Err(error(offset, "Global stack depth budget exceeded"));
                }
                self.stack.push(self.globals.clone());
            } else if let Some(state) = self.stack.pop() {
                self.globals = state;
            } else {
                self.malformed(index, "Global stack Pop underflow");
            }
            return Ok(());
        }
        if width == 0 {
            self.malformed(index, "Global value item has no data");
            return Ok(());
        }
        match tag {
            0 if value <= u16::MAX as u32 => self.globals.usage_page = value as u16,
            0 => self.malformed(index, "Usage Page exceeds sixteen bits"),
            1 => self.globals.logical_min = Some(signed(value, width)),
            2 => {
                self.globals.logical_max =
                    Some(if self.globals.logical_min.is_some_and(|v| v < 0) {
                        signed(value, width)
                    } else {
                        value as i64
                    })
            }
            3 => self.globals.physical_min = Some(signed(value, width)),
            4 => {
                self.globals.physical_max =
                    Some(if self.globals.physical_min.is_some_and(|v| v < 0) {
                        signed(value, width)
                    } else {
                        value as i64
                    })
            }
            5 => {
                let exponent = ((value & 15) as i8) << 4 >> 4;
                if value <= 15 || signed(value, width) == exponent as i64 {
                    self.globals.exponent = Some(exponent);
                } else {
                    self.malformed(index, "Unit exponent is not a signed four-bit value");
                }
            }
            6 => self.globals.unit = Some(value),
            7 => self.globals.size = value as usize,
            8 if width == 1 && (1..=255).contains(&value) => {
                self.globals.report_id = Some(value as u8)
            }
            8 => self.malformed(index, "Report ID requires one nonzero byte"),
            9 => self.globals.count = value as usize,
            _ => unreachable!(),
        }
        Ok(())
    }
    fn main_item(
        &mut self,
        index: usize,
        tag: u8,
        value: u32,
        width: usize,
    ) -> Result<(), Diagnostic> {
        let offset = self.data.items[index].offset;
        let delimited = self.local.delimited();
        let (primary, alternatives, errors) = std::mem::take(&mut self.local).finish(
            self.globals.usage_page,
            matches!(tag, 8 | 9 | 11).then_some(self.globals.count),
        );
        for reason in errors {
            self.malformed(index, reason);
        }
        match tag {
            8 | 9 | 11 => {
                if width == 0 {
                    self.malformed(index, "Report Main item has no flags");
                }
                if value & !0x1ff != 0 || (tag == 8 && value & 0x80 != 0) {
                    self.malformed(index, "Reserved Main flag bits are nonzero");
                }
                if delimited && value & 2 == 0 {
                    self.malformed(index, "Array items cannot use delimiter usages");
                }
                if self.globals.size == 0 || self.globals.count == 0 {
                    self.malformed(index, "Report Size and Count must be nonzero");
                }
                if value & 0x101 == 0 && self.globals.size > MAX_SCALAR_BITS {
                    return Err(error(offset, "Scalar width budget exceeded"));
                }
                let elements = if value & 0x100 != 0 {
                    1
                } else {
                    self.globals.count
                };
                self.elements = self
                    .elements
                    .checked_add(elements)
                    .filter(|n| *n <= MAX_FIELDS)
                    .ok_or_else(|| error(offset, "Total scalar element budget exceeded"))?;
                if self.data.fields.len() == MAX_FIELDS {
                    return Err(error(offset, "Main field declaration budget exceeded"));
                }
                for (minimum, maximum) in [
                    (self.globals.logical_min, self.globals.logical_max),
                    (self.globals.physical_min, self.globals.physical_max),
                ] {
                    if minimum.zip(maximum).is_some_and(|(lo, hi)| lo > hi) {
                        self.malformed(index, "Global range minimum exceeds maximum");
                    }
                }
                let bits = self
                    .globals
                    .size
                    .checked_mul(self.globals.count)
                    .ok_or_else(|| error(offset, "Report bit size overflow"))?;
                let kind = match tag {
                    8 => ReportKind::Input,
                    9 => ReportKind::Output,
                    _ => ReportKind::Feature,
                };
                let key = (kind, self.globals.report_id);
                let layout = if let Some(&layout) = self.layouts.get(&key) {
                    layout
                } else {
                    let layout = self.data.layouts.len();
                    self.layouts.insert(key, layout);
                    self.data.layouts.push(Layout {
                        kind,
                        report_id: key.1,
                        payload_bits: 0,
                        fields: Vec::new(),
                    });
                    layout
                };
                let current = self.data.layouts[layout].payload_bits;
                let maximum = (MAX_REPORT_BYTES - usize::from(key.1.is_some())) * 8;
                let end = current
                    .checked_add(bits)
                    .filter(|n| *n <= maximum)
                    .ok_or_else(|| error(offset, "Report payload byte budget exceeded"))?;
                self.data.layouts[layout].payload_bits = end;
                self.data.layouts[layout]
                    .fields
                    .push(self.data.fields.len());
                self.data.fields.push(Field {
                    item_index: index,
                    collection: self.collections.last().copied(),
                    kind,
                    offset_bits: current,
                    flags: value,
                    globals: self.globals.clone(),
                    primary,
                    alternatives,
                });
            }
            10 => {
                if width != 1 {
                    self.malformed(index, "Collection requires one type byte");
                }
                if delimited && value == 1 {
                    self.malformed(index, "Application Collection cannot use delimiter usages");
                }
                if self.collections.len() == MAX_DEPTH {
                    return Err(error(offset, "Collection depth budget exceeded"));
                }
                if self.data.collections.len() == MAX_FIELDS {
                    return Err(error(offset, "Collection count budget exceeded"));
                }
                let collection = self.data.collections.len();
                self.data.collections.push(Collection {
                    parent: self.collections.last().copied(),
                    item_index: index,
                    type_code: value as u8,
                    primary,
                    alternatives,
                });
                self.collections.push(collection);
            }
            12 => {
                if width != 0 {
                    self.malformed(index, "End Collection requires zero data bytes");
                }
                if self.collections.pop().is_none() {
                    self.malformed(index, "End Collection underflow");
                }
            }
            _ => self.data.items[index].disposition = Disposition::Preserved,
        }
        Ok(())
    }
}

/// Parse HID 1.11 items without device I/O. Every in-budget byte remains in raw,
/// including unsupported and malformed items. Nonempty diagnostics forbid decode.
pub fn parse_descriptor(raw: &[u8]) -> Result<Descriptor, Diagnostic> {
    if raw.len() > MAX_DESCRIPTOR_BYTES {
        return Err(error(0, "Descriptor byte budget exceeded"));
    }
    let mut p = Parser {
        data: DescriptorData {
            raw: raw.to_vec(),
            ..Default::default()
        },
        globals: Globals::default(),
        stack: Vec::new(),
        local: LocalState::default(),
        collections: Vec::new(),
        layouts: BTreeMap::new(),
        elements: 0,
    };
    let mut offset = 0;
    while offset < raw.len() {
        let start = offset;
        let prefix = raw[offset];
        offset += 1;
        let long = prefix == 0xfe;
        let (type_code, tag, width, header) = if long {
            if raw.len() - offset < 2 {
                p.data.items.push(Item {
                    offset: start,
                    length: raw.len() - start,
                    long: true,
                    type_code: 3,
                    tag: raw.get(offset + 1).copied().unwrap_or(0),
                    disposition: Disposition::Malformed,
                });
                p.diagnose(start, "Truncated long item header");
                break;
            }
            (3, raw[offset + 1], raw[offset] as usize, 3)
        } else {
            (
                (prefix >> 2) & 3,
                prefix >> 4,
                [0, 1, 2, 4][(prefix & 3) as usize],
                1,
            )
        };
        let end = start + header + width;
        let index = p.data.items.len();
        p.data.items.push(Item {
            offset: start,
            length: end.min(raw.len()) - start,
            long,
            type_code,
            tag,
            disposition: Disposition::Interpreted,
        });
        if end > raw.len() {
            p.malformed(index, "Truncated item payload");
            break;
        }
        offset = end;
        if long || type_code == 3 {
            p.data.items[index].disposition = Disposition::Preserved;
            continue;
        }
        let mut bytes = [0; 4];
        bytes[..width].copy_from_slice(&raw[start + 1..end]);
        let value = u32::from_le_bytes(bytes);
        match type_code {
            0 => p.main_item(index, tag, value, width)?,
            1 => p.global(index, tag, value, width)?,
            _ if !matches!(tag,0..=5|7..=10) => {
                p.data.items[index].disposition = Disposition::Preserved
            }
            _ if width == 0 => p.malformed(index, "Local value item has no data"),
            _ => {
                if let Err(reason) = p.local.apply(tag, value, width) {
                    p.malformed(index, reason)
                }
            }
        }
    }
    if !p.collections.is_empty() {
        p.diagnose(raw.len(), "Unclosed Collection stack");
    }
    if !p.stack.is_empty() {
        p.diagnose(raw.len(), "Unclosed global Push stack");
    }
    if p.local.has_items() {
        let (_, _, errors) = std::mem::take(&mut p.local).finish(p.globals.usage_page, None);
        for reason in errors {
            p.diagnose(raw.len(), reason);
        }
        p.diagnose(raw.len(), "Unused local items at end of descriptor");
    }
    let numbered = p.data.layouts.iter().any(|l| l.report_id.is_some());
    if numbered && p.data.layouts.iter().any(|l| l.report_id.is_none()) {
        p.diagnose(0, "Numbered and unnumbered reports are mixed");
    }
    if p.data.fields.is_empty() {
        p.diagnose(0, "Descriptor declares no report fields");
    }
    Ok(Descriptor(p.data))
}

#[cfg(test)]
#[path = "parser/tests.rs"]
mod tests;
