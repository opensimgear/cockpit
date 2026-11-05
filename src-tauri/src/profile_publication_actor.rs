use super::*;
use crate::stm32::PublicationOperation::{Claim, Release, Renew, State};

const SETTLEMENT: Duration = Duration::from_secs(3);
const RENEWAL: Duration = Duration::from_secs(1);
const LEASE: Duration = Duration::from_secs(5);
const CLEANUP: Duration = Duration::from_secs(8);

struct OwnerProof {
    lease: u32,
    fence: PublicationFence,
}

struct Observation {
    fence: PublicationFence,
    state: Value,
    lease: Option<u32>,
}

fn current(io: &dyn Io, gateway: &str, fence: &PublicationFence) -> bool {
    io.fence(gateway)
        .is_some_and(|latest| fence.same_stream(&latest) && fence.is_current())
}

fn observe(
    io: &dyn Io,
    value: &Cleanup,
    fence: &PublicationFence,
    known: Option<&OwnerProof>,
) -> Result<Option<Inspection>, String> {
    let state = request(io, value, State, None, fence)?.state;
    if !current(io, &value.gateway, fence) {
        return Ok(None);
    }
    let inspection = if let Some(known) = known.filter(|known| {
        known.fence.same_stream(fence)
            && candidate(value, &state)
            && state["publication"]["leaseId"] == known.lease
            && gateway_identity(&state).as_deref() == Some(value.gateway.as_str())
            && state["connected"] == true
    }) {
        Inspection::Owned(known.lease, state)
    } else {
        inspect(io, value, &state, fence)?
    };
    Ok(current(io, &value.gateway, fence).then_some(inspection))
}

fn delivery(value: &Cleanup, state: &Value) -> Vec<Value> {
    value
        .targets
        .iter()
        .filter_map(|target| module_delivery(target, state).map(|(lease, _)| lease.clone()))
        .collect()
}

fn cleanup_result(
    value: &Cleanup,
    observation: &Observation,
    error: &mut Option<String>,
) -> Option<Status> {
    let state = &observation.state;
    if observation.lease.is_some()
        || gateway_identity(state).as_deref() != Some(value.gateway.as_str())
        || state["connected"] != true
        || state["epoch"] != value.epoch
        || state["publication"]["phase"] != "idle"
        || state["layoutPending"] != false
    {
        return None;
    }
    let pristine = state["publication"]["outcome"] == "ok"
        && state["publication"]["leaseId"] == 0
        && value.targets.iter().all(|t| matches_target(t, state));
    let restored = confirmed_restored(value, state);
    if state["publication"]["outcome"] == "preserved" {
        *error = Some("An independently changed publication was preserved; its original layout was not restored.".into());
    } else if restored {
        *error = None;
    } else if !pristine {
        error.get_or_insert_with(|| "Publication restoration has no exact settled readback of the sealed original targets; recovery remains pending.".into());
        return None;
    }
    Some(Status {
        active: false,
        stopped: true,
        cleanup_complete: true,
        error: error.clone(),
    })
}

