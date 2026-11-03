//! Bounded conversion of original native captures into portable table references.
use super::*;
use crate::native_capture::{Contribution, InputId, Stamp, Validity};
use std::time::Duration;

pub(super) struct Tables {
    clock: osg_controller_input::stream::Clock,
    pub captures: Vec<NativeCapture>,
    pub leases: Vec<NativeLease>,
    positions: BTreeMap<(String, u64, u64, Option<u32>), (u32, Instant)>,
    references: usize,
    bytes: usize,
}
impl Tables {
    pub fn new(clock: osg_controller_input::stream::Clock) -> Self {
        Self {
            clock,
            captures: vec![],
            leases: vec![],
            positions: BTreeMap::new(),
            references: 0,
            bytes: 0,
        }
    }
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or("Native frame budget overflow")?;
        if self.bytes > MAX_NATIVE_FRAME_BYTES {
            return Err("Native frame budget exceeded".into());
        }
        Ok(())
    }
    fn capture(&mut self, stamp: &Stamp) -> Result<u32, String> {
        let capture = NativeCapture {
            producer_id: stamp.producer_id.to_string(),
            generation: Counter(stamp.generation),
            sequence: Counter(stamp.sequence),
            byte_offset: stamp.byte_offset,
            captured_us: Counter(self.clock.captured_us(stamp.captured_at)?),
        };
        let key = (
            capture.producer_id.clone(),
            stamp.generation,
            stamp.sequence,
            stamp.byte_offset,
        );
        if let Some((index, original_at)) = self.positions.get(&key) {
            if *original_at != stamp.captured_at || self.captures[*index as usize] != capture {
                return Err("Conflicting original native capture".into());
            }
            return Ok(*index);
        }
        if capture.producer_id.len() > MAX_NATIVE_ID_BYTES
            || self.captures.len() >= MAX_NATIVE_CAPTURES
        {
            return Err("Native capture table limit".into());
        }
        self.charge(512 + capture.producer_id.len() * 6)?;
        let index =
            u32::try_from(self.captures.len()).map_err(|_| "Native capture reference overflow")?;
        self.positions.insert(key, (index, stamp.captured_at));
        self.captures.push(capture);
        Ok(index)
    }
    pub fn contributors(
        &mut self,
        items: &[std::sync::Arc<Contribution>],
        policy: NativePolicy,
        freshness: u64,
    ) -> Result<Vec<NativeContributor>, String> {
        let mut result = Vec::with_capacity(items.len());
        for item in items {
            self.references = self
                .references
                .checked_add(1)
                .ok_or("Native reference overflow")?;
            if self.references > MAX_NATIVE_REFERENCES {
                return Err("Native reference limit".into());
            }
            let capture = self.capture(&item.capture)?;
            let validity = match (&item.validity, policy) {
                (Validity::Sampled { valid_until }, NativePolicy::Sampled) => {
                    let policy_end = item
                        .capture
                        .captured_at
                        .checked_add(Duration::from_millis(freshness))
                        .ok_or("Native freshness overflow")?;
                    NativeValidity::Sampled {
                        valid_until_us: Counter(
                            self.clock.deadline_us((*valid_until).min(policy_end))?,
                        ),
                    }
                }
                (Validity::Latched(lease), NativePolicy::Latched) => {
                    let observation = self.capture(&lease.observation)?;
                    let value = NativeLease {
                        domain_id: lease.domain_id.to_string(),
                        generation: Counter(lease.generation),
                        observation,
                        valid_until_us: Counter(
                            self.clock.deadline_us(lease.valid_until)?,
                        ),
                    };
                    let index =
                        if let Some(index) = self.leases.iter().position(|old| old == &value) {
                            index
                        } else {
                            if value.domain_id.len() > MAX_NATIVE_ID_BYTES
                                || self.leases.len() >= MAX_NATIVE_LEASES
                            {
                                return Err("Native lease table limit".into());
                            }
                            self.charge(512 + value.domain_id.len() * 6)?;
                            self.leases.push(value);
                            self.leases.len() - 1
                        };
                    NativeValidity::Latched {
                        lease: u32::try_from(index)
                            .map_err(|_| "Native lease reference overflow")?,
                    }
                }
                _ => return Err("Native validity differs from prepared policy".into()),
            };
            let input = match &item.input {
                InputId::Byte(value) => NativeInput::Byte(*value),
                InputId::Channel(value) => NativeInput::Channel(value.to_string()),
            };
            self.charge(
                512 + match &input {
                    NativeInput::Channel(name) => name.len() * 6,
                    _ => 0,
                },
            )?;
            result.push(NativeContributor {
                input,
                capture,
                validity,
            });
        }
        Ok(result)
    }
}
