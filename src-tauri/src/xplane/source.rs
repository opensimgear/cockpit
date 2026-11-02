pub(crate) struct Source {
    socket: UdpSocket,
    connection: Value,
    paths: Vec<String>,
    supported: Vec<bool>,
    plan: subscriptions::SubscriptionPlanner,
    wire_indices: BTreeMap<i32, Vec<usize>>,
    subscribed: Instant,
    session: Session,
    monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    outputs: Vec<Value>,
    catalogue: BTreeMap<String, datarefs::Channel>,
    inspection: Vec<String>,
    pending_subscriptions: VecDeque<Vec<u8>>,
    last_subscription_batch: Instant,
    initializing_subscriptions: bool,
    terminal: Option<String>,
}
impl Source {
    pub(crate) fn new(connection: &Value, outputs: Vec<Value>) -> Result<Self, String> {
        validate_connection(connection)?;
        if !cfg!(windows) {
            return Err("X-Plane native process monitoring is currently supported on Windows; Linux/macOS profiles can be edited but cannot start.".into());
        }
        let identity =
            crate::game_discovery::ProcessIdentity::parse(&connection["processIdentity"])?;
        let game = format!("x-plane-{}", connection["version"].as_u64().unwrap());
        use crate::game_discovery::Platform;
        crate::game_discovery::NativePlatform.validate(&game, &identity.path)?;
        crate::game_discovery::exclusive_xplane(&identity)?;
        let available =
            crate::xplane_installation::available(std::path::Path::new(&identity.path))?;
        for path in GUARDS
            .iter()
            .copied()
            .chain(Some("sim/aircraft/view/acf_relative_path"))
        {
            if !available.contains(path) {
                return Err(format!("Required X-Plane session dataref is unavailable: {path}. Repair the simulator installation."));
            }
        }
        let mut channels = legacy_channels();
        for channel in &mut channels {
            channel.metadata["supported"] = json!(available.contains(channel.dataref.split('[').next().unwrap()));
        }
        channels.extend(datarefs::load(std::path::Path::new(&identity.path))?);
        let catalogue = channels.into_iter().map(|c| (c.id.clone(), c)).collect();
        let selected = Self::selected(&catalogue, &outputs, &[])?;
        let supported = vec![true; GUARDS.len() + PATH_BYTES + selected.len()];
        let mut last_check = Instant::now();
        let mut monitor_error: Option<String> = None;
        Self::connect_selected(
            connection,
            outputs,
            supported,
            catalogue,
            selected,
            Box::new(move || {
                if let Some(error) = &monitor_error {
                    return Err(error.clone());
                }
                if last_check.elapsed() < Duration::from_millis(250) {
                    return Ok(());
                }
                last_check = Instant::now();
                use crate::game_discovery::Platform;
                let result = (|| {
                    if !crate::game_discovery::NativePlatform.alive(&identity)? {
                        return Err("Selected X-Plane process exited. Stop and reactivate its game profile.".to_owned());
                    }
                    crate::game_discovery::exclusive_xplane(&identity)
                })();
                if let Err(error) = &result {
                    monitor_error = Some(error.clone());
                }
                result
            }),
        )
    }
    fn connect(
        connection: &Value,
        outputs: Vec<Value>,
        supported: Vec<bool>,
        monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    ) -> Result<Self, String> {
        let channels = legacy_channels();
        let catalogue = channels.iter().map(|c| (c.id.clone(), c.clone())).collect();
        Self::connect_selected(connection, outputs, supported, catalogue, channels, monitor)
    }
    fn connect_selected(
        connection: &Value,
        outputs: Vec<Value>,
        supported: Vec<bool>,
        catalogue: BTreeMap<String, datarefs::Channel>,
        channels: Vec<datarefs::Channel>,
        monitor: Box<dyn FnMut() -> Result<(), String> + Send>,
    ) -> Result<Self, String> {
        validate_connection(connection)?;
        let socket = Self::socket(connection)?;
        let paths = subscriptions_for(&channels);
        let mut plan = subscriptions::SubscriptionPlanner::with_start_index(
            subscriptions_for(&[]), Self::base()).map_err(|e| e.to_string())?;
        plan.add_lease("profile", channels.iter().map(|c| c.dataref.clone()))
            .map_err(|e| e.to_string())?;
        plan.add_lease("inspection", Vec::<String>::new()).map_err(|e| e.to_string())?;
        let mut source = Self {
            socket,
            connection: connection.clone(),
            paths,
            supported,
            plan,
            wire_indices: BTreeMap::new(),
            subscribed: Instant::now(),
            session: Session::new(connection),
            monitor,
            outputs,
            catalogue,
            inspection: vec![],
            pending_subscriptions: VecDeque::new(),
            last_subscription_batch: Instant::now() - Duration::from_millis(100),
            initializing_subscriptions: true,
            terminal: None,
        };
        source.session.channels = channels;
        source.rebuild_indices();
        source.subscribe(connection["updateHz"].as_i64().unwrap() as i32)?;
        Ok(source)
    }
    fn selected(catalogue: &BTreeMap<String, datarefs::Channel>, outputs: &[Value], inspection: &[String]) -> Result<Vec<datarefs::Channel>, String> {
        let mut selected = BTreeSet::new();
        for output in outputs {
            selected.insert(output["channel"].as_str().ok_or("Choose a numeric X-Plane source channel.")?.to_owned());
        }
        selected.extend(inspection.iter().cloned());
        if selected.len() > 1024 { return Err("Select at most 1024 X-Plane dataref components per reader.".into()); }
        selected.into_iter().map(|id| catalogue.get(&id)
            .filter(|c| c.metadata["supported"] == true).cloned()
            .ok_or_else(|| format!("Selected X-Plane dataref is unavailable in this installation: {id}."))).collect()
    }
    /// Retain profile subscriptions and add only the currently visible passive
    /// inspection page. Only retired page indices are cancelled; unchanged
    /// profile datarefs keep their indices and native evidence.
    pub(crate) fn set_inspection_channels(&mut self, ids: &[String]) -> Result<(), String> {
        if let Some(error) = &self.terminal { return Err(error.clone()); }
        if ids.len() > 512 { return Err("Inspect at most 512 X-Plane components per page.".into()); }
        let mut inspection = ids.to_vec();
        inspection.sort(); inspection.dedup();
        if inspection == self.inspection { return Ok(()); }
        let channels = Self::selected(&self.catalogue, &self.outputs, &inspection)?;
        let mut plan = self.plan.clone();
        let delta = plan.replace_lease("inspection", inspection.iter()
            .map(|id| self.catalogue[id].dataref.clone())).map_err(|e| e.to_string())?;
        let pending = self.pending_delta(&delta)?;
        self.plan = plan;
        self.pending_subscriptions = pending;
        self.paths = subscriptions_for(&channels);
        self.supported = vec![true; self.paths.len()];
        self.session.select_channels(channels);
        self.inspection = inspection;
        self.rebuild_indices();
        // This commits the requested selection, not UDP delivery. The source
        // scheduler dispatches its bounded queue and reports transport failure.
        Ok(())
    }
    fn rebuild_indices(&mut self) {
        self.wire_indices.clear();
        for (i, path) in self.paths.iter().enumerate().filter(|(i, _)| self.supported[*i]) {
            if let Some(index) = self.plan.index_for(path) {
                self.wire_indices.entry(index).or_default().push(i);
            }
        }
    }
    fn pending_delta(&self, delta: &subscriptions::LeaseDelta) -> Result<VecDeque<Vec<u8>>, String> {
        let removed: BTreeSet<_> = delta.removed.iter().map(|entry| entry.index).collect();
        let mut pending: VecDeque<_> = self.pending_subscriptions.iter()
            .filter(|bytes| bytes[5..9] == [0; 4] || !removed.contains(&i32::from_le_bytes(bytes[9..13].try_into().unwrap())))
            .cloned().collect();
        if pending.len() + delta.removed.len() + delta.added.len() > 8192 {
            return Err("X-Plane subscription changes exceeded the bounded request queue.".into());
        }
        for entry in &delta.removed { pending.push_back(request(entry.index, 0, &entry.path)); }
        let hz = self.connection["updateHz"].as_i64().unwrap() as i32;
        for entry in &delta.added { pending.push_back(request(entry.index, hz, &entry.path)); }
        Ok(pending)
    }
    fn rotate_subscriptions(&mut self) -> Result<(), String> {
        let mut plan = self.plan.clone();
        let delta = plan.rotate().map_err(|error| error.to_string())?;
        let pending = self.pending_delta(&delta)?;
        self.plan = plan;
        self.pending_subscriptions = pending;
        self.rebuild_indices();
        Ok(())
    }
    fn base() -> i32 {
        (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 1
    }
    fn socket(v: &Value) -> Result<UdpSocket, String> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, v["listenPort"].as_u64().unwrap() as u16)).map_err(|e| format!("Cannot bind X-Plane UDP reply port; close conflicting listeners or choose another port: {e}"))?;
        socket2::SockRef::from(&socket)
            .set_recv_buffer_size(65536)
            .map_err(|e| e.to_string())?;
        socket
            .connect(SocketAddrV4::new(
                v["host"].as_str().unwrap().parse().unwrap(),
                v["port"].as_u64().unwrap() as u16,
            ))
            .map_err(|e| e.to_string())?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        Ok(socket)
    }
    fn subscribe(&mut self, hz: i32) -> Result<(), String> {
        if hz == 0 {
            // A pending old positive request must never resurrect a page that
            // was replaced before its subscription batch reached the simulator.
            self.pending_subscriptions.retain(|request| request[5..9] == [0;4]);
        }
        let active: Vec<_> = self.plan.current().collect();
        if self.pending_subscriptions.len() + active.len() > 8192 {
            return Err("X-Plane subscription changes exceeded the bounded request queue.".into());
        }
        for entry in active {
            if self.wire_indices.contains_key(&entry.index) {
                self.pending_subscriptions.push_back(request(entry.index, hz, &entry.path));
            }
        }
        self.subscribed = Instant::now();
        self.flush_subscriptions(Instant::now())
    }
    fn flush_subscriptions(&mut self, now: Instant) -> Result<(), String> {
        if now.saturating_duration_since(self.last_subscription_batch) < Duration::from_millis(100) { return Ok(()); }
        // The simulator's own network documentation recommends at most 100
        // requests per 100 ms. Work is advanced by the regular poll loop.
        for _ in 0..100 {
            let Some(bytes) = self.pending_subscriptions.front() else { break; };
            self.socket.send(bytes).map_err(|e| format!("Cannot subscribe to X-Plane UDP datarefs: {e}"))?;
            self.pending_subscriptions.pop_front();
        }
        self.last_subscription_batch = now;
        if self.pending_subscriptions.is_empty() { self.initializing_subscriptions = false; }
        Ok(())
    }
    pub(crate) fn poll(&mut self, now: Instant) -> (ExportSnapshot, Value) {
        if let Some(error) = &self.terminal {
            let (snapshot, mut status, _) = self.session.snapshot(now, &self.outputs);
            status["status"] = json!(error);
            return (snapshot, status);
        }
        let result = (|| -> Result<(), String> {
            (self.monitor)()?;
            self.flush_subscriptions(now)?;
            if now.saturating_duration_since(self.subscribed) >= Duration::from_secs(2) {
                self.subscribe(self.connection["updateHz"].as_i64().unwrap() as i32)?;
                self.subscribed = now;
            }
            let mut bytes = [0; MAX_PACKET + 1];
            // Bound work per poll; no unbounded receive queue is copied into memory.
            for _ in 0..64 {
                match self.socket.recv(&mut bytes) {
                    Ok(n) => {
                        let values = decode_selected(&bytes[..n], &self.wire_indices)?;
                        if values.iter().any(|(i, _)| !self.supported[*i]) {
                            return Err("Received an unavailable X-Plane dataref index.".into());
                        }
                        self.session.ingest(values, now);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        return Err(format!(
                            "X-Plane UDP unavailable (check simulator port/firewall): {e}"
                        ))
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.session.reset(error);
        }
        let (mut snapshot, mut status, rotate) = self.session.snapshot(now, &self.outputs);
        if self.initializing_subscriptions {
            snapshot.fresh = false;
            snapshot.mission_active = false;
            snapshot.values.clear();
            status["fresh"] = json!(false);
            status["status"] = json!("Subscribing to selected X-Plane datarefs in bounded batches.");
            for channel in status["channels"].as_object_mut().into_iter().flat_map(|channels| channels.values_mut()) {
                channel["status"] = json!("unavailable"); channel["value"] = Value::Null;
            }
        }
        if snapshot.fresh {
            for id in &self.inspection {
                if status["channels"][id]["status"] == "ready" {
                    snapshot.values.insert(id.clone(), status["channels"][id]["value"].clone());
                }
            }
        }
        let failed = result.is_err();
        if let Err(error) = result {
            status["status"] = json!(error);
        }
        if rotate || failed {
            let replacement = self.rotate_subscriptions();
            self.initializing_subscriptions = true;
            if let Err(error) = replacement {
                let error = format!("X-Plane subscription context could not be fenced; reactivate the reader: {error}");
                self.session.reset(&error);
                self.terminal = Some(error.clone());
                self.wire_indices.clear();
                status["status"] = json!(error);
            }
            self.subscribed = now;
        }
        (snapshot, status)
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        let _ = self.subscribe(0);
        // Do not block shutdown waiting for future poll ticks. Flush remaining
        // cancellations in bounded batches using the documented pacing.
        while !self.pending_subscriptions.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            if self.flush_subscriptions(Instant::now()).is_err() { break; }
        }
    }
}
