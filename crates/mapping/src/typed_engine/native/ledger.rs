use super::super::*;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::engine::typed) struct Owner {
    pub producer: String,
    pub generation: u64,
    pub domain: Option<(String, u64)>,
}
#[derive(Clone, Debug, Default)]
pub(in crate::engine::typed) struct Port {
    pub continuity: u64,
    pub active: bool,
    pub owner: Option<Owner>,
    pub proof: NativeProof,
    pub sample: Option<SignalSample>,
}
#[derive(Clone, Default)]
pub(in crate::engine::typed) struct Ledger {
    pub clock: Option<String>,
    pub registrations: BTreeMap<String, NativeSourceRegistration>,
    pub sources: BTreeMap<String, (u64, u64)>,
    pub ports: BTreeMap<Endpoint, Port>,
    pub inputs: BTreeMap<(String, u64, NativeInput), NativeCapture>,
    pub producers: BTreeMap<String, u64>,
    pub positions: BTreeMap<(String, u64, u64, Option<u32>), u64>,
    pub leases: BTreeMap<(String, String, u64), (NativeCapture, u64)>,
}
impl Phase {
    pub fn pin_native_clock(&mut self, clock: &str) -> Result<(), String> {
        identity(clock)?;
        if self.native.clock.as_deref().is_some_and(|old| old != clock) {
            return Err("Native admission clock is already pinned.".into());
        }
        self.native.clock = Some(clock.into());
        Ok(())
    }
    pub fn register_native_source(
        &mut self,
        registration: NativeSourceRegistration,
    ) -> Result<(), String> {
        identity(&registration.source)?;
        identity(&registration.clock_id)?;
        if self.native.clock.as_deref() != Some(&registration.clock_id) {
            return Err("Native registration requires the pinned acquisition clock.".into());
        }
        if let Some(old) = self.native.registrations.get(&registration.source) {
            return if old == &registration {
                Ok(())
            } else {
                Err("Native membership and policy are pinned for the run.".into())
            };
        }
        let expected = self
            .compiled
            .program
            .sources
            .iter()
            .filter(|port| port.endpoint.node_id == registration.source)
            .map(|port| port.endpoint.port_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for port in &registration.ports {
            identity(&port.port_id)?;
            identity(&port.family)?;
            if !expected.contains(port.port_id.as_str()) || !seen.insert(port.port_id.as_str()) {
                return Err("Native registration contains an unknown or duplicate port.".into());
            }
        }
        if expected.is_empty() || seen != expected || self.sources[&registration.source].available {
            return Err(
                "Native registration must pin complete membership before the first baseline."
                    .into(),
            );
        }
        for port in &registration.ports {
            self.native.ports.insert(
                Endpoint {
                    node_id: registration.source.clone(),
                    port_id: port.port_id.clone(),
                },
                Port::default(),
            );
        }
        self.native
            .registrations
            .insert(registration.source.clone(), registration);
        Ok(())
    }
}
pub(in crate::engine::typed) fn identity(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_NATIVE_ID_BYTES || value.chars().any(char::is_control)
    {
        Err("Invalid bounded native identity.".into())
    } else {
        Ok(())
    }
}
