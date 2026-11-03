//! A bounded stream decoder that keeps each payload byte's acquisition mark.
use super::CapturedDatagram;
use std::time::Instant;

const MAX_FRAME: usize = 131_072;
const MAX_CAPTURED_EVENTS: usize = 16;
const EVENT_OVERFLOW: &str = "DCS export datagram exceeds the decoded event limit.";

#[derive(Debug)]
pub(crate) struct Block {
    pub(crate) address: u16,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum ExportEvent {
    Frame(Vec<Block>),
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CapturePosition {
    pub report_sequence: u64,
    pub byte_offset: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ByteCapture {
    pub position: CapturePosition,
    pub captured_at: Instant,
}

#[derive(Debug)]
pub(crate) struct CapturedBlock {
    block: Block,
    captures: Vec<ByteCapture>,
}

impl CapturedBlock {
    pub(crate) fn new(block: Block, captures: Vec<ByteCapture>) -> Result<Self, String> {
        if block.data.len() != captures.len() {
            return Err("DCS export payload and capture marks are not aligned.".into());
        }
        Ok(Self { block, captures })
    }

    pub(crate) fn block(&self) -> &Block {
        &self.block
    }

    pub(crate) fn captures(&self) -> &[ByteCapture] {
        &self.captures
    }
}

#[derive(Debug)]
pub(crate) enum CapturedExportEvent {
    Frame(Vec<CapturedBlock>),
    Reset,
}

struct MarkedBlock<M> {
    block: Block,
    captures: Vec<M>,
}

enum CoreEvent<M> {
    Frame(Vec<MarkedBlock<M>>),
    Reset,
}

/// At most seven pending marker candidates retain marks. Eight or more 0x55
/// bytes form a saturated sync run, so no preceding payload is replayed.
pub(crate) struct ParserCore<M> {
    counter_address: u16,
    sync: usize,
    sync_captures: Vec<M>,
    active: bool,
    header: Vec<u8>,
    address: u16,
    remaining: usize,
    data: Vec<u8>,
    captures: Vec<M>,
    blocks: Vec<MarkedBlock<M>>,
    frame_bytes: usize,
}

impl<M> ParserCore<M> {
    pub(crate) fn new(counter_address: u16) -> Self {
        Self {
            counter_address,
            sync: 0,
            sync_captures: Vec::new(),
            active: false,
            header: Vec::new(),
            address: 0,
            remaining: 0,
            data: Vec::new(),
            captures: Vec::new(),
            blocks: Vec::new(),
            frame_bytes: 0,
        }
    }

    pub(crate) fn discard(&mut self) {
        self.active = false;
        self.header.clear();
        self.remaining = 0;
        self.data.clear();
        self.captures.clear();
        self.blocks.clear();
        self.frame_bytes = 0;
    }

    pub(crate) fn reset(&mut self) {
        self.discard();
        self.sync = 0;
        self.sync_captures.clear();
    }

    fn emit(
        events: &mut Vec<CoreEvent<M>>,
        event: CoreEvent<M>,
        limit: Option<usize>,
    ) -> Result<(), String> {
        if limit.is_some_and(|limit| events.len() >= limit) {
            return Err(EVENT_OVERFLOW.into());
        }
        events.push(event);
        Ok(())
    }

    fn byte(
        &mut self,
        byte: u8,
        capture: M,
        events: &mut Vec<CoreEvent<M>>,
        limit: Option<usize>,
    ) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        if self.remaining > 0 {
            self.data.push(byte);
            self.captures.push(capture);
            self.remaining -= 1;
            if self.remaining == 0 {
                let complete = self.address <= self.counter_address
                    && self.address as usize + self.data.len() >= self.counter_address as usize + 2;
                self.blocks.push(MarkedBlock {
                    block: Block {
                        address: self.address,
                        data: std::mem::take(&mut self.data),
                    },
                    captures: std::mem::take(&mut self.captures),
                });
                if complete {
                    Self::emit(
                        events,
                        CoreEvent::Frame(std::mem::take(&mut self.blocks)),
                        limit,
                    )?;
                    self.discard();
                }
            }
        } else {
            self.header.push(byte);
            if self.header.len() == 4 {
                self.address = u16::from_le_bytes([self.header[0], self.header[1]]);
                let length = u16::from_le_bytes([self.header[2], self.header[3]]) as usize;
                self.header.clear();
                self.frame_bytes += length + 4;
                if length == 0
                    || length % 2 != 0
                    || self.address % 2 != 0
                    || self.address as usize + length > 65536
                    || self.frame_bytes > MAX_FRAME
                    || self.blocks.len() >= 8192
                {
                    self.discard();
                    Self::emit(events, CoreEvent::Reset, limit)?;
                } else {
                    self.remaining = length;
                }
            }
        }
        Ok(())
    }

    fn feed_marked(
        &mut self,
        bytes: impl Iterator<Item = (u8, M)>,
        limit: Option<usize>,
    ) -> Result<Vec<CoreEvent<M>>, String> {
        let mut events = Vec::new();
        let result = (|| {
            for (byte, capture) in bytes {
                if byte == 0x55 {
                    self.sync = (self.sync + 1).min(8);
                    if self.sync < 8 {
                        self.sync_captures.push(capture);
                    } else {
                        self.sync_captures.clear();
                    }
                    continue;
                }
                let run = std::mem::take(&mut self.sync);
                let marks = std::mem::take(&mut self.sync_captures);
                if run >= 4 {
                    if run <= 7 {
                        for capture in marks.into_iter().take(run - 4) {
                            self.byte(0x55, capture, &mut events, limit)?;
                        }
                    }
                    if self.active
                        && (!self.blocks.is_empty()
                            || !self.header.is_empty()
                            || self.remaining > 0)
                    {
                        Self::emit(&mut events, CoreEvent::Reset, limit)?;
                    }
                    self.discard();
                    self.active = true;
                } else {
                    for capture in marks {
                        self.byte(0x55, capture, &mut events, limit)?;
                    }
                }
                self.byte(byte, capture, &mut events, limit)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.reset();
            return Err(error);
        }
        Ok(events)
    }
}

pub(crate) type Parser = ParserCore<()>;

impl ParserCore<()> {
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<ExportEvent> {
        self.feed_marked(bytes.iter().map(|&byte| (byte, ())), None)
            .expect("the pure decoder has no event limit")
            .into_iter()
            .map(|event| match event {
                CoreEvent::Frame(blocks) => {
                    ExportEvent::Frame(blocks.into_iter().map(|block| block.block).collect())
                }
                CoreEvent::Reset => ExportEvent::Reset,
            })
            .collect()
    }
}

pub(crate) struct CapturedParser {
    core: ParserCore<ByteCapture>,
}

impl CapturedParser {
    pub(crate) fn new(counter_address: u16) -> Self {
        Self {
            core: ParserCore::new(counter_address),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.core.reset();
    }

    pub(crate) fn feed(
        &mut self,
        report: &CapturedDatagram,
    ) -> Result<Vec<CapturedExportEvent>, String> {
        if report.sequence == 0 || report.bytes.len() > u32::MAX as usize {
            self.reset();
            return Err("DCS export capture has an invalid report sequence or byte range.".into());
        }
        let bytes = report.bytes.iter().enumerate().map(|(offset, &byte)| {
            (
                byte,
                ByteCapture {
                    position: CapturePosition {
                        report_sequence: report.sequence,
                        byte_offset: offset as u32,
                    },
                    captured_at: report.captured_at,
                },
            )
        });
        self.core
            .feed_marked(bytes, Some(MAX_CAPTURED_EVENTS))?
            .into_iter()
            .map(|event| match event {
                CoreEvent::Frame(blocks) => blocks
                    .into_iter()
                    .map(|block| CapturedBlock::new(block.block, block.captures))
                    .collect::<Result<Vec<_>, _>>()
                    .map(CapturedExportEvent::Frame),
                CoreEvent::Reset => Ok(CapturedExportEvent::Reset),
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
