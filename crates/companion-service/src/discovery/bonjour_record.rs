use super::{pairing_hint, ADVERTISED_HOST, HOST_ID};
use serde_json::Value;
use std::net::IpAddr;

pub(super) fn hint(name: &str, hostname: &str, network_port: u16, mut txt: &[u8]) -> Option<Value> {
    let mut host = None;
    let mut host_id = None;
    while let Some((&length, rest)) = txt.split_first() {
        let (entry, remaining) = rest.split_at_checked(usize::from(length))?;
        txt = remaining;
        let Some(equals) = entry.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let (key, value) = (&entry[..equals], &entry[equals + 1..]);
        if key.eq_ignore_ascii_case(ADVERTISED_HOST.as_bytes()) && host.is_none() {
            host = Some(std::str::from_utf8(value).ok()?);
        } else if key.eq_ignore_ascii_case(HOST_ID.as_bytes()) && host_id.is_none() {
            host_id = std::str::from_utf8(value).ok();
        }
    }
    // SRV's target is diagnostic. The configured TXT host binds short-code pairing.
    let addresses = host
        .and_then(|host| host.parse::<IpAddr>().ok())
        .map(|ip| ip.to_string())
        .into_iter()
        .collect();
    pairing_hint(
        name,
        hostname,
        u16::from_be(network_port),
        host,
        host_id,
        addresses,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn txt(fields: &[&[u8]]) -> Vec<u8> {
        fields
            .iter()
            .flat_map(|field| {
                std::iter::once(u8::try_from(field.len()).unwrap()).chain(field.iter().copied())
            })
            .collect()
    }

    #[test]
    fn bonjour_resolve_keeps_the_listener_host_and_network_order_port() {
        let fields = txt(&[
            b"HOSTID=fixture",
            b"H=Cockpit.example",
            b"h=foreign.example",
            b"ignored",
        ]);
        let result = hint(
            "Fixture._osg-companion._tcp.local.",
            "osg-fixture.local.",
            54437u16.to_be(),
            &fields,
        )
        .unwrap();
        assert_eq!(result["endpoint"], "wss://Cockpit.example:54437/companion");
        assert_eq!(result["hostname"], "osg-fixture.local.");
        assert_eq!(result["hostId"], "fixture");
        assert_eq!(result["addresses"], json!([]));
        assert_eq!(result["trusted"], false);
        assert_eq!(result["requiresOutOfBandPairing"], true);

        let fields = txt(&[b"h=fd00:0::12"]);
        let result = hint("Fixture", "local.", 54437u16.to_be(), &fields).unwrap();
        assert_eq!(result["endpoint"], "wss://[fd00:0::12]:54437/companion");
        assert_eq!(result["addresses"], json!(["fd00::12"]));
    }

    #[test]
    fn bonjour_resolve_rejects_truncated_or_invalid_host_records() {
        assert!(hint("Fixture", "local.", 1u16.to_be(), &[5, b'h', b'=']).is_none());
        assert!(hint("Fixture", "local.", 1u16.to_be(), &txt(&[b"h=\xff"])).is_none());
        assert!(hint("Fixture", "local.", 1u16.to_be(), &txt(&[b"hostId=id"])).is_none());
        assert!(hint("Fixture", "local.", 0, &txt(&[b"h=host"])).is_none());
    }
}
