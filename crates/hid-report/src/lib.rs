//! Portable HID report descriptor parsing and read-only report interpretation.
mod decode;
mod encoding;
mod flags;
mod model;
mod parser;

pub use decode::decode_report;
pub use flags::MainFlags;
pub use model::*;
pub use parser::parse_descriptor;
