use super::*;
use crate::module_usb::stream::{Batch, Snapshot};

#[derive(Clone, Copy)]
pub(super) struct Clock { pub started: Instant, pub origin: u64 }
impl Clock {
    pub(super) fn at(self, at: Instant) -> Result<u64, String> {
        if at >= self.started {
            self.origin.checked_add(u64::try_from(at.duration_since(self.started).as_micros())
                .map_err(|_| "USB input clock overflowed.")?)
        } else {
            self.origin.checked_sub(u64::try_from(self.started.duration_since(at).as_micros())
                .map_err(|_| "USB input clock underflowed.")?)
        }.ok_or_else(|| "USB input capture cannot be represented in the profile clock.".into())
    }
    pub(super) fn fresh(at: Instant, now: Instant) -> Result<(), String> {
        if at > now || now.duration_since(at) > Duration::from_millis(250) {
            Err("USB input evidence is stale; missing transitions were discarded.".into())
        } else { Ok(()) }
    }
}

impl Source {
    pub(super) fn usb_baseline(&mut self, snapshot: &Snapshot, clock: Clock, now: Instant) -> Result<Frame, String> {
        let Target::Usb { route, .. } = &self.spec.target else { unreachable!() };
        if snapshot.route != *route || snapshot.stream != route.configuration_generation || !snapshot.ready {
            return Err(snapshot.error.clone().unwrap_or_else(|| "USB input capture has no verified baseline.".into()));
        }
        let at = snapshot.received_at.ok_or("USB input baseline has no capture witness.")?;
        Clock::fresh(at, now)?;
        let events = snapshot.state.as_ref().ok_or("USB input baseline omitted metadata.")?;
        self.verify(events)?;
        let mut next = self.clone();
        next.sample_advanced(events)?;
        // The capture already proves this complete sample is fresh. Reusing
        // its original witness allows immediate axes at activation without
        // waiting for another source sample or creating event occurrences.
        let changes = next.state_changes(events, true, true)?;
        next.held = events["held"].as_array().ok_or("USB input held state disappeared.")?
            .iter().map(|value| value.as_bool().ok_or("Invalid USB held state.")).collect::<Result<_,_>>()?;
        next.cursor = Some((snapshot.stream, events["head"].as_u64().ok_or("Invalid USB input head.")? as u32));
        let frame = next.frame(clock.at(at)?, Reason::Baseline, changes)?;
        *self = next;
        Ok(frame)
    }
    pub(super) fn usb_batch(&mut self, batch: &Batch, clock: Clock, now: Instant) -> Result<Vec<Frame>, String> {
        let witnessed = batch.witnessed_at;
        Clock::fresh(witnessed, now)?;
        // Device milliseconds and native RX are separate clock domains. One
        // coherent batch uses conservative native first observation, never an
        // inferred physical capture time or a newer cached-axis timestamp.
        let batch_witness = batch.events.iter().map(|edge| edge.received_at).min()
            .map_or(witnessed, |first| first.min(witnessed));
        Clock::fresh(batch_witness, now)?;
        self.verify(&batch.latest.state)?;
        let (stream, cursor) = self.cursor.ok_or("USB input needs a new baseline.")?;
        if batch.latest.stream != stream { return Err("USB input scope changed.".into()); }
        let mut held = self.held.clone();
        let mut sequence = cursor;
        for edge in &batch.events {
            Clock::fresh(edge.received_at, now)?;
            if batch.latest.sampled_at_ms.wrapping_sub(edge.captured_at_ms) > 250 {
                return Err("USB input edge expired in the device clock; establish a fresh baseline.".into());
            }
            sequence = sequence.checked_add(1).ok_or("USB edge sequence exhausted.")?;
            if edge.sequence != sequence || usize::from(edge.control) >= held.len() {
                return Err("USB input edge continuity changed.".into());
            }
            held[usize::from(edge.control)] = edge.pressed;
        }
        if sequence != batch.latest.cursor || batch.latest.cursor != batch.latest.head
            || batch.latest.state["held"].as_array().is_none_or(|values| {
                values.len() != held.len() || values.iter().zip(&held).any(|(value, held)| value.as_bool() != Some(*held))
            })
        { return Err("USB input journal does not reconstruct current held state.".into()); }
        let mut next = self.clone();
        let mut frames = Vec::new();
        for edge in &batch.events {
            let changes = next.spec.ports.iter().filter(|port| {
                port.observed == format!("button:{}", edge.control)
                    || port.observed == format!("button-event:{}", edge.control)
            }).map(|port| Change { port_id: port.logical.clone(),
                sample: SignalSample::scalar(PortValue::Boolean(edge.pressed)) }).collect::<Vec<_>>();
            if !changes.is_empty() { frames.push(next.frame(clock.at(batch_witness)?, Reason::Change, changes)?); }
        }
        let advanced = next.sample_advanced(&batch.latest.state)?;
        let changes = next.state_changes(&batch.latest.state, false, advanced)?;
        if !changes.is_empty() { frames.push(next.frame(clock.at(batch_witness)?, Reason::Change, changes)?); }
        next.held = held;
        next.cursor = Some((stream, sequence));
        *self = next;
        Ok(frames)
    }
}
