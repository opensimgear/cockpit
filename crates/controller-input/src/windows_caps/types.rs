use serde::Serialize;

pub const MAX_CAPS: usize = 8192;
pub const MAX_LINKS: usize = 4096;
pub const MAX_FAILURES: usize = 64;
pub const MAX_SAMPLE_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}
impl ReportKind {
    pub(crate) fn native(self) -> i32 {
        match self {
            Self::Input => 0,
            Self::Output => 1,
            Self::Feature => 2,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub stage: String,
    pub kind: Option<ReportKind>,
    pub status: Option<i32>,
    pub message: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRange {
    pub is_range: bool,
    pub minimum: u16,
    pub maximum: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub usage_page: u16,
    pub report_id: u8,
    pub is_alias: bool,
    pub bit_field: u16,
    pub link_collection: u16,
    pub link_usage: u16,
    pub link_usage_page: u16,
    pub is_absolute: bool,
    pub usages: IndexRange,
    pub strings: IndexRange,
    pub designators: IndexRange,
    pub data_indices: IndexRange,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Button {
    #[serde(flatten)]
    pub common: Capability,
    /// This word was reserved before HidP API version 2; never expose it then.
    pub report_count: Option<u16>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Value {
    #[serde(flatten)]
    pub common: Capability,
    pub bit_size: u16,
    pub report_count: u16,
    pub has_null: bool,
    #[serde(serialize_with = "wire::decimal")]
    pub units: u32,
    #[serde(serialize_with = "wire::decimal")]
    pub units_exp_raw: u32,
    pub unit_exponent: i8,
    #[serde(serialize_with = "wire::decimal")]
    pub logical_min_raw: i32,
    #[serde(serialize_with = "wire::decimal")]
    pub logical_max_raw: i32,
    #[serde(serialize_with = "wire::decimal")]
    pub physical_min_raw: i32,
    #[serde(serialize_with = "wire::decimal")]
    pub physical_max_raw: i32,
    #[serde(serialize_with = "wire::decimal")]
    pub logical_min: i64,
    #[serde(serialize_with = "wire::decimal")]
    pub logical_max: i64,
    #[serde(serialize_with = "wire::decimal")]
    pub physical_min: i64,
    #[serde(serialize_with = "wire::decimal")]
    pub physical_max: i64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub index: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub parent: u16,
    pub number_of_children: u16,
    pub next_sibling: u16,
    pub first_child: u16,
    pub collection_type: u8,
    pub is_alias: bool,
    #[serde(serialize_with = "wire::decimal")]
    pub raw_flags: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub kind: ReportKind,
    pub byte_length: u16,
    pub data_indices: u16,
    pub declared_button_caps: u16,
    pub declared_value_caps: u16,
    pub report_ids: Vec<u8>,
    pub button_caps: Vec<Button>,
    pub value_caps: Vec<Value>,
    pub complete: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub provenance: String,
    pub available: bool,
    pub complete: bool,
    pub api_version: ApiVersion,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
    pub declared_link_count: u16,
    pub reports: Vec<Report>,
    pub links: Vec<Link>,
    pub failures: Vec<Failure>,
}
impl Snapshot {
    pub fn unavailable(reason: &str) -> Self {
        Self {
            provenance: "windows-hidp-preparsed-capabilities".into(),
            available: false,
            complete: false,
            api_version: ApiVersion::unavailable(),
            usage_page: None,
            usage: None,
            declared_link_count: 0,
            reports: Vec::new(),
            links: Vec::new(),
            failures: vec![Failure {
                stage: "preparsed".into(),
                kind: None,
                status: None,
                message: reason.into(),
            }],
        }
    }
    pub fn retained_records(&self) -> usize {
        self.links.len()
            + self
                .reports
                .iter()
                .map(|r| r.button_caps.len() + r.value_caps.len())
                .sum::<usize>()
    }
    pub fn report(&self, kind: ReportKind) -> Option<&Report> {
        self.reports.iter().find(|r| r.kind == kind)
    }
    pub(crate) fn fail(
        &mut self,
        stage: &str,
        kind: Option<ReportKind>,
        status: Option<i32>,
        message: &str,
    ) {
        self.complete = false;
        if self.failures.len() < MAX_FAILURES {
            self.failures.push(Failure {
                stage: stage.into(),
                kind,
                status,
                message: message.into(),
            });
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiVersion {
    pub number: Option<u32>,
    pub source: String,
    pub status: Option<i32>,
}
impl ApiVersion {
    pub(crate) fn unavailable() -> Self {
        Self {
            number: None,
            source: "unavailable".into(),
            status: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataValue {
    pub data_index: u16,
    #[serde(serialize_with = "wire::decimal")]
    pub raw_value: u32,
    pub quality: String,
    pub matching_button_caps: Vec<usize>,
    pub matching_value_caps: Vec<usize>,
    pub values: Vec<ScalarValue>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScalarValue {
    pub capability_index: usize,
    #[serde(serialize_with = "wire::optional_decimal")]
    pub native: Option<i64>,
    pub quality: String,
    pub semantics: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueArray {
    pub capability_index: usize,
    pub usage_page: u16,
    pub usage: u16,
    pub link_collection: u16,
    pub bit_size: u16,
    pub report_count: u16,
    #[serde(serialize_with = "wire::hex")]
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Samples {
    pub provenance: String,
    pub kind: ReportKind,
    pub report_id: Option<u8>,
    #[serde(serialize_with = "wire::hex")]
    pub raw: Vec<u8>,
    pub data: Vec<DataValue>,
    pub value_arrays: Vec<ValueArray>,
    pub failures: Vec<Failure>,
}

mod wire {
    use serde::Serializer;
    use std::fmt::Display;

    pub fn decimal<T: Display, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn optional_decimal<S: Serializer>(
        value: &Option<i64>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
    pub fn hex<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::new();
        for byte in bytes {
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 15)] as char);
        }
        serializer.serialize_str(&encoded)
    }
}
