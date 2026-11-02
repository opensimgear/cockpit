use super::*;
use std::sync::atomic::Ordering;

struct Capture {
    driver: Driver,
    leases: BTreeMap<String, Selection>,
    sequence: u64,
    captured_at: Option<Instant>,
    expiry: Option<Instant>,
}
impl Capture {
    fn commit(&mut self, shared: &Shared, now: Instant) -> Result<(), String> {
        let (snapshot, rotate) = self.driver.session.native_snapshot(
            now,
            self.captured_at,
            self.sequence,
            &shared.clock_id,
            self.driver.initializing_subscriptions,
        );
        shared.publish(snapshot, now)?;
        self.expiry = self.driver.session.deadline(now);
        if rotate {
            self.driver.rotate_subscriptions()?;
            self.driver.initializing_subscriptions = true;
            self.driver.subscribed = now;
        }
        Ok(())
    }
    fn apply(
        &mut self,
        leases: BTreeMap<String, Selection>,
        catalogue: &Catalogue,
    ) -> Result<bool, String> {
        let ids: BTreeSet<_> = leases
            .values()
            .flat_map(|s| s.ids.iter().cloned())
            .collect();
        if ids.len() > 1024 {
            return Err("Shared X-Plane selection exceeds 1024 components.".into());
        }
        let channels: Vec<_> = ids
            .iter()
            .map(|id| {
                catalogue
                    .get(id)
                    .cloned()
                    .ok_or_else(|| format!("Unknown X-Plane component: {id}"))
            })
            .collect::<Result<_, _>>()?;
        let changed = !self
            .driver
            .session
            .channels
            .iter()
            .map(|c| &c.id)
            .eq(ids.iter());
        let mut plan = self.driver.plan.clone();
        let mut delta = subscriptions::LeaseDelta::default();
        let mut append = |next: subscriptions::LeaseDelta| {
            delta.added.extend(next.added);
            delta.removed.extend(next.removed);
        };
        for id in self.leases.keys().filter(|id| !leases.contains_key(*id)) {
            append(plan.remove_lease(id).map_err(|e| e.to_string())?);
        }
        for (id, selection) in &leases {
            let paths = selection.ids.iter().map(|id| catalogue[id].dataref.clone());
            let next = if self.leases.contains_key(id) {
                plan.replace_lease(id, paths)
            } else {
                plan.add_lease(id, paths)
            }
            .map_err(|e| e.to_string())?;
            append(next);
        }
        let pending = self.driver.pending_delta(&delta)?;
        self.driver.plan = plan;
        self.driver.pending_subscriptions = pending;
        self.driver.paths = subscriptions_for(&[]).into_iter()
            .chain(channels.iter().map(|channel| channel.dataref.clone())).collect();
        self.driver.supported = vec![true; self.driver.paths.len()];
        if changed {
            self.driver.session.select_shared_channels(channels);
        }
        self.driver.rebuild_indices();
        self.leases = leases;
        Ok(changed)
    }
    fn prune(&mut self, catalogue: &Catalogue) -> Result<bool, String> {
        let retained: BTreeMap<_, _> = self
            .leases
            .iter()
            .filter(|(_, selection)| {
                selection
                    .lease
                    .upgrade()
                    .is_some_and(|lease| lease.active.load(Ordering::Acquire))
            })
            .map(|(id, selection)| (id.clone(), selection.clone()))
            .collect();
        if retained.len() == self.leases.len() {
            return Ok(false);
        }
        self.apply(retained, catalogue)
    }
    fn deadline(&self) -> Instant {
        let mut next = self.driver.subscribed + Duration::from_secs(2);
        if !self.driver.pending_subscriptions.is_empty() {
            next = next.min(self.driver.last_subscription_batch + Duration::from_millis(100));
        }
        if let Some(expiry) = self.expiry {
            next = next.min(expiry);
        }
        next
    }
    fn timers(&mut self, shared: &Shared, now: Instant) -> Result<(), String> {
        // Commit actual data expiry before any renewal or batch completion.
        if self.expiry.is_some_and(|at| at <= now) {
            self.commit(shared, now)?;
        }
        if now >= self.driver.subscribed + Duration::from_secs(2) {
            self.driver
                .subscribe(self.driver.connection["updateHz"].as_i64().unwrap() as i32)?;
            self.driver.subscribed = now;
        }
        let was_initializing = self.driver.initializing_subscriptions;
        if !self.driver.pending_subscriptions.is_empty() {
            self.driver.flush_subscriptions(now)?;
        }
        if was_initializing != self.driver.initializing_subscriptions {
            self.commit(shared, now)?;
        }
        Ok(())
    }
    fn receive(&mut self, shared: &Shared, bytes: &[u8], at: Instant) -> Result<(), String> {
        let pairs = match decode_selected(bytes, &self.driver.wire_indices) {
            Ok(pairs) if pairs.is_empty() => return Ok(()),
            Ok(pairs) => pairs,
            Err(error) => {
                self.driver.session.reset(&error);
                self.commit(shared, at)?;
                self.driver.rotate_subscriptions()?;
                self.driver.initializing_subscriptions = true;
                return Ok(());
            }
        };
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("X-Plane capture sequence exhausted.")?;
        if self.captured_at.is_some_and(|last| at < last) {
            return Err("X-Plane native clock regressed.".into());
        }
        self.captured_at = Some(at);
        self.driver.session.ingest(pairs, at);
        self.commit(shared, at)
    }
}

