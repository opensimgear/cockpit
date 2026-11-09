//! Untrusted LAN discovery for the native companion listener and client.
use mdns_sd::{ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use rustls_pki_types::ServerName;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    net::IpAddr,
    time::{Duration, Instant},
};

pub const SERVICE_TYPE: &str = "_osg-companion._tcp.local.";
// A single-byte key leaves room for a 253-byte DNS hostname in one TXT entry.
const ADVERTISED_HOST: &str = "h";

pub(crate) fn listener_endpoint(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("wss://[{host}]:{port}/companion")
    } else {
        format!("wss://{host}:{port}/companion")
    }
}

pub(crate) fn advertise(host_id: &str, host: &str, port: u16) -> Result<ServiceDaemon, String> {
    let info = service_info(host_id, host, port).map_err(discovery_error)?;
    let daemon = ServiceDaemon::new().map_err(discovery_error)?;
    daemon.register(info).map_err(discovery_error)?;
    Ok(daemon)
}

fn discovery_error(error: mdns_sd::Error) -> String {
    error.to_string()
}

fn service_info(host_id: &str, host: &str, port: u16) -> Result<ServiceInfo, mdns_sd::Error> {
    let hostname = format!(
        "osg-{}.local.",
        host_id
            .chars()
            .filter(|value| value.is_ascii_alphanumeric() || *value == '-')
            .take(48)
            .collect::<String>()
    );
    let address = host
        .parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .unwrap_or_default();
    let properties = [
        ("hostId", host_id),
        (ADVERTISED_HOST, host),
        ("version", "1"),
        ("path", "/companion"),
        ("trust", "out-of-band-required"),
    ];
    let info = ServiceInfo::new(
        SERVICE_TYPE,
        &format!("Cockpit-{}", &host_id[..host_id.len().min(12)]),
        &hostname,
        address.as_str(),
        port,
        &properties[..],
    )?;
    Ok(if address.is_empty() {
        info.enable_addr_auto()
    } else {
        info
    })
}

fn discovered_endpoint(host: Option<&str>, port: u16) -> Option<String> {
    let host = host.filter(|host| host.len() <= 253)?;
    if port == 0 || ServerName::try_from(host).is_err() {
        return None;
    }
    Some(listener_endpoint(host, port))
}

pub fn discover(duration: Duration) -> Result<Value, String> {
    let daemon = ServiceDaemon::new().map_err(discovery_error)?;
    let receiver = daemon.browse(SERVICE_TYPE).map_err(discovery_error)?;
    let deadline = Instant::now() + duration;
    let mut hints = BTreeMap::new();
    while Instant::now() < deadline {
        match receiver.recv_deadline(deadline) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let Some(hint) = discovery_hint(&info) else {
                    continue;
                };
                hints.insert(info.get_fullname().to_string(), hint);
                if hints.len() >= 32 {
                    break;
                }
            }
            Ok(ServiceEvent::SearchStopped(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let _ = daemon.shutdown();
    Ok(json!({"hints":hints.into_values().collect::<Vec<_>>(),"trusted":false}))
}

fn discovery_hint(info: &ResolvedService) -> Option<Value> {
    let endpoint =
        discovered_endpoint(info.get_property_val_str(ADVERTISED_HOST), info.get_port())?;
    let addresses = info
        .get_addresses()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    Some(json!({
        "name": info.get_fullname(),
        "hostname": info.get_hostname(),
        "port": info.get_port(),
        "addresses": addresses,
        "endpoint": endpoint,
        "hostId": info.get_property_val_str("hostId"),
        "trusted": false,
        "requiresOutOfBandPairing": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_hint_uses_the_advertised_host_and_port() {
        let mut service = service_info("fixture", "Cockpit.example", 54437)
            .unwrap()
            .as_resolved_service();
        service.addresses = service_info("fixture", "192.168.1.2", 54437)
            .unwrap()
            .as_resolved_service()
            .addresses;
        let hint = discovery_hint(&service).unwrap();
        assert_eq!(hint["addresses"], json!(["192.168.1.2"]));
        assert_eq!(hint["hostname"], "osg-fixture.local.");
        assert_eq!(hint["endpoint"], "wss://Cockpit.example:54437/companion");
        assert_eq!(hint["trusted"], false);
        assert_eq!(hint["requiresOutOfBandPairing"], true);
        service.port = 0;
        assert!(discovery_hint(&service).is_none());
    }

    #[test]
    fn discovery_preserves_the_exact_pairing_endpoint() {
        assert_eq!(
            discovered_endpoint(Some("Cockpit.example"), 54437).as_deref(),
            Some("wss://Cockpit.example:54437/companion")
        );
        assert_eq!(
            discovered_endpoint(Some("fd00:0::12"), 54437).as_deref(),
            Some("wss://[fd00:0::12]:54437/companion")
        );
        let label = "a".repeat(63);
        let longest = format!("{label}.{label}.{label}.{}", "a".repeat(61));
        assert_eq!(longest.len(), 253);
        assert_eq!(
            discovered_endpoint(Some(&longest), 54437).unwrap(),
            format!("wss://{longest}:54437/companion")
        );
        assert!(discovered_endpoint(None, 54437).is_none());
        assert!(discovered_endpoint(Some("host@elsewhere"), 54437).is_none());
        assert!(discovered_endpoint(Some("cockpit.local"), 0).is_none());
        assert!(discovered_endpoint(Some(&"x".repeat(254)), 54437).is_none());
    }

    #[test]
    fn bounded_discovery_returns_only_untrusted_hints() {
        let expected_host = std::env::var("OSG_DISCOVERY_EXPECTED_HOST_ID").ok();
        let duration = if expected_host.is_some() { 1200 } else { 25 };
        let result = discover(Duration::from_millis(duration)).unwrap();
        assert_eq!(result["trusted"], false);
        let hints = result["hints"].as_array().unwrap();
        assert!(hints.len() <= 32);
        assert!(hints
            .iter()
            .all(|hint| hint["trusted"] == false && hint["requiresOutOfBandPairing"] == true));
        if let Some(host_id) = expected_host {
            assert!(
                hints.iter().any(|hint| hint["hostId"] == host_id),
                "Running Cockpit was not discovered"
            );
        }
    }
}
