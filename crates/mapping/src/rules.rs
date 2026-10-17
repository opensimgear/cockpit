//! Bounded built-ins. Integer values, explicit time and no OS or UI dependencies.
use crate::profile::{AxisRange, Kind, Value};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Point {
    pub input: i32,
    pub output: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Step {
    pub after_us: u32,
    pub pressed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gesture {
    Tap,
    Hold,
    DoubleTap,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Rule {
    Axis {
        dead_zone: i32,
        invert: bool,
        scale: i32,
        curve: Vec<Point>,
    },
    Merge,
    Split {
        positive: bool,
    },
    ButtonAxis {
        range: AxisRange,
        pressed: i32,
        released: i32,
    },
    Threshold {
        press: i32,
        release: i32,
    },
    Chord,
    Gate,
    Toggle,
    Gesture {
        gesture: Gesture,
        hold_us: u32,
        double_us: u32,
        pulse_us: u32,
    },
    Pulse {
        duration_us: u32,
        repeat_us: u32,
    },
    Macro {
        steps: Vec<Step>,
    },
}
fn time(value: u32) -> bool {
    (1_000..=60_000_000).contains(&value)
}
impl Rule {
    pub fn validate(&self, inputs: &[Kind]) -> Result<Kind, String> {
        let fail = || Err("Invalid transformation input types, bounds or timing.".into());
        match self {
            Self::Axis {
                dead_zone,
                invert: _,
                scale,
                curve,
            } => {
                let [Kind::Axis { range }] = inputs else {
                    return fail();
                };
                if !(0..32767).contains(dead_zone)
                    || !(0..=4000).contains(scale)
                    || curve.len() > 32
                {
                    return fail();
                }
                if !curve.is_empty()
                    && (curve.len() < 2
                        || curve.first().unwrap().input != (*range).min()
                        || curve.last().unwrap().input != 32767
                        || curve
                            .iter()
                            .any(|p| !range.contains(p.input) || !range.contains(p.output))
                        || curve.windows(2).any(|w| w[0].input >= w[1].input))
                {
                    return fail();
                }
                Ok(inputs[0])
            }
            Self::Merge
                if inputs
                    == [Kind::Axis {
                        range: AxisRange::Unipolar,
                    }; 2] =>
            {
                Ok(Kind::Axis {
                    range: AxisRange::Bipolar,
                })
            }
            Self::Split { .. }
                if inputs
                    == [Kind::Axis {
                        range: AxisRange::Bipolar,
                    }] =>
            {
                Ok(Kind::Axis {
                    range: AxisRange::Unipolar,
                })
            }
            Self::ButtonAxis {
                range,
                pressed,
                released,
            } if inputs == [Kind::Button]
                && range.contains(*pressed)
                && range.contains(*released) =>
            {
                Ok(Kind::Axis { range: *range })
            }
            Self::Threshold { press, release } => {
                let [Kind::Axis { range }] = inputs else {
                    return fail();
                };
                if release >= press || !range.contains(*press) || !range.contains(*release) {
                    return fail();
                }
                Ok(Kind::Button)
            }
            Self::Chord
                if (2..=8).contains(&inputs.len()) && inputs.iter().all(|k| *k == Kind::Button) =>
            {
                Ok(Kind::Button)
            }
            Self::Gate if inputs.len() == 2 && inputs[1] == Kind::Button => Ok(inputs[0]),
            Self::Toggle if inputs == [Kind::Button] => Ok(Kind::Button),
            Self::Gesture {
                hold_us,
                double_us,
                pulse_us,
                ..
            } if inputs == [Kind::Button]
                && time(*hold_us)
                && time(*double_us)
                && time(*pulse_us) =>
            {
                Ok(Kind::Button)
            }
            Self::Pulse {
                duration_us,
                repeat_us,
            } if inputs == [Kind::Button]
                && time(*duration_us)
                && (*repeat_us == 0 || (time(*repeat_us) && repeat_us > duration_us)) =>
            {
                Ok(Kind::Button)
            }
            Self::Macro { steps }
                if inputs == [Kind::Button]
                    && !steps.is_empty()
                    && steps.len() <= 16
                    && steps.first().unwrap().after_us == 0
                    && !steps.last().unwrap().pressed
                    && steps.last().unwrap().after_us <= 60_000_000
                    && steps.windows(2).all(|w| w[0].after_us < w[1].after_us) =>
            {
                Ok(Kind::Button)
            }
            _ => fail(),
        }
    }
}
#[derive(Default)]
pub struct State {
    armed: bool,
    previous: bool,
    value: bool,
    pressed_at: Option<u64>,
    tap_until: Option<u64>,
    pulse_until: Option<u64>,
    repeat_at: Option<u64>,
    sequence: Vec<(u64, bool)>,
}
impl State {
    pub fn next(&self) -> Option<u64> {
        [
            self.tap_until,
            self.pulse_until,
            self.repeat_at,
            self.sequence.first().map(|s| s.0),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    pub fn apply(&mut self, rule: &Rule, inputs: &[Value], kind: Kind, now: u64) -> Option<Value> {
        let button = |i: usize| matches!(inputs[i], Value::Button(true));
        let axis = |i: usize| if let Value::Axis(v) = inputs[i] { v } else { 0 };
        match rule {
            Rule::Axis {
                dead_zone,
                invert,
                scale,
                curve,
            } => {
                let Kind::Axis { range } = kind else {
                    return None;
                };
                let mut v = axis(0) as i64;
                let zone = *dead_zone as i64;
                if v.abs() <= zone {
                    v = 0;
                } else {
                    let max = if v < 0 { 32768 } else { 32767 };
                    v = v.signum() * (v.abs() - zone) * max / (max - zone);
                }
                if *invert {
                    v = if range == AxisRange::Unipolar {
                        32767 - v
                    } else {
                        -v
                    };
                }
                v = (v * (*scale as i64) / 1000).clamp(range.min() as i64, 32767);
                if !curve.is_empty() {
                    let pair = curve.windows(2).find(|w| v <= w[1].input as i64).unwrap();
                    let (a, b) = (&pair[0], &pair[1]);
                    v = a.output as i64
                        + (v - a.input as i64) * (b.output as i64 - a.output as i64)
                            / (b.input as i64 - a.input as i64);
                }
                return Some(Value::Axis(v as i32));
            }
            Rule::Merge => return Some(Value::Axis((axis(1) - axis(0)).clamp(-32768, 32767))),
            Rule::Split { positive } => {
                return Some(Value::Axis(if *positive {
                    axis(0).max(0)
                } else {
                    (-i64::from(axis(0))).clamp(0, 32767) as i32
                }))
            }
            Rule::Threshold { press, release } => {
                if axis(0) <= *release {
                    self.armed = true;
                    self.value = false;
                } else if axis(0) >= *press && self.armed {
                    self.value = true;
                }
                return Some(Value::Button(self.value));
            }
            Rule::Gate => return if button(1) { Some(inputs[0]) } else { None },
            _ => {}
        }
        let down = if matches!(rule, Rule::Chord) {
            inputs.iter().all(|v| *v == Value::Button(true))
        } else {
            button(0)
        };
        if !down {
            self.armed = true;
        }
        let rise = down && !self.previous && self.armed;
        let fall = !down && self.previous;
        self.previous = down;
        if self.pulse_until.is_some_and(|at| at <= now) {
            self.value = false;
            self.pulse_until = None;
        }
        match rule {
            Rule::Chord => self.value = down && self.armed,
            Rule::ButtonAxis {
                pressed, released, ..
            } => {
                return Some(Value::Axis(if down && self.armed {
                    *pressed
                } else {
                    *released
                }))
            }
            Rule::Toggle => {
                if rise {
                    self.value = !self.value;
                }
            }
            Rule::Pulse {
                duration_us,
                repeat_us,
            } => {
                if !down {
                    self.repeat_at = None;
                }
                if rise || (down && self.repeat_at.is_some_and(|at| at <= now)) {
                    self.value = true;
                    self.pulse_until = Some(now.saturating_add(*duration_us as u64));
                    self.repeat_at = if *repeat_us == 0 {
                        None
                    } else {
                        Some(now.saturating_add(*repeat_us as u64))
                    };
                }
            }
            Rule::Macro { steps } => {
                if rise {
                    self.sequence = steps
                        .iter()
                        .map(|s| (now.saturating_add(s.after_us as u64), s.pressed))
                        .collect();
                }
                while self.sequence.first().is_some_and(|s| s.0 <= now) {
                    self.value = self.sequence.remove(0).1;
                }
            }
            Rule::Gesture {
                gesture,
                hold_us,
                double_us,
                pulse_us,
            } => {
                if self.tap_until.is_some_and(|at| at <= now) {
                    self.tap_until = None;
                    if matches!(gesture, Gesture::Tap) {
                        self.value = true;
                        self.pulse_until = Some(now.saturating_add(*pulse_us as u64));
                    }
                }
                if rise {
                    self.pressed_at = Some(now);
                    if matches!(gesture, Gesture::Hold) {
                        self.repeat_at = Some(now.saturating_add(*hold_us as u64));
                    }
                }
                if matches!(gesture, Gesture::Hold)
                    && down
                    && self.repeat_at.is_some_and(|at| at <= now)
                {
                    self.value = true;
                    self.repeat_at = None;
                }
                if fall {
                    self.repeat_at = None;
                    if matches!(gesture, Gesture::Hold) {
                        self.value = false;
                    }
                    if let Some(at) = self.pressed_at.take() {
                        if now.saturating_sub(at) < *hold_us as u64
                            && !matches!(gesture, Gesture::Hold)
                        {
                            if self.tap_until.is_some() {
                                self.tap_until = None;
                                if matches!(gesture, Gesture::DoubleTap) {
                                    self.value = true;
                                    self.pulse_until = Some(now.saturating_add(*pulse_us as u64));
                                }
                            } else {
                                self.tap_until = Some(now.saturating_add(*double_us as u64));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        Some(Value::Button(self.value))
    }
}
#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
