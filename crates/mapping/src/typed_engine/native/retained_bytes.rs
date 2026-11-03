//! Conservative checked heap accounting; opaque proof payloads never enter IPC.
use super::super::*;
use super::*;
use std::mem::{size_of, size_of_val};

const MAP_NODE: usize = 128;
#[derive(Default)]
struct Bytes {
    used: usize,
    bindings: BTreeSet<usize>,
}
impl Bytes {
    fn add(&mut self, bytes: usize) -> Result<(), String> {
        self.used = self
            .used
            .checked_add(bytes)
            .ok_or("Retained native byte budget overflow.")?;
        if self.used > MAX_ADMISSION_BYTES {
            return Err("Retained native evidence exceeds its byte budget.".into());
        }
        Ok(())
    }
    fn text(&mut self, text: &String) -> Result<(), String> {
        self.add(text.capacity())
    }
    fn entry<K, V>(&mut self, key: &K, value: &V) -> Result<(), String> {
        self.add(MAP_NODE + 3 * (size_of_val(key) + size_of_val(value)))
    }
    fn endpoint(&mut self, endpoint: &Endpoint) -> Result<(), String> {
        self.text(&endpoint.node_id)?;
        self.text(&endpoint.port_id)
    }
    fn input(&mut self, input: &NativeInput) -> Result<(), String> {
        if let NativeInput::Channel(channel) = input {
            self.text(channel)?;
        }
        Ok(())
    }
    fn owner(&mut self, owner: &ledger::Owner) -> Result<(), String> {
        self.text(&owner.producer)?;
        if let Some((domain, _)) = &owner.domain {
            self.text(domain)?;
        }
        Ok(())
    }
    fn sample(&mut self, sample: &SignalSample) -> Result<(), String> {
        fn scalar(bytes: &mut Bytes, value: &PortValue) -> Result<(), String> {
            if let PortValue::Text(value) = value {
                bytes.text(value)?;
            }
            Ok(())
        }
        match sample {
            SignalSample::Known(SignalValue::Scalar(value)) => scalar(self, value)?,
            SignalSample::Known(SignalValue::ActionPack(pack)) => {
                self.add(
                    pack.arguments
                        .capacity()
                        .checked_mul(size_of::<NamedArgument>())
                        .ok_or("Native sample byte overflow.")?,
                )?;
                for argument in &pack.arguments {
                    self.text(&argument.id)?;
                    scalar(self, &argument.value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn proof(&mut self, proof: &NativeProof) -> Result<(), String> {
        self.add(
            proof
                .bindings
                .capacity()
                .checked_mul(size_of::<std::sync::Arc<proof::Binding>>())
                .ok_or("Native reference byte overflow.")?,
        )?;
        if !proof.unproven.is_empty() {
            self.add(MAP_NODE + 11 * size_of::<Endpoint>())?;
        }
        for endpoint in &proof.unproven {
            self.add(MAP_NODE + 3 * size_of_val(endpoint))?;
            self.endpoint(endpoint)?;
        }
        for binding in &proof.bindings {
            if !self
                .bindings
                .insert(std::sync::Arc::as_ptr(binding) as usize)
            {
                continue;
            }
            self.add(size_of::<proof::Binding>() + 2 * size_of::<usize>())?;
            self.endpoint(&binding.endpoint)?;
            self.owner(&binding.owner)?;
            self.add(
                binding
                    .captures
                    .capacity()
                    .checked_mul(size_of::<(NativeInput, NativeCapture, proof::Authority)>())
                    .ok_or("Native capture byte overflow.")?,
            )?;
            for (input, capture, authority) in &binding.captures {
                self.input(input)?;
                self.text(&capture.producer_id)?;
                if let proof::Authority::Latched { domain, .. } = authority {
                    self.text(domain)?;
                }
            }
        }
        Ok(())
    }
    fn ledger(&mut self, ledger: &Ledger) -> Result<(), String> {
        // Reserve root-node slack as well as conservative per-entry occupancy.
        self.add(size_of::<Ledger>() + 16 * 1024)?;
        if let Some(clock) = &ledger.clock {
            self.text(clock)?;
        }
        for (source, registration) in &ledger.registrations {
            self.entry(source, registration)?;
            self.text(source)?;
            self.text(&registration.source)?;
            self.text(&registration.clock_id)?;
            self.add(
                registration
                    .ports
                    .capacity()
                    .checked_mul(size_of::<NativePortRegistration>())
                    .ok_or("Native registration byte overflow.")?,
            )?;
            for port in &registration.ports {
                self.text(&port.port_id)?;
                self.text(&port.family)?;
            }
        }
        for (source, state) in &ledger.sources {
            self.entry(source, state)?;
            self.text(source)?;
        }
        for (endpoint, port) in &ledger.ports {
            self.entry(endpoint, port)?;
            self.endpoint(endpoint)?;
            if let Some(owner) = &port.owner {
                self.owner(owner)?;
            }
            if let Some(sample) = &port.sample {
                self.sample(sample)?;
            }
            self.proof(&port.proof)?;
        }
        for (key, capture) in &ledger.inputs {
            self.entry(key, capture)?;
            self.text(&key.0)?;
            self.input(&key.2)?;
            self.text(&capture.producer_id)?;
        }
        for (producer, generation) in &ledger.producers {
            self.entry(producer, generation)?;
            self.text(producer)?;
        }
        for (key, time) in &ledger.positions {
            self.entry(key, time)?;
            self.text(&key.0)?;
        }
        for (key, (capture, deadline)) in &ledger.leases {
            self.entry(key, &(capture, deadline))?;
            self.add(3 * (size_of_val(capture) + size_of_val(deadline)))?;
            self.text(&key.0)?;
            self.text(&key.1)?;
            self.text(&capture.producer_id)?;
        }
        Ok(())
    }
}
impl Phase {
    pub(super) fn check_native_retained_bytes<'a>(
        &'a self,
        ledger: &Ledger,
        extra: impl IntoIterator<Item = &'a NativeProof>,
    ) -> Result<(), String> {
        let mut bytes = Bytes::default();
        bytes.ledger(ledger)?;
        for proof in self.retained_native_proofs().chain(extra) {
            bytes.proof(proof)?;
        }
        Ok(())
    }
    pub(super) fn native_memory_fault(&mut self, error: String) {
        self.effects.clear();
        self.reports.clear();
        self.timers.clear();
        for state in self.states.values_mut() {
            state.bindings.clear();
            state.pending.clear();
            state.inflight = None;
            state.rule_proof = NativeProof::default();
        }
        for value in self.values.values_mut() {
            value.proof = NativeProof::default();
        }
        self.causal = NativeProof::default();
        self.fault = Some(error);
        self.stop();
    }
}
