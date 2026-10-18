//! Shared ownership reducer; an OS key is released only after its final owner.
use crate::{
    keys::scan_code,
    protocol::{NativeEvent, MAX_BATCH, MAX_OWNERS},
};
use osg_mapping::{
    desktop::{Control, Delivery, Intent, MouseButton},
    engine::{typed::Token, RunToken},
};
use std::collections::BTreeMap;

#[derive(Clone, Default)]
pub struct Reducer {
    owners: BTreeMap<String, Owner>,
}
#[derive(Clone)]
struct Owner {
    token: Token,
    action: String,
    controls: BTreeMap<String, Control>,
    operation: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Reduction {
    pub events: Vec<NativeEvent>,
    pub skipped: bool,
    pub collision: bool,
}
fn owner_id(token: &Token, action: &str) -> String {
    serde_json::to_string(&(&token.run, &token.node_id, action)).expect("serializable owner")
}
fn control_id(control: &Control) -> Result<String, String> {
    Ok(match control {
        Control::Key { key } => {
            let scan = scan_code(key.0)?;
            format!("k{:02x}{}", scan.code, scan.extended as u8)
        }
        Control::MouseButton { button } => format!("m{:?}", button),
    })
}
fn rank(control: &Control) -> (u8, u16) {
    match control {
        Control::Key { key } => (if (0xe0..=0xe7).contains(&key.0) { 0 } else { 1 }, key.0),
        Control::MouseButton { button } => (
            2,
            match button {
                MouseButton::Left => 0,
                MouseButton::Right => 1,
                MouseButton::Middle => 2,
                MouseButton::X1 => 3,
                MouseButton::X2 => 4,
            },
        ),
    }
}
fn edge(control: &Control, down: bool) -> NativeEvent {
    match control {
        Control::Key { key } => NativeEvent::Key {
            usage: key.0,
            down,
            repeat: false,
        },
        Control::MouseButton { button } => NativeEvent::MouseButton {
            button: *button,
            down,
        },
    }
}
impl Reducer {
    pub fn held(&self) -> Vec<Control> {
        self.combined().into_values().collect()
    }
    pub fn collisions(&self) -> usize {
        let mut counts = BTreeMap::<String, usize>::new();
        for owner in self.owners.values() {
            for key in owner.controls.keys() {
                *counts.entry(key.clone()).or_default() += 1;
            }
        }
        counts.values().filter(|count| **count > 1).count()
    }
    fn combined(&self) -> BTreeMap<String, Control> {
        self.owners
            .values()
            .flat_map(|o| o.controls.clone())
            .collect()
    }
    fn changes(&self, before: BTreeMap<String, Control>) -> Vec<NativeEvent> {
        let after = self.combined();
        let mut releases: Vec<_> = before
            .iter()
            .filter(|(id, _)| !after.contains_key(*id))
            .map(|(_, c)| c)
            .collect();
        let mut presses: Vec<_> = after
            .iter()
            .filter(|(id, _)| !before.contains_key(*id))
            .map(|(_, c)| c)
            .collect();
        releases.sort_by_key(|c| std::cmp::Reverse(rank(c)));
        presses.sort_by_key(|c| rank(c));
        releases
            .into_iter()
            .map(|c| edge(c, false))
            .chain(presses.into_iter().map(|c| edge(c, true)))
            .collect()
    }
    /// Only the host's fresh released/neutral baseline may issue Arm.
    pub fn arm(&mut self, token: Token, action: String) -> Result<Vec<NativeEvent>, String> {
        if action.is_empty()
            || action.len() > 128
            || token.node_id.is_empty()
            || token.node_id.len() > 128
            || token.dependencies.len() > 64
        {
            return Err("Invalid desktop producer identity.".into());
        }
        let id = owner_id(&token, &action);
        if !self.owners.contains_key(&id) && self.owners.len() >= MAX_OWNERS {
            return Err("Desktop owner limit exceeded.".into());
        }
        let before = self.combined();
        self.owners.insert(
            id,
            Owner {
                token,
                action,
                controls: BTreeMap::new(),
                operation: 0,
            },
        );
        Ok(self.changes(before))
    }
    pub fn delivery(&mut self, delivery: &Delivery, expired: bool) -> Result<Reduction, String> {
        let mut next = self.clone();
        let reduction = next.apply_delivery(delivery, expired)?;
        *self = next;
        Ok(reduction)
    }
    fn apply_delivery(&mut self, delivery: &Delivery, expired: bool) -> Result<Reduction, String> {
        let id = owner_id(&delivery.producer, &delivery.action_id);
        if (matches!(delivery.intent, Intent::Cleanup)
            || matches!(&delivery.intent,Intent::Held{controls} if controls.is_empty()))
            && !self.owners.contains_key(&id)
        {
            return Ok(Reduction::default());
        }
        let owner = self
            .owners
            .get(&id)
            .ok_or("Desktop producer requires a fresh neutral baseline.")?;
        if owner.token != delivery.producer || delivery.operation_id.0 <= owner.operation {
            return Err("Stale desktop run, source generation or operation.".into());
        }
        let before = self.combined();
        let mut next = BTreeMap::new();
        let mut direct = Vec::new();
        let mut skipped = false;
        let mut collision = false;
        match &delivery.intent {
            Intent::Cleanup => {}
            Intent::Held { controls } => {
                if controls.len() > 16 {
                    return Err("Desktop chord exceeds 16 held controls.".into());
                }
                if expired && !controls.is_empty() {
                    skipped = true;
                }
                if !expired {
                    for c in controls {
                        next.insert(control_id(c)?, c.clone());
                    }
                }
            }
            Intent::Repeat { key } => {
                scan_code(key.0)?;
                next = owner.controls.clone();
                if expired {
                    skipped = true
                } else if before.contains_key(&control_id(&Control::Key { key: *key })?) {
                    direct.push(NativeEvent::Key {
                        usage: key.0,
                        down: true,
                        repeat: true,
                    });
                } else {
                    return Err("Repeat requires an owned key hold.".into());
                }
            }
            Intent::Pointer { dx, dy } => {
                next = owner.controls.clone();
                if expired {
                    skipped = true
                } else {
                    direct.push(NativeEvent::Pointer { dx: *dx, dy: *dy });
                }
            }
            Intent::Wheel {
                horizontal,
                vertical,
            } => {
                next = owner.controls.clone();
                if expired {
                    skipped = true
                } else {
                    direct.push(NativeEvent::Wheel {
                        horizontal: *horizontal,
                        vertical: *vertical,
                    });
                }
            }
        }
        if let Intent::Held { .. } = delivery.intent {
            collision = next.keys().any(|key| {
                self.owners
                    .iter()
                    .any(|(other, o)| other != &id && o.controls.contains_key(key))
            });
        }
        let target = self.owners.get_mut(&id).unwrap();
        target.controls = next;
        target.operation = delivery.operation_id.0;
        if self.combined().len() > MAX_BATCH {
            return Err("Desktop held-key ledger exceeds 64 controls.".into());
        }
        let mut events = self.changes(before);
        events.extend(direct);
        if events.len() > MAX_BATCH {
            return Err("Desktop batch exceeds its native limit.".into());
        }
        Ok(Reduction {
            events,
            skipped,
            collision,
        })
    }
    pub fn pause(&mut self, run: &RunToken) -> Vec<NativeEvent> {
        let before = self.combined();
        self.owners.retain(|_, o| &o.token.run != run);
        self.changes(before)
    }
    pub fn stop(&mut self) -> Vec<NativeEvent> {
        let before = self.combined();
        self.owners.clear();
        self.changes(before)
    }
    pub fn owner_count(&self) -> usize {
        self.owners.len()
    }
    pub fn owners(&self) -> Vec<(Token, String, Vec<Control>)> {
        self.owners
            .values()
            .map(|o| {
                (
                    o.token.clone(),
                    o.action.clone(),
                    o.controls.values().cloned().collect(),
                )
            })
            .collect()
    }
}

/// Construct releases for a bounded explicit interrupted-session recovery.
pub fn recovery_releases(controls: &[Control]) -> Result<Vec<NativeEvent>, String> {
    let mut values = BTreeMap::new();
    for c in controls {
        values.insert(control_id(c)?, c.clone());
    }
    if values.len() > MAX_BATCH {
        return Err("Recovery ledger exceeds bounded batch.".into());
    }
    let mut values: Vec<_> = values.into_values().collect();
    values.sort_by_key(|c| std::cmp::Reverse(rank(c)));
    Ok(values.iter().map(|c| edge(c, false)).collect())
}
