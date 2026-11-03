use super::super::*;
use super::*;
use std::io::{self, Write};

struct Bounded {
    remaining: usize,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("Native byte budget exceeded."));
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn within_bytes(value: &impl Serialize, limit: usize) -> bool {
    serde_json::to_writer(Bounded { remaining: limit }, value).is_ok()
}
impl Phase {
    pub(super) fn check_native_proof_budget<'a>(
        &'a self,
        extra: impl IntoIterator<Item = &'a NativeProof>,
    ) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        let mut count = 0usize;
        let proofs = self
            .values
            .values()
            .map(|value| &value.proof)
            .chain(self.effects.iter().map(|effect| &effect.native_proof))
            .chain(self.timers.values().map(|timer| &timer.proof))
            .chain(self.states.values().flat_map(|state| {
                state
                    .bindings
                    .values()
                    .chain(std::iter::once(&state.rule_proof))
            }))
            .chain(extra);
        for proof in proofs {
            for binding in &proof.bindings {
                if seen.insert(std::sync::Arc::as_ptr(binding) as usize) {
                    count = count
                        .checked_add(binding.captures.len())
                        .ok_or("Native proof reference budget overflow.")?;
                    if count > MAX_NATIVE_REFERENCES {
                        return Err(
                            "Retained native causal proofs exceed their reference budget.".into(),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
