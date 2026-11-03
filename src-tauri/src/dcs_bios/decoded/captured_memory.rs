//! Sparse per-byte capture provenance alongside the native decoded memory.
use super::Memory;
use crate::dcs_bios::{ByteCapture, CapturedBlock};
use std::{
    ops::{Deref, Range},
    sync::Arc,
};

const ADDRESS_SPACE: usize = 65_536;
const PAGE_SIZE: usize = 256;
const PAGE_COUNT: usize = ADDRESS_SPACE / PAGE_SIZE;

#[derive(Clone)]
struct CapturePage {
    cells: [Option<ByteCapture>; PAGE_SIZE],
}

impl CapturePage {
    fn empty() -> Self {
        Self {
            cells: [None; PAGE_SIZE],
        }
    }
}

/// A memory image whose bytes and capture evidence share the same lifetime.
#[derive(Clone)]
pub(crate) struct CapturedMemory {
    memory: Memory,
    acquisition_id: String,
    pages: [Option<Arc<CapturePage>>; PAGE_COUNT],
}

impl CapturedMemory {
    pub(crate) const fn max_retained_bytes() -> usize {
        std::mem::size_of::<Self>()
            + (2 * ADDRESS_SPACE)
            + 512
            + PAGE_COUNT * (std::mem::size_of::<CapturePage>() + 2 * std::mem::size_of::<usize>())
    }

    pub(crate) fn new(acquisition_id: String) -> Self {
        Self {
            memory: Memory::new(),
            acquisition_id,
            pages: std::array::from_fn(|_| None),
        }
    }

    pub(crate) fn acquisition_id(&self) -> &str {
        &self.acquisition_id
    }

    /// Validates the whole batch before changing either bytes or provenance.
    pub(crate) fn apply(&mut self, blocks: &[CapturedBlock]) -> Result<(), String> {
        if self.acquisition_id.is_empty() {
            return Err("Cannot apply captured DCS-BIOS memory without an acquisition ID.".into());
        }
        for block in blocks {
            let data = &block.block().data;
            let address = block.block().address as usize;
            if address % 2 != 0
                || data.is_empty()
                || data.len() % 2 != 0
                || address
                    .checked_add(data.len())
                    .map_or(true, |end| end > ADDRESS_SPACE)
                || block.captures().len() != data.len()
                || block
                    .captures()
                    .iter()
                    .any(|capture| capture.position.report_sequence == 0)
            {
                return Err("Invalid captured DCS-BIOS memory block.".into());
            }
        }

        for block in blocks {
            let raw = block.block();
            let start = raw.address as usize;
            let end = start + raw.data.len();
            self.memory.bytes[start..end].copy_from_slice(&raw.data);
            self.memory.valid[start..end].fill(true);
            for (address, capture) in (start..end).zip(block.captures().iter().copied()) {
                self.set_capture(address, Some(capture));
            }
        }
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.memory.clear();
        self.pages = std::array::from_fn(|_| None);
    }

    /// Copies a range including invalid-cell bytes and all available provenance.
    pub(crate) fn copy_range_from(
        &mut self,
        other: &Self,
        range: Range<usize>,
    ) -> Result<(), String> {
        if self.acquisition_id != other.acquisition_id {
            return Err("Cannot copy captured memory across acquisitions.".into());
        }
        if range.start > range.end || range.end > ADDRESS_SPACE {
            return Err("Captured memory copy range is out of bounds.".into());
        }

        self.memory.bytes[range.clone()].copy_from_slice(&other.memory.bytes[range.clone()]);
        self.memory.valid[range.clone()].copy_from_slice(&other.memory.valid[range.clone()]);
        if range.is_empty() {
            return Ok(());
        }

        let first_page = range.start / PAGE_SIZE;
        let last_page = (range.end - 1) / PAGE_SIZE;
        for page_index in first_page..=last_page {
            let page_start = page_index * PAGE_SIZE;
            let lo = range.start.max(page_start) - page_start;
            let hi = range.end.min(page_start + PAGE_SIZE) - page_start;
            let source_has_capture = other.pages[page_index]
                .as_ref()
                .is_some_and(|page| page.cells[lo..hi].iter().any(Option::is_some));
            if self.pages[page_index].is_none() && !source_has_capture {
                continue;
            }
            let destination =
                self.pages[page_index].get_or_insert_with(|| Arc::new(CapturePage::empty()));
            let destination = Arc::make_mut(destination);
            for offset in lo..hi {
                destination.cells[offset] = other.pages[page_index]
                    .as_ref()
                    .and_then(|page| page.cells[offset]);
            }
            if destination.cells.iter().all(Option::is_none) {
                self.pages[page_index] = None;
            }
        }
        Ok(())
    }

    pub(crate) fn capture(&self, address: u16) -> Option<ByteCapture> {
        let index = address as usize;
        if !self.memory.valid[index] {
            return None;
        }
        self.pages[index / PAGE_SIZE]
            .as_ref()
            .and_then(|page| page.cells[index % PAGE_SIZE])
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        let base = std::mem::size_of::<Self>()
            .saturating_add(self.memory.bytes.capacity())
            .saturating_add(self.memory.valid.capacity())
            .saturating_add(self.acquisition_id.capacity());
        let page_allocation =
            std::mem::size_of::<CapturePage>().saturating_add(2 * std::mem::size_of::<usize>());
        self.pages
            .iter()
            .filter(|page| page.is_some())
            .count()
            .saturating_mul(page_allocation)
            .saturating_add(base)
    }

    fn set_capture(&mut self, address: usize, capture: Option<ByteCapture>) {
        let page = &mut self.pages[address / PAGE_SIZE];
        if page.is_none() {
            *page = Some(Arc::new(CapturePage::empty()));
        }
        Arc::make_mut(page.as_mut().expect("capture page was allocated")).cells
            [address % PAGE_SIZE] = capture;
    }
}

impl Deref for CapturedMemory {
    type Target = Memory;

    fn deref(&self) -> &Self::Target {
        &self.memory
    }
}

#[cfg(test)]
#[path = "captured_memory_tests.rs"]
mod tests;
