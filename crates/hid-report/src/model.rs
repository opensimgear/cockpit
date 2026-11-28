use serde::Serialize;
use std::ops::Deref;

pub const MAX_DESCRIPTOR_BYTES: usize = 65_536;
pub const MAX_REPORT_BYTES: usize = 65_536;
pub const MAX_DEPTH: usize = 64;
pub const MAX_FIELDS: usize = 16_384;
pub const MAX_SCALAR_BITS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Usage {
    pub page: u16,
    pub id: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum LocalIndex<T> {
    Single(T),
    Range { minimum: T, maximum: T },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Locals {
    pub delimited: bool,
    /// Position occupied by a delimiter group among sequential control usages.
    pub control_index: Option<usize>,
    pub usages: Vec<LocalIndex<Usage>>,
    pub strings: Vec<LocalIndex<u32>>,
    pub designators: Vec<LocalIndex<u32>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Item {
    pub offset: usize,
    pub length: usize,
    pub long: bool,
    pub type_code: u8,
    pub tag: u8,
    pub disposition: Disposition,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Disposition {
    Interpreted,
    Preserved,
    Malformed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Globals {
    pub usage_page: u16,
    #[serde(serialize_with = "crate::encoding::bound")]
    pub logical_min: Option<i64>,
    #[serde(serialize_with = "crate::encoding::bound")]
    pub logical_max: Option<i64>,
    #[serde(serialize_with = "crate::encoding::bound")]
    pub physical_min: Option<i64>,
    #[serde(serialize_with = "crate::encoding::bound")]
    pub physical_max: Option<i64>,
    pub unit: Option<u32>,
    pub exponent: Option<i8>,
    pub report_id: Option<u8>,
    pub size: usize,
    pub count: usize,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Field {
    pub item_index: usize,
    pub collection: Option<usize>,
    pub kind: ReportKind,
    pub offset_bits: usize,
    pub flags: u32,
    pub globals: Globals,
    pub primary: Locals,
    pub alternatives: Vec<Locals>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Collection {
    pub parent: Option<usize>,
    pub item_index: usize,
    pub type_code: u8,
    pub primary: Locals,
    pub alternatives: Vec<Locals>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Layout {
    pub kind: ReportKind,
    pub report_id: Option<u8>,
    pub payload_bits: usize,
    pub fields: Vec<usize>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub offset: usize,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DescriptorData {
    #[serde(serialize_with = "crate::encoding::hex")]
    pub raw: Vec<u8>,
    pub items: Vec<Item>,
    pub collections: Vec<Collection>,
    pub fields: Vec<Field>,
    pub layouts: Vec<Layout>,
    pub diagnostics: Vec<Diagnostic>,
}
/// Only the parser can construct a descriptor. Consumers can inspect but cannot
/// mutate the offsets, field references or budget checks used by the decoder.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Descriptor(pub(crate) DescriptorData);
impl Deref for Descriptor {
    type Target = DescriptorData;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Framing {
    IdPrefix,
    Unnumbered,
    OsZeroPrefix,
    OutOfBand(Option<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum Sample {
    Integer(String),
    Array {
        selector: String,
        usages: Vec<Usage>,
    },
    Bytes(#[serde(serialize_with = "crate::encoding::hex")] Vec<u8>),
    Null(String),
    Invalid(String),
    Unknown {
        reason: String,
        encoded: Option<String>,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Semantics {
    Absolute,
    Relative,
    Event,
    Opaque,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Physical {
    pub numerator: String,
    pub denominator: String,
    pub unit: u32,
    pub exponent: i8,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Value {
    pub field: usize,
    pub element: usize,
    #[serde(serialize_with = "crate::encoding::hex")]
    pub raw_bits: Vec<u8>,
    pub sample: Sample,
    pub usages: Vec<Usage>,
    pub semantics: Semantics,
    pub physical: Option<Physical>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecodedReport {
    pub kind: ReportKind,
    pub framing: Framing,
    pub report_id: Option<u8>,
    #[serde(serialize_with = "crate::encoding::hex")]
    pub raw: Vec<u8>,
    pub values: Vec<Value>,
    pub diagnostics: Vec<Diagnostic>,
}
