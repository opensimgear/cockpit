use super::MAX_EFFECT_BLOCK_INDEX;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Default)]
struct Effect {
    effect_type: String,
    running: Option<bool>,
    loops: Option<u8>,
    fields: Value,
}

/// Accumulates decoded reports into a bounded per-slot lifecycle view.
#[derive(Debug, Default)]
pub(in crate::vjoy_debug) struct Effects {
    effects: BTreeMap<u8, Effect>,
    gain: Option<u8>,
    paused: Option<bool>,
    actuators_enabled: Option<bool>,
}

impl Effects {
    pub(in crate::vjoy_debug) fn observe(&mut self, event: &Value) {
        if event.get("status").and_then(Value::as_str) != Some("ok") {
            return;
        }
        let Some(report) = event
            .get("reportId")
            .and_then(Value::as_u64)
            .map(|n| n as u8)
        else {
            return;
        };
        let Some(fields) = event.get("fields").and_then(Value::as_object) else {
            return;
        };
        let block = fields
            .get("effectBlockIndex")
            .and_then(Value::as_u64)
            .map(|n| n as u8);
        match report {
            0x11 => {
                let Some(effect_type) = fields.get("effectType").and_then(Value::as_str) else {
                    return;
                };
                let Some(block) =
                    block.filter(|index| (1..=MAX_EFFECT_BLOCK_INDEX).contains(index))
                else {
                    return;
                };
                // The pinned driver's Ffb_ProcessPacket rewrites packet[2] with
                // the allocated EID (or zero on exhaustion) before it queues
                // the frame for the callback. The requested byte count is lost.
                let effect = self.effects.entry(block).or_default();
                effect.effect_type = effect_type.to_owned();
                effect.fields = json!({"created": true});
            }
            0x01 => {
                let Some(block) =
                    block.filter(|index| (1..=MAX_EFFECT_BLOCK_INDEX).contains(index))
                else {
                    return;
                };
                let effect = self.effects.entry(block).or_default();
                if let Some(t) = fields.get("effectType").and_then(Value::as_str) {
                    effect.effect_type = t.to_owned();
                }
                merge_fields(effect, fields);
            }
            0x02..=0x08 | 0x0e => {
                if let Some(block) =
                    block.filter(|index| (1..=MAX_EFFECT_BLOCK_INDEX).contains(index))
                {
                    if report == 0x03
                        && fields
                            .get("parameterBlockOffset")
                            .and_then(Value::as_u64)
                            .is_some_and(|n| n > 3)
                    {
                        return;
                    }
                    let effect = self.effects.entry(block).or_default();
                    merge_fields(effect, fields);
                }
            }
            0x0a => {
                let Some(block) =
                    block.filter(|index| (1..=MAX_EFFECT_BLOCK_INDEX).contains(index))
                else {
                    return;
                };
                let op = fields
                    .get("operation")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                match op {
                    "start" | "solo" => {
                        if op == "solo" {
                            for effect in self.effects.values_mut() {
                                effect.running = Some(false);
                            }
                        }
                        let effect = self.effects.entry(block).or_default();
                        effect.running = Some(true);
                        effect.loops = fields
                            .get("loopCount")
                            .and_then(Value::as_u64)
                            .map(|n| n as u8);
                    }
                    "stop" => {
                        if let Some(effect) = self.effects.get_mut(&block) {
                            effect.running = Some(false);
                        }
                    }
                    _ => {}
                }
            }
            0x0b => {
                if let Some(block) =
                    block.filter(|index| (1..=MAX_EFFECT_BLOCK_INDEX).contains(index))
                {
                    self.effects.remove(&block);
                }
            }
            0x0c => match fields.get("control").and_then(Value::as_str).unwrap_or("") {
                "enableActuators" => self.actuators_enabled = Some(true),
                "disableActuators" => self.actuators_enabled = Some(false),
                "stopAll" => {
                    for effect in self.effects.values_mut() {
                        effect.running = Some(false);
                    }
                }
                "deviceReset" => {
                    self.effects.clear();
                    self.paused = Some(false);
                    self.actuators_enabled = Some(true);
                }
                "pause" => self.paused = Some(true),
                "continue" => self.paused = Some(false),
                _ => {}
            },
            0x0d => {
                self.gain = fields
                    .get("gainRaw")
                    .and_then(Value::as_u64)
                    .map(|n| n as u8)
            }
            _ => {}
        }
    }

    pub(in crate::vjoy_debug) fn snapshot(&self) -> Value {
        let effects: Vec<Value> = self.effects.iter().map(|(block, effect)| json!({
            "blockIndex": block,
            "type": if effect.effect_type.is_empty() { "unknown" } else { &effect.effect_type },
            "running": effect.running,
            "loopCount": effect.loops,
            "fields": effect.fields,
        })).collect();
        json!({
            "effects": effects,
            "running": if self.effects.values().any(|effect| effect.running.is_some()) { Some(self.effects.values().filter(|effect| effect.running == Some(true)).count()) } else { None },
            "gain": self.gain,
            "paused": self.paused,
            "actuatorsEnabled": self.actuators_enabled,
        })
    }
}

fn merge_fields(effect: &mut Effect, fields: &Map<String, Value>) {
    if !effect.fields.is_object() {
        effect.fields = Value::Object(Map::new());
    }
    effect
        .fields
        .as_object_mut()
        .unwrap()
        .extend(fields.clone());
}