pub(super) fn drive(
    io: Arc<dyn Io>,
    value: Cleanup,
    shared: &Shared,
    recovery: bool,
    update: impl Fn(Status),
) -> Status {
    shared
        .wake
        .bind_thread(thread::current())
        .expect("A publication slot owns one worker");
    loop {
        let observed = shared.wake.observed();
        if shared.start.load(Ordering::Acquire) || shared.stop.load(Ordering::Acquire) {
            break;
        }
        shared.wake.wait_scheduled(observed, None);
    }
    let io = io.as_ref();
    io.watch_inventory(&value.gateway, &shared.wake);
    let mut known = None;
    let mut error = None;
    let mut expected_reconnect = false;
    let mut dispatched = Instant::now();
    if !recovery && !shared.stop.load(Ordering::Acquire) && shared.authority.load(Ordering::Acquire)
    {
        let claim = || -> Result<(Value, PublicationFence), String> {
            let fence = io
                .fence(&value.gateway)
                .filter(|f| f.epoch() == value.epoch)
                .ok_or(INVALID)?;
            let state = io.cached(&value.gateway)?;
            if state["publicationRevision"] != value.revision
                || state["publication"]["phase"] != "idle"
                || state["layoutPending"] != false
                || value.targets.iter().any(|t| !matches_target(t, &state))
                || !current(io, &value.gateway, &fence)
            {
                return Err(INVALID.into());
            }
            let reply = request(
                io,
                &value,
                Claim {
                    owner: value.secret.clone(),
                    revision: value.revision,
                    target_mask: value.target_mask,
                    enabled_mask: value.enabled_mask,
                    ttl_ms: LEASE.as_millis() as u32,
                },
                Some(shared.authority.clone()),
                &fence,
            )?;
            Ok((reply.state, fence))
        };
        dispatched = Instant::now();
        match claim() {
            Ok((state, fence))
                if candidate(&value, &state)
                    && gateway_identity(&state).as_deref() == Some(value.gateway.as_str()) =>
            {
                expected_reconnect = state["publication"]["expectedReconnect"] == true
                    || state["layoutPending"] == true;
                match positive(&state["publication"]["leaseId"]) {
                    Ok(lease) => known = Some(OwnerProof { lease, fence }),
                    Err(e) => error = Some(e),
                }
            }
            Ok(_) => error = Some(INVALID.into()),
            Err(e) => error = Some(e),
        }
    }
    let settlement = dispatched + SETTLEMENT;
    let mut renew_at = dispatched + RENEWAL;
    let mut expires_at = dispatched + LEASE;
    let mut cleanup_until = None;
    let mut explicit_stop = false;
    let mut sampled = None::<PublicationFence>;
    let mut observation = None::<Observation>;
    let mut released = false;
    let mut admitted = None::<(PublicationFence, u32, Vec<Value>)>;

    loop {
        let wake = shared.wake.observed();
        let stop = recovery || shared.stop.load(Ordering::Acquire);
        let new_stop = stop && !explicit_stop;
        if new_stop {
            explicit_stop = true;
            sampled = None;
            observation = None;
        }
        let fence = io
            .fence(&value.gateway)
            .filter(|f| f.epoch() == value.epoch);
        let required_stream = admitted.as_ref().map(|(fence, _, _)| fence).or_else(|| {
            if expected_reconnect {
                None
            } else {
                known.as_ref().map(|known| &known.fence)
            }
        });
        if required_stream.is_some_and(|required| {
            fence
                .as_ref()
                .is_none_or(|fence| !required.same_stream(fence))
        }) {
            error
                .get_or_insert_with(|| "The native CAN publication stream lost continuity.".into());
        }
        let now = Instant::now();
        if now >= expires_at && admitted.is_some() {
            error.get_or_insert_with(|| {
                "The native publication lease reached its renewal deadline.".into()
            });
        }
        if stop || error.is_some() || !shared.authority.load(Ordering::Acquire) {
            shared.authority.store(false, Ordering::Release);
            let deadline = *cleanup_until.get_or_insert(now + CLEANUP);
            if now >= deadline {
                return Status {
                    active: false,
                    stopped: true,
                    cleanup_complete: false,
                    error: Some(error.unwrap_or("Publication restoration remains unverified; retain its sealed recovery receipt.".into())),
                };
            }
            if error.is_some() {
                update(Status {
                    active: false,
                    stopped: false,
                    cleanup_complete: false,
                    error: error.clone(),
                });
            }
        }
        if let Some(fence) = fence.as_ref() {
            let changed = sampled
                .as_ref()
                .is_none_or(|sampled| !sampled.same_stream(fence) || !sampled.is_current());
            if changed {
                sampled = Some(fence.clone());
                observation = None;
                match observe(
                    io,
                    &value,
                    fence,
                    (!new_stop).then_some(known.as_ref()).flatten(),
                ) {
                    Ok(Some(Inspection::Owned(lease, state))) => {
                        expected_reconnect |= state["publication"]["expectedReconnect"] == true;
                        known = Some(OwnerProof {
                            lease,
                            fence: fence.clone(),
                        });
                        observation = Some(Observation {
                            fence: fence.clone(),
                            state,
                            lease: Some(lease),
                        });
                    }
                    Ok(Some(Inspection::Absent(state))) => {
                        observation = Some(Observation {
                            fence: fence.clone(),
                            state,
                            lease: None,
                        });
                    }
                    Ok(Some(Inspection::Foreign)) => {
                        shared.authority.store(false, Ordering::Release);
                        shared
                            .verified
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .take();
                        return Status {
                            active: false,
                            stopped: true,
                            cleanup_complete: true,
                            error: Some(
                                "An independently owned publication was verified and preserved."
                                    .into(),
                            ),
                        };
                    }
                    Ok(None) => {}
                    Err(e) if current(io, &value.gateway, fence) => {
                        error.get_or_insert(e);
                    }
                    Err(_) => {}
                }
            }
        } else {
            sampled = None;
            observation = None;
        }

        let observed = observation
            .as_ref()
            .filter(|observed| current(io, &value.gateway, &observed.fence));
        let now = Instant::now();
        if now >= expires_at && admitted.is_some() {
            error.get_or_insert_with(|| {
                "The native publication lease reached its renewal deadline.".into()
            });
        }
        let mut active = false;
        if !stop && error.is_none() && shared.authority.load(Ordering::Acquire) {
            if let Some((observed, lease)) = observed
                .filter(|o| settled(&value, &o.state))
                .and_then(|o| o.lease.map(|lease| (o, lease)))
            {
                let leases = delivery(&value, &observed.state);
                if admitted
                    .as_ref()
                    .is_some_and(|(_, admitted_lease, previous)| {
                        *admitted_lease != lease || previous != &leases
                    })
                {
                    error = Some("The admitted CAN delivery lease changed independently of the owned publication.".into());
                } else if admitted.is_some() || now < settlement {
                    admitted.get_or_insert_with(|| (observed.fence.clone(), lease, leases.clone()));
                    if now >= renew_at {
                        let dispatched = Instant::now();
                        match request(
                            io,
                            &value,
                            Renew {
                                owner: value.secret.clone(),
                                lease,
                                ttl_ms: LEASE.as_millis() as u32,
                            },
                            Some(shared.authority.clone()),
                            &observed.fence,
                        ) {
                            Ok(reply)
                                if candidate(&value, &reply.state)
                                    && settled(&value, &reply.state)
                                    && reply.state["publication"]["leaseId"]
                                        == observed.state["publication"]["leaseId"]
                                    && delivery(&value, &reply.state) == leases =>
                            {
                                renew_at = dispatched + RENEWAL;
                                expires_at = dispatched + LEASE;
                            }
                            Ok(_) => error = Some(INVALID.into()),
                            Err(e) if e == crate::stm32::PUBLICATION_EVIDENCE_CHANGED => {
                                // The transport rejected the write before dispatch. A newer
                                // fence can still prove the same lease before it expires.
                                observation = None;
                                shared
                                    .verified
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .take();
                                continue;
                            }
                            Err(e) => error = Some(e),
                        }
                    }
                    active = error.is_none() && current(io, &value.gateway, &observed.fence);
                    if active {
                        *shared.verified.lock().unwrap_or_else(|p| p.into_inner()) = Some(Proof {
                            state: observed.state.clone(),
                            live: Some((expires_at, observed.fence.clone())),
                        });
                    }
                }
            } else if observed.is_some() && admitted.is_some() {
                error = Some(
                    "The temporary CAN publication lost its exact settled native owner.".into(),
                );
            }
            if admitted.is_none() && now >= settlement {
                error.get_or_insert_with(|| {
                    "The temporary CAN publication did not settle before its deadline.".into()
                });
            }
        }
        let stopping = stop || !shared.authority.load(Ordering::Acquire) || error.is_some();
        let deadline = if stopping {
            if !stop && error.is_none() {
                error = Some("The native publication authority was withdrawn; only ordered restoration remains available.".into());
            }
            shared.authority.store(false, Ordering::Release);
            shared
                .verified
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            active = false;
            let deadline = *cleanup_until.get_or_insert_with(|| Instant::now() + CLEANUP);
            if let Some(observed) = observed {
                if let Some(status) = cleanup_result(&value, observed, &mut error) {
                    if status.error.is_none() {
                        *shared.verified.lock().unwrap_or_else(|p| p.into_inner()) = Some(Proof {
                            state: observed.state.clone(),
                            live: None,
                        });
                    }
                    return status;
                }
                if let Some(lease) = observed.lease.filter(|_| stop && !released) {
                    if current(io, &value.gateway, &observed.fence) {
                        released = true;
                        match request(
                            io,
                            &value,
                            Release {
                                owner: value.secret.clone(),
                                lease,
                            },
                            None,
                            &observed.fence,
                        ) {
                            Ok(_) => {
                                sampled = None;
                                continue;
                            }
                            Err(e) => {
                                error.get_or_insert(e);
                            }
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Status {
                    active: false,
                    stopped: true,
                    cleanup_complete: false,
                    error: Some(error.unwrap_or("Publication restoration remains unverified; retain its sealed recovery receipt.".into())),
                };
            }
            deadline
        } else if admitted.is_some() {
            if observed.is_some() {
                renew_at.min(expires_at)
            } else {
                expires_at
            }
        } else {
            settlement
        };
        update(Status {
            active,
            stopped: false,
            cleanup_complete: false,
            error: error.clone(),
        });
        shared.wake.wait_scheduled(
            wake,
            Some(deadline.saturating_duration_since(Instant::now())),
        );
    }
}
