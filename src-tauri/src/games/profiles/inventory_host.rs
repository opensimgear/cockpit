#[derive(Default, PartialEq, Eq)]
struct HostUsbEvidence {
    verified: BTreeMap<String, String>,
    usb_ports: BTreeSet<(String, u16, u16)>,
}

fn collect_host(
    controllers: &osg_controller_input::ControllerService,
    usb_evidence: &HostUsbEvidence,
    revision: u64,
) -> Result<ProviderBatch, String> {
    controllers
        .authoritative_resource_inventory()
        .map(|controllers| {
            let host = controllers
                .sources
                .into_iter()
                .map(|source| {
                    let mut result = observation(
                        source.saved.id.clone(),
                        source.saved.name,
                        ResourceType::HostController,
                    );
                    result.evidence.push(evidence(
                        EvidenceKind::LocalPath,
                        format!("host:{}", source.saved.id),
                    ));
                    if let Some(serial) = &source.saved.identity.serial {
                        result
                            .evidence
                            .push(evidence(EvidenceKind::Serial, serial.clone()));
                        if let Some((module, _)) = usb_evidence
                            .verified
                            .iter()
                            .find(|(_, uid)| uid.eq_ignore_ascii_case(serial))
                        {
                            // Match the verified USB interface's VID/PID as well. A name
                            // or a familiar VID/PID by itself never establishes identity.
                            if usb_evidence.usb_ports.contains(&(
                                serial.to_ascii_lowercase(),
                                source.saved.identity.vendor_id,
                                source.saved.identity.product_id,
                            )) {
                                result.evidence.push(evidence(
                                    EvidenceKind::VerifiedPhysical,
                                    format!("osg-mcu:{module}"),
                                ));
                                result.resource_type = ResourceType::OsgModule;
                            }
                        }
                    }
                    result.attributes = attributes([
                        ("backend", source.saved.identity.backend),
                        ("collection", source.saved.identity.collection),
                    ]);
                    result.diagnostics = source.diagnostics;
                    if !controllers.diagnostic.is_empty() {
                        result.diagnostics.push(controllers.diagnostic.clone());
                    }
                    result.ports = (!source.controls.is_empty())
                        .then(|| PortUpdate::complete(ports::controls(&source.controls)))
                        .transpose()
                        .ok()
                        .flatten();
                    result.route = source.online.then(|| Route {
                        id: format!("hid:{}", source.saved.id),
                        transport: Transport::Hid,
                        provider_generation: source.generation.to_string(),
                        attributes: attributes([("path", source.saved.identity.path)]),
                        baseline_ready: source.baseline_ready,
                        writable: false,
                        priority: 10,
                    });
                    result
                })
                .collect();
            batch("host-controllers", &controllers.session, revision, host)
        })
}
