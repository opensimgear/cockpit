use super::*;

impl PanelInputs {
    pub(crate) fn prepare(plan: &ExecutionPlan, io: Arc<ProfilePanelIo>) -> Result<Self, String> {
        Self::prepare_io(plan, io)
    }
    pub(super) fn prepare_io(plan: &ExecutionPlan, io: Arc<dyn Io>) -> Result<Self, String> {
        let mut specs: Vec<Spec> = vec![];
        for source in &plan.sources {
            if source.readiness != PlanReadiness::Ready
                || !source
                    .route
                    .as_ref()
                    .is_some_and(|r| matches!(r.transport, Transport::Usb | Transport::Can))
            {
                continue;
            }
            if source
                .route
                .as_ref()
                .is_some_and(|route| route.id.starts_with("companion:"))
            {
                // USB is a transport shared with companions, whose native
                // publication and input admission belong to their own adapter.
                continue;
            }
            let pin = source
                .preparation
                .as_ref()
                .ok_or("Prepare the exact native panel route before acquiring inputs.")?;
            if pin.access() != osg_profiles::resources::Access::Read {
                return Err("Panel input preparation cannot grant output access.".into());
            }
            let observed = source
                .observed_port_id
                .as_ref()
                .ok_or("Select the exact native panel input port.")?;
            if pin.resolved().port.id != *observed
                || pin.resolved().port.contract != source.contract
                || source.inventory_resource_id.as_ref() != Some(&pin.resolved().resource_id)
                || source.route.as_ref() != Some(&pin.resolved().route)
            {
                return Err(
                    "The panel preparation no longer matches its selected contract.".into(),
                );
            }
            let target = Target::from_route(source.route.as_ref().unwrap(), io.as_ref())?;
            let port = Port {
                logical: source.endpoint.port_id.clone(),
                observed: observed.clone(),
                contract: source.contract.clone(),
                resource_id: pin.resolved().resource_id.clone(),
                contract_hash: pin.resolved().port.contract_hash()?,
            };
            if let Some(spec) = specs
                .iter_mut()
                .find(|s| s.logical == source.endpoint.node_id)
            {
                if spec.target != target || spec.ports.iter().any(|p| p.logical == port.logical) {
                    return Err(
                        "A logical panel source cannot mix routes or duplicate logical ports."
                            .into(),
                    );
                }
                spec.ports.push(port);
            } else {
                specs.push(Spec {
                    logical: source.endpoint.node_id.clone(),
                    target,
                    ports: vec![port],
                });
            }
        }
        if specs.len() > 16 || specs.iter().map(|s| s.ports.len()).sum::<usize>() > 1024 {
            return Err(
                "Panel source acquisition exceeds its bounded resource/port budget.".into(),
            );
        }
        Ok(Self {
            specs,
            io,
            mailbox: Arc::new(Mutex::new(Mailbox::default())),
            revoked: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
            admitted: Arc::new(AtomicBool::new(false)),
            workers: vec![],
            tokens: BTreeMap::new(),
            accepted: BTreeMap::new(),
            loss_epoch: 0,
            clock: None,
            bound: false,
        })
    }
}
