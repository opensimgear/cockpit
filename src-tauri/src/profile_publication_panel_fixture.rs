use crate::{
    stm32_panel_capture::Hub,
    stm32_panel_wire::{Edge, Packet, Payload, Record},
};
use serde_json::Value;
use std::time::Instant;

#[derive(Default)]
pub(super) struct Publisher {
    route: Option<[u32; 6]>,
    sequence: u32,
    cursor: u32,
}

impl Publisher {
    pub(super) fn publish(
        &mut self,
        hub: &Hub,
        gateway: &Value,
        (pressed, cursor): (bool, u32),
        sampled_at_ms: u32,
        now: Instant,
    ) {
        let module = &gateway["modules"][0];
        let number = |key: &str| module[key].as_u64().unwrap() as u32;
        let route = [
            gateway["epoch"].as_u64().unwrap() as u32,
            number("node"),
            number("sourceNode"),
            number("boot"),
            number("lease"),
            number("configurationGeneration"),
        ];
        let [gateway_epoch, node, source_node, boot_session, lease, configuration_generation] =
            route;
        let module_id = module["id"].as_str().unwrap();
        let new_route = self.route != Some(route);
        if new_route {
            self.route = Some(route);
            self.sequence = 0;
        }
        let mut send = |payload| {
            self.sequence += 1;
            hub.accept(
                Record::Event {
                    module_id: module_id.into(),
                    source_node: source_node as u8,
                    packet: Packet {
                        node: node as u8,
                        boot_session,
                        lease,
                        gateway_epoch,
                        configuration_generation,
                        sequence: self.sequence,
                        cursor,
                        sampled_at_ms,
                        payload,
                    },
                },
                now,
            );
        };
        if new_route {
            let mut maintained = [0; 16];
            maintained[0] = u8::MAX;
            send(Payload::Layout {
                contact_count: 8,
                encoder_count: 0,
                selector_count: 0,
                maintained,
            });
        } else if cursor != self.cursor {
            assert_eq!(
                cursor,
                self.cursor + 1,
                "Fixture has one contact transition"
            );
            send(Payload::Edges(vec![Edge {
                control: 0,
                pressed,
                captured_at_ms: sampled_at_ms,
            }]));
        }
        let mut held = [0; 16];
        held[0] = u8::from(pressed);
        send(Payload::State {
            held,
            axes: [0; 8],
            axis_available: 0,
        });
        self.cursor = cursor;
    }
}
