use serde_json::{json, Map, Value};
pub(super) fn report_name(id: u8) -> Option<&'static str> {
    Some(match id {
        0x01 => "setEffect",
        0x02 => "setEnvelope",
        0x03 => "setCondition",
        0x04 => "setPeriodic",
        0x05 => "setConstant",
        0x06 => "setRamp",
        0x07 => "setCustomForce",
        0x08 => "downloadForceSample",
        0x0a => "effectOperation",
        0x0b => "blockFree",
        0x0c => "deviceControl",
        0x0d => "deviceGain",
        0x0e => "setCustomForce",
        0x11 => "createNewEffect",
        0x12 => "blockLoad",
        0x13 => "pool",
        0x14 => "pidState",
        _ => return None,
    })
}

pub(super) fn decode_fields(id: u8, b: &[u8]) -> Option<Map<String, Value>> {
    let mut f = Map::new();
    let block = || b.get(1).copied().map(Value::from);
    let read_u16 =
        |at: usize| -> Option<u16> { Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])) };
    let read_i16 =
        |at: usize| -> Option<i16> { Some(i16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])) };
    let put_block = |f: &mut Map<String, Value>| {
        f.insert("effectBlockIndex".into(), block()?);
        Some(())
    };
    match id {
        0x01 => {
            need(b, 18)?;
            put_block(&mut f)?;
            let et = b[2];
            let duration = read_u16(3)?;
            f.insert("effectType".into(), json!(effect_type(et)));
            f.insert(
                "durationMs".into(),
                json!(if duration == u16::MAX {
                    "infinite".to_owned()
                } else {
                    duration.to_string()
                }),
            );
            f.insert("triggerRepeatMs".into(), word_or_infinite(read_u16(5)?));
            f.insert("samplePeriodMs".into(), word_or_infinite(read_u16(7)?));
            f.insert("startDelayMs".into(), json!(read_u16(9)?));
            f.insert("gainRaw".into(), json!(b[11]));
            f.insert("triggerButton".into(), json!(b[12]));
            let axes = b[13];
            f.insert("axesEnable".into(), json!(axes));
            if axes == 4 {
                f.insert(
                    "direction".into(),
                    json!({"mode":"polar","raw":read_u16(14)?}),
                );
            } else {
                need(b, 20)?;
                f.insert(
                    "direction".into(),
                    json!({"mode":"cartesian","x":read_i16(14)?,"y":read_i16(16)?}),
                );
            }
            if b.len() >= 22 {
                f.insert(
                    "typeSpecificOffsets".into(),
                    json!([read_u16(18)?, read_u16(20)?]),
                );
            }
        }
        0x02 => {
            need(b, 14)?;
            put_block(&mut f)?;
            f.insert("attackLevel".into(), json!(read_u16(2)?));
            f.insert("fadeLevel".into(), json!(read_u16(4)?));
            f.insert(
                "attackTimeMs".into(),
                json!(u32::from_le_bytes([b[6], b[7], b[8], b[9]])),
            );
            f.insert(
                "fadeTimeMs".into(),
                json!(u32::from_le_bytes([b[10], b[11], b[12], b[13]])),
            );
        }
        0x03 => {
            need(b, 15)?;
            put_block(&mut f)?;
            f.insert("parameterBlockOffset".into(), json!(b[2] & 0x0f));
            f.insert(
                "typeSpecificOffsets".into(),
                json!([(b[2] >> 4) & 0x03, (b[2] >> 6) & 0x03]),
            );
            f.insert("isYRaw".into(), json!(b[2]));
            f.insert("isY".into(), json!(b[2] != 0));
            for (key, at) in [
                ("centerPointOffset", 3),
                ("positiveCoefficient", 5),
                ("negativeCoefficient", 7),
                ("deadBand", 13),
            ] {
                f.insert(key.into(), json!(read_i16(at)?));
            }
            f.insert("positiveSaturation".into(), json!(read_u16(9)?));
            f.insert("negativeSaturation".into(), json!(read_u16(11)?));
        }
        0x04 => {
            need(b, 12)?;
            put_block(&mut f)?;
            f.insert("magnitude".into(), json!(read_u16(2)?));
            f.insert("offset".into(), json!(read_i16(4)?));
            f.insert("phase".into(), json!(read_u16(6)?));
            f.insert(
                "periodMs".into(),
                json!(u32::from_le_bytes([b[8], b[9], b[10], b[11]])),
            );
        }
        0x05 => {
            need(b, 4)?;
            put_block(&mut f)?;
            f.insert("magnitude".into(), json!(read_i16(2)?));
        }
        0x06 => {
            need(b, 6)?;
            put_block(&mut f)?;
            f.insert("start".into(), json!(read_i16(2)?));
            f.insert("end".into(), json!(read_i16(4)?));
        }
        0x07 => {
            need(b, 16)?;
            put_block(&mut f)?;
            f.insert("offset".into(), json!(read_u16(2)?));
            f.insert(
                "samples".into(),
                json!(b[4..16]
                    .iter()
                    .map(|sample| *sample as i8)
                    .collect::<Vec<_>>()),
            );
        }
        0x08 => {
            need(b, 3)?;
            f.insert("x".into(), json!(b[1] as i8));
            f.insert("y".into(), json!(b[2] as i8));
        }
        0x0e => {
            need(b, 5)?;
            put_block(&mut f)?;
            f.insert("sampleCount".into(), json!(b[2]));
            f.insert("samplePeriodMs".into(), json!(read_u16(3)?));
        }
        0x0a => {
            need(b, 4)?;
            put_block(&mut f)?;
            f.insert(
                "operation".into(),
                json!(match b[2] {
                    1 => "start",
                    2 => "solo",
                    3 => "stop",
                    _ => "unknown",
                }),
            );
            f.insert("operationRaw".into(), json!(b[2]));
            f.insert("loopCount".into(), json!(b[3]));
        }
        0x0b => {
            need(b, 2)?;
            put_block(&mut f)?;
        }
        0x0c => {
            need(b, 2)?;
            f.insert(
                "control".into(),
                json!(match b[1] {
                    1 => "enableActuators",
                    2 => "disableActuators",
                    3 => "stopAll",
                    4 => "deviceReset",
                    5 => "pause",
                    6 => "continue",
                    _ => "unknown",
                }),
            );
            f.insert("controlRaw".into(), json!(b[1]));
        }
        0x0d => {
            need(b, 2)?;
            f.insert("gainRaw".into(), json!(b[1]));
            f.insert(
                "gainPercent".into(),
                json!((u16::from(b[1]) * 100 / 255) as u8),
            );
        }
        0x11 => {
            need(b, 4)?;
            f.insert("effectType".into(), json!(effect_type(b[1])));
            f.insert("effectTypeRaw".into(), json!(b[1]));
            f.insert("effectBlockIndex".into(), json!(b[2]));
            f.insert(
                "allocationStatus".into(),
                json!(if b[2] == 0 { "failed" } else { "assigned" }),
            );
            f.insert("byteCountAvailable".into(), json!(false));
        }
        0x12 => {
            need(b, 5)?;
            f.insert("effectBlockIndex".into(), json!(b[1]));
            f.insert("loadStatusRaw".into(), json!(b[2]));
            f.insert(
                "poolAvailable".into(),
                json!(u16::from_le_bytes([b[3], b[4]])),
            );
        }
        0x13 => {
            need(b, 5)?;
            f.insert("poolSize".into(), json!(u16::from_le_bytes([b[1], b[2]])));
            f.insert("simultaneousEffectsMax".into(), json!(b[3]));
            f.insert("deviceManagedPool".into(), json!(b[4] & 1 != 0));
            f.insert("sharedParameterBlocks".into(), json!(b[4] & 2 != 0));
            f.insert("paddingRaw".into(), json!(b[4] >> 2));
        }
        0x14 => {
            need(b, 3)?;
            f.insert("effectBlockIndex".into(), json!(b[1]));
            let bits = b[2];
            f.insert("stateRaw".into(), json!(bits));
            f.insert(
                "state".into(),
                json!({
                    "effectPlaying": bits & 0x01 != 0,
                    "paused": bits & 0x02 != 0,
                    "actuatorsEnabled": bits & 0x04 != 0,
                    "safetySwitch": bits & 0x08 != 0,
                    "actuatorOverride": bits & 0x10 != 0,
                    "actuatorPower": bits & 0x20 != 0,
                    "paddingRaw": bits >> 6,
                }),
            );
        }
        _ => return None,
    }
    Some(f)
}

fn need(bytes: &[u8], len: usize) -> Option<()> {
    (bytes.len() >= len).then_some(())
}
fn word_or_infinite(n: u16) -> Value {
    json!(if n == u16::MAX {
        "infinite".to_owned()
    } else {
        n.to_string()
    })
}
fn effect_type(n: u8) -> &'static str {
    match n {
        1 => "constant",
        2 => "ramp",
        3 => "square",
        4 => "sine",
        5 => "triangle",
        6 => "sawtoothUp",
        7 => "sawtoothDown",
        8 => "spring",
        9 => "damper",
        10 => "inertia",
        11 => "friction",
        12 => "custom",
        _ => "unknown",
    }
}
