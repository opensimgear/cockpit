use super::*;
impl Registry {
    pub fn connect(&mut self, mut device: Definition) -> Result<String, String> {
        if device.identity.vendor_id == 0x1234 && device.identity.product_id == 0xbead {
            return Err("vJoy virtual outputs are excluded from physical input.".into());
        }
        device.identity.fingerprint = fingerprint(&device.controls);
        let unique: BTreeSet<_> = device.controls.iter().map(|c| &c.id).collect();
        if !device.identity.valid()
            || device.controls.len() > MAX_CONTROLS
            || unique.len() != device.controls.len()
            || device.controls.iter().any(|c| {
                c.id.len() > 128
                    || (c.kind != Kind::Diagnostic && (c.bits == 0 || c.bits > 32 || c.max < c.min))
            })
        {
            return Err("Unsupported controller descriptor or inventory limit.".into());
        }
        let same_path = self
            .sources
            .iter()
            .filter(|(_, s)| s.saved.identity.same_interface(&device.identity))
            // Prefer the current capture over an obsolete remembered layout.
            .min_by_key(|(_, s)| !s.online)
            .map(|(id, _)| id.clone());
        let serials: Vec<_> = self
            .sources
            .iter()
            .filter(|(_, s)| s.saved.identity.serial_match(&device.identity))
            .map(|(id, s)| (id.clone(), s.online))
            .collect();
        // A live duplicate serial or multiple remembered candidates never merges automatically.
        let id = same_path.or_else(|| {
            if serials.len() == 1 && !serials[0].1 {
                Some(serials[0].0.clone())
            } else {
                None
            }
        });
        let id = if let Some(id) = id {
            id
        } else {
            if self.sources.len() >= MAX_SOURCES {
                return Err("Forget an offline controller before adding another.".into());
            }
            let id = uuid::Uuid::new_v4().to_string();
            let name = if valid_label(&device.name) {
                device.name
            } else {
                format!(
                    "Controller {:04X}:{:04X}",
                    device.identity.vendor_id, device.identity.product_id
                )
            };
            self.sources.insert(
                id.clone(),
                Source {
                    saved: SavedSource {
                        id: id.clone(),
                        identity: device.identity.clone(),
                        name,
                        role: "controller".into(),
                    },
                    online: false,
                    generation: 0,
                    sequence: 0,
                    captured_us: 0,
                    cursor: 0,
                    controls: vec![],
                    values: BTreeMap::new(),
                    diagnostics: vec![],
                },
            );
            id
        };
        let source = self.sources.get_mut(&id).unwrap();
        let layout_changed = source.saved.identity.fingerprint != device.identity.fingerprint;
        if !source.online || layout_changed {
            source.generation = source
                .generation
                .checked_add(2).map(|wire|wire - 1)
                .ok_or("Controller generation exhausted.")?;
            source.sequence = 0;
            source.values.clear();
            if source.online {
                self.journal.publish(source.event(Reason::Gap));
            }
        }
        if !source.online || layout_changed {
            source.captured_us = self.journal.now();
            source.cursor = self.journal.cursor();
        }
        source.online = true;
        source.saved.identity = device.identity;
        source.controls = device.controls;
        source.diagnostics = device
            .diagnostics
            .into_iter()
            .take(16)
            .map(|s| s.chars().take(256).collect())
            .collect();
        self.dirty = true;
        self.publish_observation(&id, "metadata", None, true);
        self.inventory_changed();
        Ok(id)
    }
    pub fn disconnect(&mut self, id: &str) {
        if let Some(s) = self.sources.get_mut(id).filter(|s| s.online) {
            s.online = false;
            s.values.clear();
            s.sequence = s.sequence.saturating_add(1);
            let event = self.journal.publish(s.event(Reason::Disconnect));
            self.publish_observation(id, "disconnect", event.as_ref(), false);
            self.inventory_changed();
        }
    }
    pub fn disconnect_all(&mut self) {
        let ids: Vec<_> = self.sources.keys().cloned().collect();
        for id in ids {
            self.disconnect(&id);
        }
    }
    pub fn fault(&mut self, id: &str, message: &str) {
        if let Some(s) = self.sources.get_mut(id) {
            let next = s.generation.checked_add(2).map(|wire|wire - 1);
            if let Some(generation) = next { s.generation = generation; }
            else { s.online = false; }
            s.sequence = 0;
            s.values.clear();
            s.diagnostics = vec![if next.is_some() {message.chars().take(256).collect()}
                else {"Controller generation exhausted; capture requires restart.".into()}];
            let event = self.journal.publish(s.event(if next.is_some() {Reason::Gap} else {Reason::Disconnect}));
            self.publish_observation(id, if next.is_some() {"gap"} else {"disconnect"}, event.as_ref(), false);
            self.inventory_changed();
        }
    }
    /// Commit a whole report under one registry lock, including implicit button releases.
    pub fn report(
        &mut self,
        id: &str,
        report: u8,
        data: &BTreeMap<u16, u32>,
    ) -> Result<(), String> {
        self.apply_values(id, report, data, true)
    }
    /// Incremental native values preserve untouched controls, including held buttons.
    pub fn delta(&mut self, id: &str, report: u8, data: &BTreeMap<u16, u32>) -> Result<(), String> {
        self.apply_values(id, report, data, false)
    }
    fn apply_values(
        &mut self,
        id: &str,
        report: u8,
        data: &BTreeMap<u16, u32>,
        implicit_buttons: bool,
    ) -> Result<(), String> {
        let s = self.sources.get_mut(id).ok_or("Unknown controller.")?;
        if !s.online {
            return Err("Controller is offline.".into());
        }
        let was_ready = s.ready();
        let mut update = BTreeMap::new();
        for control in s.controls.iter().filter(|c| c.report_id == report) {
            let value = data
                .get(&control.data_index)
                .copied()
                .or_else(|| (implicit_buttons && control.kind == Kind::Button).then_some(0));
            if let Some(value) = value {
                update.insert(control.id.clone(), control.decode(value)?);
            }
        }
        let sequence = s
            .sequence
            .checked_add(1)
            .ok_or("Controller sequence exhausted.")?;
        s.values.extend(update);
        s.sequence = sequence;
        let reason = if was_ready {
            Reason::Change
        } else {
            Reason::Baseline
        };
        let ready = s.ready();
        let event = if ready {
            self.journal.publish(s.event(reason.clone()))
        } else {
            None
        };
        self.publish_observation(
            id,
            if reason == Reason::Baseline {
                "baseline"
            } else {
                "change"
            },
            event.as_ref(),
            false,
        );
        if !was_ready && ready { self.inventory_readiness(id); }
        Ok(())
    }
}
