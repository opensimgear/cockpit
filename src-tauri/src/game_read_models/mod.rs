//! Passive, source-driven game projections and immutable, issued page data.
pub(crate) mod signals;
mod pages;
pub(crate) use pages::Pages as RecordPages;
mod codec;
mod runtime;
mod details;
pub(crate) use runtime::Runtime;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) struct Parts {
    pub catalogue: Value,
    pub meta: Value,
    pub discovery: Option<Value>,
    pub details: BTreeMap<String,Value>,
    pub library_revision:u64,
}
