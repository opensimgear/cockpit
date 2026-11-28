//! Borrow JSON containers without constructing the archive's decoded report tree.
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, fmt};
pub(super) struct Fields<'a>(pub BTreeMap<String, &'a RawValue>);
impl<'de> Deserialize<'de> for Fields<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Fields<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded HID recording object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key.len() > 128 || fields.len() >= 128 || fields.contains_key(&key) {
                        return Err(serde::de::Error::custom(
                            "Recording metadata fields exceed bounds or duplicate a key",
                        ));
                    }
                    fields.insert(key, map.next_value::<&'de RawValue>()?);
                }
                Ok(Fields(fields))
            }
        }
        deserializer.deserialize_map(Read)
    }
}
pub(super) struct Rows<'a>(pub Vec<&'a RawValue>);
impl<'de> Deserialize<'de> for Rows<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Rows<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded HID recording rows")
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut rows = Vec::new();
                while let Some(row) = seq.next_element::<&'de RawValue>()? {
                    if rows.len() >= 1_000_000 {
                        return Err(serde::de::Error::custom("Recording row capacity exceeded"));
                    }
                    rows.push(row);
                }
                Ok(Rows(rows))
            }
        }
        deserializer.deserialize_seq(Read)
    }
}
