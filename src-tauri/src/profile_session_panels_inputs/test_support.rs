fn events() -> Value {
    json!({"ok":true,"schemaVersion":1,"stream":7,"head":0,"oldest":1,"overflow":0,
        "buttonCount":1,"held":[false],"contactButtonCount":1,"maintainedButtons":[0],"encoderButtons":[],"selectorButtons":[],
        "axisCount":1,"axes":[65535],"axisNeutral":[32767],"axisUsage":[48],"axisAvailable":[true],"sampledAtMs":10})
}
fn usb_token() -> UsbRouteToken {
    UsbRouteToken {
        module_id: "4f53474400112233445566778899aabb".into(),
        serial: "00112233445566778899aabb".into(),
        connection_generation: 1,
        boot: 2,
        configuration_generation: 3,
    }
}
fn native_route(can: bool, key: &str) -> Route {
    Route {
        id: if can {
            format!("can:{key}:module")
        } else {
            format!("usb:{key}")
        },
        transport: if can { Transport::Can } else { Transport::Usb },
        provider_generation: if can {
            json!([1, 2, 3, 4, 7]).to_string()
        } else {
            "1:2:3".into()
        },
        attributes: if can {
            BTreeMap::from([
                ("gatewayId".into(), key.into()),
                ("moduleId".into(), "module".into()),
                ("node".into(), "5".into()),
                ("boot".into(), "2".into()),
            ])
        } else {
            BTreeMap::new()
        },
        baseline_ready: true,
        writable: false,
        priority: 100,
    }
}
struct Block {
    entered: Mutex<bool>,
    released: Mutex<bool>,
    cv: Condvar,
}
impl Block {
    fn new() -> Self {
        Self {
            entered: Mutex::new(false),
            released: Mutex::new(false),
            cv: Condvar::new(),
        }
    }
    fn wait(&self) {
        *self.entered.lock().unwrap() = true;
        self.cv.notify_all();
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.cv.wait(released).unwrap();
        }
    }
    fn entered(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !*self.entered.lock().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.cv.notify_all();
    }
}
struct Fake {
    data: Mutex<Value>,
    journal: Mutex<Vec<Value>>,
    routes: Mutex<BTreeMap<String, Route>>,
    usb: Mutex<UsbRouteToken>,
    calls: AtomicUsize,
    automatic_clock: AtomicBool,
    block: Mutex<Option<(String, Arc<Block>)>>,
    fail: AtomicBool,
}
impl Fake {
    fn new() -> Self {
        Self {
            data: Mutex::new(events()),
            journal: Mutex::new(vec![]),
            routes: Mutex::new(BTreeMap::new()),
            usb: Mutex::new(usb_token()),
            calls: AtomicUsize::new(0),
            automatic_clock: AtomicBool::new(false),
            block: Mutex::new(None),
            fail: AtomicBool::new(false),
        }
    }
    fn press(&self, pressed: bool) {
        let mut data = self.data.lock().unwrap();
        let mut history = self.journal.lock().unwrap();
        let sequence = data["head"].as_u64().unwrap() + 1;
        history.push(json!({"sequence":sequence,"control":0,"pressed":pressed}));
        data["head"] = json!(sequence);
        data["held"][0] = json!(pressed);
        let time = data["sampledAtMs"].as_u64().unwrap() + 1;
        data["sampledAtMs"] = json!(time);
    }
    fn reply(&self, cursor: Option<(u32, u32)>) -> Value {
        let mut state = self.data.lock().unwrap();
        if self.automatic_clock.load(Ordering::Acquire) {
            let n = state["sampledAtMs"].as_u64().unwrap() + 1;
            state["sampledAtMs"] = json!(n);
        }
        let mut data = state.clone();
        drop(state);
        if let Some((_, after)) = cursor {
            let entries = self
                .journal
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e["sequence"].as_u64().unwrap() > after as u64)
                .take(16)
                .cloned()
                .collect::<Vec<_>>();
            let next = entries
                .last()
                .map_or(after as u64, |e| e["sequence"].as_u64().unwrap());
            data["after"] = json!(after);
            data["events"] = json!(entries);
            data["next"] = json!(next);
            data["more"] = json!(next < data["head"].as_u64().unwrap());
        }
        data
    }
    fn state(&self, key: &str, events: Value) -> Value {
        let route = self.routes.lock().unwrap().get(key).unwrap().clone();
        let values: Vec<Value> = serde_json::from_str(&route.provider_generation).unwrap();
        let node: Value = serde_json::from_str(&route.attributes["node"]).unwrap();
        let mut events = events;
        events["route"] = json!([values[0], node, values[2], values[1]]);
        events["configurationGeneration"] = values[3].clone();
        json!({"connected":true,"epoch":values[0],"modules":[{"id":"module","online":true,"node":node,"lease":values[2],"boot":values[1],"configurationGeneration":values[3]}],"moduleEvents":{"module":events}})
    }
    fn before(&self, key: &str) {
        self.calls.fetch_add(1, Ordering::AcqRel);
        let block = self
            .block
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(blocked, _)| blocked == key)
            .map(|(_, b)| b.clone());
        if let Some(block) = block {
            block.wait();
        }
    }
}
impl Io for Fake {
    fn can(&self, r: Value) -> Value {
        let key = r["gatewayId"].as_str().unwrap();
        self.before(key);
        if self.fail.load(Ordering::Acquire) {
            return json!({"ok":false,"error":"device read failed\nprivate"});
        }
        let cursor = r["after"]
            .as_u64()
            .map(|after| (r["stream"].as_u64().unwrap() as u32, after as u32));
        json!({"ok":true,"state":self.state(key,self.reply(cursor))})
    }
    fn can_cached(&self, key: &str) -> Result<Value, String> {
        if !self.routes.lock().unwrap().contains_key(key) {
            return Err("missing gateway".into());
        }
        Ok(self.state(key, self.data.lock().unwrap().clone()))
    }
    fn usb(&self, key: &str, r: UsbRequest) -> Result<UsbSnapshot, String> {
        self.before(key);
        if self.fail.load(Ordering::Acquire) {
            return Err("USB read failed".into());
        }
        let cursor = match r {
            UsbRequest::EventsState { .. } => None,
            UsbRequest::EventsRead { stream, after, .. } => Some((stream, after)),
            _ => panic!("read-only worker attempted mutation"),
        };
        let route = self.usb.lock().unwrap().clone();
        let mut events = self.reply(cursor);
        events["route"] = json!(route);
        Ok(UsbSnapshot {
            connected: true,
            route: Some(route),
            events: Some(events),
            ..Default::default()
        })
    }
    fn snapshots(&self) -> Vec<SlotSnapshot> {
        let route = self.usb.lock().unwrap().clone();
        self.routes
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, r)| r.transport == Transport::Usb)
            .map(|(key, _)| SlotSnapshot {
                key: key.clone(),
                port: crate::transport::PortInfo {
                    path: "COM4".into(),
                    serial_number: Some(route.serial.clone()),
                    vendor_id: None,
                    product_id: None,
                    manufacturer: None,
                    friendly_name: None,
                },
                present: true,
                automatic: false,
                admissible: true,
                snapshot: UsbSnapshot {
                    connected: true,
                    route: Some(route.clone()),
                    ..Default::default()
                },
                diagnostics: vec![],
            })
            .collect()
    }
}
fn empty_plan() -> ExecutionPlan {
    osg_profiles::compiler::compile(
        &osg_profiles::default_document(),
        &CompileSnapshot {
            inventory_session: "inventory".into(),
            inventory_revision: "1".into(),
            assignments: vec![],
            resources: vec![],
            game_context: None,
            backend: BackendCapabilities::default(),
            inventory_diagnostics: vec![],
            read_only: false,
        },
    )
    .plan
    .unwrap()
}
fn fixture(can: bool, count: usize) -> (ExecutionPlan, Arc<Fake>) {
    let io = Arc::new(Fake::new());
    let mut registry = Registry::new();
    let descriptors = crate::profile_resource_ports::events(&events()).unwrap();
    for n in 0..count {
        let key = format!("panel{n}");
        let route = native_route(can, &key);
        io.routes.lock().unwrap().insert(key.clone(), route.clone());
        registry
            .reconcile(ProviderBatch {
                scope: format!("panel:{n}"),
                session: format!("native:{n}"),
                revision: 1,
                complete: true,
                observations: vec![Observation {
                    key,
                    name: "Panel".into(),
                    resource_type: ResourceType::OsgModule,
                    evidence: vec![],
                    attributes: BTreeMap::new(),
                    diagnostics: vec![],
                    route: Some(route),
                    ports: Some(PortUpdate::complete(descriptors.clone()).unwrap()),
                }],
            })
            .unwrap();
    }
    let mut plan = empty_plan();
    for (n, resource) in registry.list(0, 128).unwrap().resources.iter().enumerate() {
        let details = registry.inspect(&resource.id, 0, 128).unwrap();
        for port in &details.ports {
            let preparation = registry
                .prepare_port(
                    &resource.id,
                    &port.descriptor.id,
                    &port.contract_hash,
                    Access::Read,
                )
                .unwrap();
            let route = preparation.resolved().route.clone();
            plan.sources.push(SourcePlan {
                endpoint: osg_profiles::Endpoint {
                    node_id: format!("logical{n}"),
                    port_id: port.descriptor.id.clone(),
                },
                inventory_resource_id: Some(resource.id.clone()),
                observed_port_id: Some(port.descriptor.id.clone()),
                contract: port.descriptor.contract.clone(),
                token: Some(
                    registry
                        .token(&resource.id, &port.descriptor.id, Access::Read)
                        .unwrap(),
                ),
                preparation: Some(preparation),
                route: Some(route),
                readiness: PlanReadiness::Ready,
            });
        }
    }
    (plan, io)
}
fn make_source(spec: Spec) -> Source {
    Source {
        spec,
        cursor: None,
        held: vec![],
        sequence: 0,
        at: 0,
        retry: Instant::now(),
        generation: 1,
        sampled_at: None,
    }
}
fn sample<'a>(frame: &'a Frame, id: &str) -> Option<&'a SignalSample> {
    frame
        .changes
        .iter()
        .find(|c| c.port_id == id)
        .map(|c| &c.sample)
}
fn eventually<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(value) = f() {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "bounded condition did not become ready"
        );
        thread::sleep(Duration::from_millis(3));
    }
}