enum Work {
    Released,
    Request(Option<Request>),
    Received(std::io::Result<usize>),
}
async fn next_work(
    shared: &Shared,
    requests: &mut mpsc::Receiver<Request>,
    socket: &tokio::net::UdpSocket,
    bytes: &mut [u8],
) -> Work {
    // Fairness here prevents either a busy socket or rapid inspection changes
    // from starving the other. Safety and real deadlines remain outside it.
    tokio::select! {
        _ = shared.release.notified() => Work::Released,
        request = requests.recv() => Work::Request(request),
        received = socket.recv(bytes) => Work::Received(received),
    }
}
pub(super) async fn run(
    driver: Driver,
    monitor: Option<Arc<process_events::Monitor>>,
    mut requests: mpsc::Receiver<Request>,
    shared: Arc<Shared>,
    catalogue: Arc<Catalogue>,
    stop: CancellationToken,
) {
    let socket = driver
        .socket
        .try_clone()
        .and_then(tokio::net::UdpSocket::from_std);
    let socket = match socket {
        Ok(socket) => socket,
        Err(error) => {
            shared.fail(error.to_string());
            return;
        }
    };
    let mut capture = Capture {
        driver,
        leases: BTreeMap::new(),
        sequence: 0,
        captured_at: None,
        expiry: None,
    };
    let mut bytes = [0; MAX_PACKET + 1];
    let failure = loop {
        let next = capture.deadline();
        let failed = async {
            match monitor.as_ref() {
                Some(monitor) => monitor.failed().await,
                None => std::future::pending::<String>().await,
            }
        };
        let result: Result<(), String> = tokio::select! {
            biased;
            _ = stop.cancelled() => break "X-Plane acquisition closed.".to_owned(),
            error = failed => break error,
            _ = tokio::time::sleep_until(next.into()) => capture.timers(&shared, Instant::now()),
            work = next_work(&shared, &mut requests, &socket, &mut bytes) => match work {
                Work::Released => {
                    if shared.active_leases.load(Ordering::Acquire) == 0 { break "X-Plane acquisition has no active readers.".into(); }
                    capture.prune(&catalogue).and_then(|changed| if changed { capture.commit(&shared, Instant::now()) } else { Ok(()) })
                },
                Work::Request(request) => {
                    let Some(request) = request else { break "X-Plane acquisition closed.".into(); };
                    if request.cancelled.load(Ordering::Acquire) { continue; }
                    let Some(lease) = request.lease.upgrade().filter(|lease| lease.active.load(Ordering::Acquire)) else { continue; };
                    let mut selections: BTreeMap<_,_> = capture.leases.iter().filter(|(_, selection)|
                        selection.lease.upgrade().is_some_and(|lease| lease.active.load(Ordering::Acquire)))
                        .map(|(id, selection)| (id.clone(), selection.clone())).collect();
                    selections.insert(lease.id.clone(), Selection { lease: request.lease, ids: request.ids });
                    let changed = match capture.apply(selections, &catalogue) {
                        Ok(changed) => changed,
                        Err(error) => { let _ = request.reply.send(Err(error)); continue; }
                    };
                    let now = Instant::now();
                    if let Err(error) = if changed || capture.expiry.is_some_and(|at| at <= now) { capture.commit(&shared, now) } else { Ok(()) } {
                        let _ = request.reply.send(Err(error.clone()));
                        break error;
                    }
                    let result = if request.subscribe { shared.journal.subscribe().map(Some).map_err(|e| format!("{e:?}")) } else { Ok(None) };
                    let _ = request.reply.send(result);
                    Ok(())
                },
                Work::Received(received) => match received {
                    Ok(count) => {
                        let at = Instant::now();
                        // A delayed socket callback cannot postpone an earlier expiry.
                        let expired = if capture.expiry.is_some_and(|expiry| expiry <= at) {
                            capture.commit(&shared, at)
                        } else { Ok(()) };
                        expired.and_then(|_| capture.receive(&shared, &bytes[..count], at))
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(()),
                    Err(error) => Err(format!("X-Plane UDP receive failed: {error}")),
                }
            },
        };
        if let Err(error) = result {
            break error;
        }
    };
    capture.driver.session.reset(&failure);
    let _ = capture.commit(&shared, Instant::now());
    shared.fail(failure);
    // The only remaining work is paced protocol cancellation, not acquisition.
    let _ = capture.driver.subscribe(0);
    while !capture.driver.pending_subscriptions.is_empty() {
        tokio::time::sleep_until(
            (capture.driver.last_subscription_batch + Duration::from_millis(100)).into(),
        )
        .await;
        if capture.driver.flush_subscriptions(Instant::now()).is_err() {
            break;
        }
    }
}
