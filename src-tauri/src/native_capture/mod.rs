//! Values and original native dependencies stay paired through projection.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) const MAX_CONTRIBUTORS: usize = 1024;
pub(crate) const MAX_PROJECTION_REFERENCES: usize = 1_048_576;
const MAX_ID: usize = 512;
const MAX_TEXT: usize = 16_384;
const MAX_VALIDITY: Duration = Duration::from_secs(11);
pub(crate) type CapturedValues = BTreeMap<String, CapturedValue>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InputId {
    Byte(u16),
    Channel(Arc<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub producer_id: Arc<str>,
    pub generation: u64,
    pub sequence: u64,
    pub byte_offset: Option<u32>,
    pub captured_at: Instant,
}
impl Stamp {
    fn valid(&self) -> bool {
        id(&self.producer_id) && self.generation > 0 && self.sequence > 0
    }
    fn same_owner(&self, other: &Self) -> bool {
        self.producer_id == other.producer_id && self.generation == other.generation
    }
    fn position(&self) -> (u64, Option<u32>) {
        (self.sequence, self.byte_offset)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lease {
    pub domain_id: Arc<str>,
    pub generation: u64,
    pub observation: Stamp,
    pub valid_until: Instant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Validity {
    Sampled { valid_until: Instant },
    Latched(Arc<Lease>),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Contribution {
    pub input: InputId,
    pub capture: Stamp,
    pub validity: Validity,
}
impl Contribution {
    fn current_at(&self, now: Instant) -> bool {
        if !self.capture.valid() || self.capture.captured_at > now {
            return false;
        }
        match &self.input {
            InputId::Byte(_) if self.capture.byte_offset.is_none() => return false,
            InputId::Channel(name) if !id(name) || self.capture.byte_offset.is_some() => {
                return false
            }
            _ => {}
        }
        match &self.validity {
            Validity::Sampled { valid_until } => {
                interval(self.capture.captured_at, *valid_until) && now < *valid_until
            }
            Validity::Latched(lease) => {
                id(&lease.domain_id)
                    && lease.generation > 0
                    && lease.observation.valid()
                    && self.capture.same_owner(&lease.observation)
                    && self.capture.byte_offset.is_some() == lease.observation.byte_offset.is_some()
                    && self.capture.position() <= lease.observation.position()
                    && (self.capture.position() != lease.observation.position()
                        || self.capture.captured_at == lease.observation.captured_at)
                    && self.capture.captured_at <= lease.observation.captured_at
                    && lease.observation.captured_at <= now
                    && interval(lease.observation.captured_at, lease.valid_until)
                    && now < lease.valid_until
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CapturedValue {
    value: Value,
    // None denotes explicit pure/authored fixture data, never native proof.
    contributors: Option<Arc<[Arc<Contribution>]>>,
}
impl CapturedValue {
    pub fn fixture(value: Value) -> Self {
        Self {
            value,
            contributors: None,
        }
    }
    pub fn native(
        value: Value,
        mut contributors: Vec<Arc<Contribution>>,
        now: Instant,
    ) -> Option<Self> {
        if !scalar(&value)
            || contributors.is_empty()
            || contributors.len() > MAX_CONTRIBUTORS
            || contributors.iter().any(|c| !c.current_at(now))
        {
            return None;
        }
        contributors.sort_by(|a, b| {
            (&a.capture.producer_id, &a.input).cmp(&(&b.capture.producer_id, &b.input))
        });
        if contributors.windows(2).any(|pair| {
            pair[0].capture.producer_id == pair[1].capture.producer_id
                && pair[0].input == pair[1].input
        }) {
            return None;
        }
        Some(Self {
            value,
            contributors: Some(contributors.into()),
        })
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn contributors(&self) -> Option<&[Arc<Contribution>]> {
        self.contributors.as_deref()
    }
    pub fn is_current(&self, now: Instant) -> bool {
        self.contributors
            .as_ref()
            .is_none_or(|items| items.iter().all(|c| c.current_at(now)))
    }
    pub fn authority_age_ms(&self, now: Instant) -> Option<u64> {
        self.contributors.as_ref()?.iter().try_fold(0, |old, c| {
            let at = match &c.validity {
                Validity::Sampled { .. } => c.capture.captured_at,
                Validity::Latched(lease) => lease.observation.captured_at,
            };
            now.checked_duration_since(at)
                .map(|age| old.max(u64::try_from(age.as_millis()).unwrap_or(u64::MAX)))
        })
    }
    pub fn with_value(&self, value: Value) -> Option<Self> {
        scalar(&value).then(|| Self {
            value,
            contributors: self.contributors.clone(),
        })
    }
    pub fn combine(value: Value, inputs: &[&Self], now: Instant) -> Option<Self> {
        if inputs.is_empty() || !scalar(&value) {
            return None;
        }
        if inputs.iter().all(|value| value.contributors.is_none()) {
            return Some(Self::fixture(value));
        }
        let mut merged: BTreeMap<(Arc<str>, InputId), Arc<Contribution>> = BTreeMap::new();
        for input in inputs {
            for contribution in input.contributors.as_ref()?.iter() {
                let key = (
                    contribution.capture.producer_id.clone(),
                    contribution.input.clone(),
                );
                if let Some(previous) = merged.get_mut(&key) {
                    *previous = Arc::new(merge(previous, contribution)?);
                } else {
                    if merged.len() >= MAX_CONTRIBUTORS {
                        return None;
                    }
                    merged.insert(key, contribution.clone());
                }
            }
        }
        Self::native(value, merged.into_values().collect(), now)
    }
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>().saturating_add(value_heap(&self.value));
        if let Some(items) = &self.contributors {
            bytes = bytes
                .saturating_add(2 * std::mem::size_of::<usize>())
                .saturating_add(std::mem::size_of_val(items.as_ref()));
            for c in items.iter() {
                bytes = bytes
                    .saturating_add(
                        std::mem::size_of::<Contribution>() + 2 * std::mem::size_of::<usize>(),
                    )
                    .saturating_add(arc_bytes(&c.capture.producer_id));
                if let InputId::Channel(name) = &c.input {
                    bytes = bytes.saturating_add(arc_bytes(name));
                }
                if let Validity::Latched(lease) = &c.validity {
                    bytes = bytes
                        .saturating_add(
                            std::mem::size_of::<Lease>() + 2 * std::mem::size_of::<usize>(),
                        )
                        .saturating_add(arc_bytes(&lease.domain_id))
                        .saturating_add(arc_bytes(&lease.observation.producer_id));
                }
            }
        }
        bytes
    }
}

/// Caps retained reference arrays independently of the provider's port count.
#[derive(Default)]
pub(crate) struct ProjectionBudget {
    references: usize,
}
impl ProjectionBudget {
    pub fn admit(&mut self, value: &CapturedValue) -> bool {
        let Some(next) = self
            .references
            .checked_add(value.contributors().map_or(0, <[_]>::len))
        else {
            return false;
        };
        if next > MAX_PROJECTION_REFERENCES {
            return false;
        }
        self.references = next;
        true
    }
}
fn merge(a: &Contribution, b: &Contribution) -> Option<Contribution> {
    if a.capture != b.capture {
        return None;
    }
    let validity = match (&a.validity, &b.validity) {
        (Validity::Sampled { valid_until: a }, Validity::Sampled { valid_until: b }) => {
            Validity::Sampled {
                valid_until: (*a).min(*b),
            }
        }
        (Validity::Latched(a), Validity::Latched(b)) => {
            if a.domain_id != b.domain_id
                || a.generation != b.generation
                || !a.observation.same_owner(&b.observation)
            {
                return None;
            }
            if a.observation.position() == b.observation.position()
                && a.observation != b.observation
            {
                return None;
            }
            let latest = if a.observation.position() >= b.observation.position() {
                a
            } else {
                b
            };
            let earlier = if Arc::ptr_eq(latest, a) { b } else { a };
            if latest.observation.captured_at < earlier.observation.captured_at {
                return None;
            }
            let ttl = a
                .valid_until
                .checked_duration_since(a.observation.captured_at)?
                .min(
                    b.valid_until
                        .checked_duration_since(b.observation.captured_at)?,
                );
            Validity::Latched(Arc::new(Lease {
                domain_id: latest.domain_id.clone(),
                generation: latest.generation,
                observation: latest.observation.clone(),
                valid_until: latest.observation.captured_at.checked_add(ttl)?,
            }))
        }
        _ => return None,
    };
    Some(Contribution {
        input: a.input.clone(),
        capture: a.capture.clone(),
        validity,
    })
}
fn id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID && !value.chars().any(char::is_control)
}
fn interval(start: Instant, end: Instant) -> bool {
    end.checked_duration_since(start)
        .is_some_and(|d| !d.is_zero() && d <= MAX_VALIDITY)
}
fn scalar(value: &Value) -> bool {
    value.is_boolean()
        || value.as_f64().is_some_and(f64::is_finite)
        || value.as_str().is_some_and(|s| s.len() <= MAX_TEXT)
}
fn arc_bytes(value: &str) -> usize {
    value
        .len()
        .saturating_add(3 * std::mem::size_of::<usize>() - 1)
}
fn value_heap(value: &Value) -> usize {
    let mut pending = vec![value];
    let mut bytes = 0usize;
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => bytes = bytes.saturating_add(text.capacity()),
            Value::Array(items) => {
                bytes = bytes.saturating_add(
                    items
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Value>()),
                );
                pending.extend(items);
            }
            Value::Object(items) => {
                for (key, value) in items {
                    bytes = bytes
                        .saturating_add(key.capacity())
                        .saturating_add(std::mem::size_of::<Value>() + 128);
                    pending.push(value);
                }
            }
            _ => {}
        }
    }
    bytes
}

#[cfg(test)]
mod tests;
