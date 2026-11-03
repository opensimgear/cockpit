use super::*;

impl Source {
    pub(super) fn frame(&mut self, at: u64, reason: Reason, changes: Vec<Change>) -> Result<Frame, String> {
        if at < self.at {
            return Err("Native panel clock moved backwards.".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Panel admission sequence exhausted.")?;
        self.at = at;
        Ok(Frame {
            source: self.spec.logical.clone(),
            generation: Counter(self.generation),
            sequence: Counter(self.sequence),
            captured_us: Counter(at),
            reason,
            changes,
        })
    }
    pub(super) fn verify(&self, events: &Value) -> Result<(), String> {
        let descriptors = crate::profile_resource_ports::events(events)?;
        for port in &self.spec.ports {
            if descriptors
                .iter()
                .find(|p| p.id == port.observed)
                .is_none_or(|p| p.contract != port.contract)
            {
                return Err(
                    "The selected panel input contract changed; reconcile its exact port.".into(),
                );
            }
        }
        Ok(())
    }
    pub(super) fn state_changes(
        &self,
        events: &Value,
        baseline: bool,
        sample_advanced: bool,
    ) -> Result<Vec<Change>, String> {
        self.spec
            .ports
            .iter()
            .filter_map(|port| {
                let sample = if let Some(index) = port.observed.strip_prefix("button:") {
                    if !baseline && !sample_advanced && events.get("sampledAtMs").is_some() {
                        return None;
                    }
                    let index = index
                        .parse::<usize>()
                        .map_err(|_| "Invalid selected button index.".to_owned());
                    Some(index.and_then(|index| {
                        events["held"][index]
                            .as_bool()
                            .map(|v| SignalSample::scalar(PortValue::Boolean(v)))
                            .ok_or_else(|| "The selected button disappeared.".into())
                    }))
                } else if let Some(index) = port.observed.strip_prefix("axis:") {
                    if !baseline
                        && !sample_advanced
                        && events["axisAvailable"][index.parse::<usize>().unwrap_or(usize::MAX)]
                            != false
                    {
                        return None;
                    }
                    Some(
                        index
                            .parse::<usize>()
                            .map_err(|_| "Invalid selected axis index.".into())
                            .and_then(|index| {
                                if events["axisAvailable"][index] == false {
                                    Ok(SignalSample::Unavailable)
                                } else if !sample_advanced {
                                    Ok(SignalSample::Unknown)
                                } else {
                                    events["axes"][index]
                                        .as_u64()
                                        .map(|v| SignalSample::scalar(PortValue::Number(v as f64)))
                                        .ok_or_else(|| "The selected raw axis disappeared.".into())
                                }
                            }),
                    )
                } else if port.observed.starts_with("button-event:") {
                    baseline.then_some(Ok(SignalSample::Unknown))
                } else {
                    Some(Err("The selected input has no native panel decoder.".into()))
                };
                sample.map(|sample| {
                    sample.map(|sample| Change {
                        port_id: port.logical.clone(),
                        sample,
                    })
                })
            })
            .collect()
    }
    pub(super) fn sample_advanced(&mut self, events: &Value) -> Result<bool, String> {
        let Some(current) = events["sampledAtMs"].as_u64() else {
            return Ok(false);
        };
        let current = current as u32;
        let previous = self.sampled_at;
        match previous {
            None => {
                self.sampled_at = Some(current);
                Ok(false)
            }
            Some(previous) => {
                let delta = current.wrapping_sub(previous);
                if delta > i32::MAX as u32 {
                    Err("Native panel sample clock moved backwards.".into())
                } else {
                    self.sampled_at = Some(current);
                    Ok(delta > 0)
                }
            }
        }
    }
    pub(super) fn baseline(&mut self, io: &dyn Io, at: u64) -> Result<Frame, String> {
        let events = self.spec.target.events(io, None)?;
        self.verify(&events)?;
        let advanced = self.sample_advanced(&events)?;
        let changes = self.state_changes(&events, true, advanced)?;
        self.held = events["held"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_bool().unwrap())
            .collect();
        self.cursor = Some((
            events["stream"].as_u64().unwrap() as u32,
            events["head"].as_u64().unwrap() as u32,
        ));
        self.frame(at, Reason::Baseline, changes)
    }
    pub(super) fn page(&mut self, io: &dyn Io, at: u64) -> Result<Vec<Frame>, String> {
        let cursor = self
            .cursor
            .ok_or("Establish a native baseline before reading panel events.")?;
        let events = self.spec.target.events(io, Some(cursor))?;
        self.verify(&events)?;
        let entries = events["events"].as_array().unwrap();
        let mut next_held = self.held.clone();
        for event in entries {
            let index = event["control"].as_u64().unwrap() as usize;
            if index >= next_held.len() {
                return Err(crate::panel_events::INVALID.into());
            }
            next_held[index] = event["pressed"].as_bool().unwrap();
        }
        if events["more"] == false
            && next_held.iter().map(|v| json!(v)).collect::<Vec<_>>()
                != *events["held"].as_array().unwrap()
        {
            return Err("The panel journal does not reconstruct its reported state; establish a new baseline.".into());
        }
        let mut frames = vec![];
        for event in entries {
            let index = event["control"].as_u64().unwrap();
            let pressed = event["pressed"].as_bool().unwrap();
            let changes = self
                .spec
                .ports
                .iter()
                .filter(|port| {
                    port.observed == format!("button:{index}")
                        || port.observed == format!("button-event:{index}")
                })
                .map(|port| Change {
                    port_id: port.logical.clone(),
                    sample: SignalSample::scalar(PortValue::Boolean(pressed)),
                })
                .collect::<Vec<_>>();
            if !changes.is_empty() {
                frames.push(self.frame(at, Reason::Change, changes)?);
            }
        }
        self.held = next_held;
        self.cursor = Some((cursor.0, events["next"].as_u64().unwrap() as u32));
        if events["more"] == false {
            // State is sampled at head; never move axes ahead of queued edges.
            let advanced = self.sample_advanced(&events)?;
            let changes = self.state_changes(&events, false, advanced)?;
            if !changes.is_empty() {
                frames.push(self.frame(at, Reason::Change, changes)?);
            }
        }
        Ok(frames)
    }
}
