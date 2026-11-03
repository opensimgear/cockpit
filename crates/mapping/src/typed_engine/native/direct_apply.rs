//! Actual Direct protocol application after the shared drain barrier.
use super::super::*;
impl Phase {
    pub fn accept(&mut self, frame: Frame, admitted_now: Counter) -> Result<(), String> {
        let implicit_loss = self.sources.get(&frame.source).is_some_and(|previous| {
            frame.reason == Reason::Change
                && (!previous.available
                    || previous.generation != frame.generation.0
                    || previous.sequence.checked_add(1) != Some(frame.sequence.0))
        });
        self.admit_batch([frame], admitted_now, false)?;
        if implicit_loss {
            return Err(
                "Typed source gap or generation change; acquire a complete fresh baseline.".into(),
            );
        }
        Ok(())
    }
    /// Evaluate a frame only after bounded batch validation and revocation.
    pub(in crate::engine::typed) fn accept_current(&mut self, frame: Frame) -> Result<(), String> {
        if !self.active {
            return Err("Typed mapping is stopped.".into());
        }
        let Some(previous) = self.sources.get(&frame.source) else {
            return Err("Unknown typed source resource.".into());
        };
        let baseline = frame.reason == Reason::Baseline;
        let valid = self.valid_frame(&frame, previous);
        if !valid {
            self.invalidate(&frame.source, SignalSample::Unknown);
            self.refresh_source(&frame.source, ReportReason::SourceLost)?;
            return Err("Invalid bounded typed source frame; acquire a complete baseline.".into());
        }
        let gap = previous.available
            && !baseline
            && (previous.generation != frame.generation.0
                || previous.sequence.checked_add(1) != Some(frame.sequence.0));
        if !baseline
            && (!previous.available
                || gap
                || matches!(frame.reason, Reason::Gap | Reason::Disconnect))
        {
            // Valid loss metadata still fences later baselines. Availability does
            // not erase the last observed generation, sequence or capture time.
            let source = self.sources.get_mut(&frame.source).unwrap();
            if frame.generation.0 >= source.generation {
                source.sequence = if frame.generation.0 == source.generation {
                    source.sequence.max(frame.sequence.0)
                } else {
                    frame.sequence.0
                };
                source.generation = frame.generation.0;
            }
            source.observed = frame.captured_us.0;
            self.invalidate(
                &frame.source,
                if frame.reason == Reason::Disconnect {
                    SignalSample::Unavailable
                } else {
                    SignalSample::Unknown
                },
            );
            self.refresh_source(&frame.source, ReportReason::SourceLost)?;
            return if matches!(frame.reason, Reason::Gap | Reason::Disconnect) {
                Ok(())
            } else {
                Err(
                    "Typed source gap or generation change; acquire a complete fresh baseline."
                        .into(),
                )
            };
        }
        if baseline
            && (frame.generation.0 < previous.generation
                || previous.generation == frame.generation.0
                    && frame.sequence.0 <= previous.sequence)
        {
            return Err("An old typed baseline cannot replay retained input.".into());
        }
        if baseline {
            self.invalidate(&frame.source, SignalSample::Unknown);
            self.refresh_source(&frame.source, ReportReason::SourceLost)?;
        }
        let state = self.sources.get_mut(&frame.source).unwrap();
        state.generation = frame.generation.0;
        state.sequence = frame.sequence.0;
        state.observed = frame.captured_us.0;
        state.available = true;
        let mut dirty = BTreeSet::new();
        let mut events = BTreeSet::new();
        for change in frame.changes {
            let endpoint = Endpoint {
                node_id: frame.source.clone(),
                port_id: change.port_id,
            };
            let event = matches!(
                self.compiled.contracts[&endpoint],
                PortContract::Event { .. }
            ) && matches!(change.sample, SignalSample::Known(_));
            let sample = if matches!(self.compiled.contracts[&endpoint],PortContract::State{freshness_ms,..} if frame.captured_us.0.saturating_add(freshness_ms as u64*1000)<=self.clock)
                && matches!(change.sample, SignalSample::Known(_))
            {
                SignalSample::Stale
            } else {
                change.sample
            };
            self.put(&endpoint, sample);
            self.values.get_mut(&endpoint).unwrap().observed = frame.captured_us.0;
            self.values.get_mut(&endpoint).unwrap().captured = frame.captured_us.0;
            self.values.get_mut(&endpoint).unwrap().captured_sequence = frame.sequence.0;
            dirty.insert(endpoint.clone());
            if event && !baseline {
                events.insert(endpoint);
            }
        }
        self.cancel_invalid();
        let events = if baseline {
            self.evaluate_baseline(dirty, events)?
        } else {
            self.evaluate(dirty, events, false)?
        };
        self.publish(
            events,
            if baseline {
                ReportReason::Baseline
            } else {
                ReportReason::Change
            },
            false,
        );
        Ok(())
    }
}
