use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Record {
    Reset,
    Loss {
        module_id: String,
    },
    Event {
        module_id: String,
        source_node: u8,
        packet: Packet,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Packet {
    pub node: u8,
    pub boot_session: u32,
    pub lease: u32,
    pub gateway_epoch: u32,
    pub configuration_generation: u32,
    pub sequence: u32,
    pub cursor: u32,
    pub sampled_at_ms: u32,
    pub payload: Payload,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Payload {
    Layout {
        contact_count: u8,
        encoder_count: u8,
        selector_count: u8,
        maintained: [u8; 16],
    },
    State {
        held: [u8; 16],
        axes: [u16; 8],
        axis_available: u8,
    },
    Edges(Vec<Edge>),
    Gap,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub control: u8,
    pub pressed: bool,
    pub captured_at_ms: u32,
}

pub(crate) fn decode(value: &Value) -> Option<Record> {
    let object = value.as_object()?;
    match object.get("panelStream")?.as_str()? {
        "reset" if object.len() == 1 => Some(Record::Reset),
        "loss" if object.len() == 2 => {
            let module_id = valid_module_id(object.get("moduleId")?.as_str()?)?;
            Some(Record::Loss { module_id })
        }
        "event" if object.len() == 4 => {
            let module_id = valid_module_id(object.get("moduleId")?.as_str()?)?;
            let source_node = u8::try_from(object.get("sourceNode")?.as_u64()?).ok()?;
            if source_node > 127 {
                return None;
            }
            let bytes = decode_hex(object.get("payload")?.as_str()?)?;
            let packet = decode_packet(&bytes)?;
            Some(Record::Event {
                module_id,
                source_node,
                packet,
            })
        }
        _ => None,
    }
}

fn valid_module_id(value: &str) -> Option<String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || value.len() % 2 != 0 || value.len() > 126 {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn decode_packet(bytes: &[u8]) -> Option<Packet> {
    if bytes.len() < 31 || bytes.len() > 63 {
        return None;
    }
    let node = bytes[0];
    let boot_session = read_u32(bytes, 1)?;
    let lease = read_u32(bytes, 5)?;
    let gateway_epoch = read_u32(bytes, 9)?;
    let configuration_generation = read_u32(bytes, 13)?;
    let sequence = read_u32(bytes, 17)?;
    let cursor = read_u32(bytes, 21)?;
    let sampled_at_ms = read_u32(bytes, 25)?;
    if !(1..=32).contains(&node)
        || [
            boot_session,
            lease,
            gateway_epoch,
            configuration_generation,
            sequence,
        ]
        .contains(&0)
    {
        return None;
    }
    let variant = bytes[29];
    let data = &bytes[30..];
    let payload = match variant {
        0 if data.len() == 19 => Payload::Layout {
            contact_count: data[0],
            encoder_count: data[1],
            selector_count: data[2],
            maintained: data[3..19].try_into().ok()?,
        },
        1 if data.len() == 33 => {
            let held = data[..16].try_into().ok()?;
            let mut axes = [0; 8];
            for (index, axis) in axes.iter_mut().enumerate() {
                *axis = u16::from_le_bytes([data[16 + index * 2], data[17 + index * 2]]);
            }
            Payload::State {
                held,
                axes,
                axis_available: data[32],
            }
        }
        2 if !data.is_empty()
            && (1..=6).contains(&data[0])
            && data.len() == 1 + 5 * data[0] as usize =>
        {
            let mut edges = Vec::with_capacity(data[0] as usize);
            for encoded in data[1..].chunks_exact(5) {
                edges.push(Edge {
                    control: encoded[0] & 0x7f,
                    pressed: encoded[0] & 0x80 != 0,
                    captured_at_ms: u32::from_le_bytes(encoded[1..5].try_into().ok()?),
                });
            }
            Payload::Edges(edges)
        }
        3 if data == [1] || data == [2] => Payload::Gap,
        _ => return None,
    };
    Some(Packet {
        node,
        boot_session,
        lease,
        gateway_epoch,
        configuration_generation,
        sequence,
        cursor,
        sampled_at_ms,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn envelope(payload: &[u8]) -> Value {
        let payload = payload
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        json!({
            "panelStream": "event",
            "moduleId": "0123456789ABCDEF0123456789ABCDEF",
            "sourceNode": 127,
            "payload": payload
        })
    }

    fn prefix(variant: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![7];
        for field in [4u32, 5, 6, 8, 9, 10, 11] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes.push(variant);
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn decodes_all_panel_payloads() {
        assert_eq!(decode(&json!({"panelStream":"reset"})), Some(Record::Reset));
        assert_eq!(
            decode(&json!({"panelStream":"loss","moduleId":"0123456789abcdef0123456789abcdef"})),
            Some(Record::Loss {
                module_id: "0123456789abcdef0123456789abcdef".into()
            })
        );

        let mut layout = vec![2, 1, 1];
        layout.extend_from_slice(&[1; 16]);
        let decoded = decode(&envelope(&prefix(0, &layout))).unwrap();
        assert!(matches!(
            decoded,
            Record::Event {
                source_node: 127,
                packet: Packet {
                    node: 7,
                    boot_session: 4,
                    lease: 5,
                    gateway_epoch: 6,
                    configuration_generation: 8,
                    sequence: 9,
                    cursor: 10,
                    sampled_at_ms: 11,
                    payload: Payload::Layout {
                        contact_count: 2,
                        encoder_count: 1,
                        selector_count: 1,
                        ..
                    },
                },
                ..
            }
        ));

        let mut state = vec![0x55; 16];
        for axis in 0..8u16 {
            state.extend_from_slice(&(1000 + axis).to_le_bytes());
        }
        state.push(0x81);
        let decoded = decode(&envelope(&prefix(1, &state))).unwrap();
        assert!(matches!(decoded, Record::Event { packet: Packet {
            payload: Payload::State { axis_available: 0x81, axes, .. }, ..
        }, .. } if axes == [1000, 1001, 1002, 1003, 1004, 1005, 1006, 1007]));

        let edges = vec![2, 0x80, 0xfe, 0xff, 0xff, 0xff, 3, 4, 3, 2, 1];
        let decoded = decode(&envelope(&prefix(2, &edges))).unwrap();
        assert!(matches!(decoded, Record::Event { packet: Packet {
            payload: Payload::Edges(edges), ..
        }, .. } if edges == vec![
            Edge { control: 0, pressed: true, captured_at_ms: u32::MAX - 1 },
            Edge { control: 3, pressed: false, captured_at_ms: 0x01020304 },
        ]));

        assert!(matches!(
            decode(&envelope(&prefix(3, &[2]))),
            Some(Record::Event {
                packet: Packet {
                    payload: Payload::Gap,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn rejects_malformed_envelopes_and_payload_lengths() {
        let mut value = envelope(&prefix(3, &[1]));
        assert!(decode(&value).is_some());
        value["extra"] = json!(true);
        assert!(decode(&value).is_none());
        assert!(decode(&json!({"panelStream":"reset","moduleId":"x"})).is_none());
        assert!(decode(&json!({"panelStream":"loss","moduleId":"xyz"})).is_none());

        for payload in [
            prefix(0, &[0; 18]),
            prefix(1, &[0; 32]),
            prefix(2, &[0]),
            prefix(2, &[7]),
            prefix(3, &[0]),
            prefix(9, &[0]),
        ] {
            assert!(decode(&envelope(&payload)).is_none());
        }
        assert!(decode(&json!({
            "panelStream":"event",
            "moduleId":"0123456789abcdef0123456789abcdef",
            "sourceNode":128,
            "payload": prefix(3, &[1]).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        }))
        .is_none());
        let mut invalid_hex = envelope(&prefix(3, &[1]));
        invalid_hex["payload"] = json!("0z");
        assert!(decode(&invalid_hex).is_none());
    }
}
