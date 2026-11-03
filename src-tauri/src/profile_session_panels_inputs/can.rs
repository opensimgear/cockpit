use super::*;

pub(super) fn run(spec: Spec, io: Arc<dyn Io>, mailbox: Arc<Mutex<Mailbox>>,
    revoked: Arc<AtomicBool>, active: Arc<AtomicBool>, admitted: Arc<AtomicBool>,
    started: Instant, clock_origin_us: u64) {

                    let mut source = Source {
                        spec, cursor: None, held: vec![], sequence: 0, at: clock_origin_us,
                        retry: Instant::now(), generation: 1, sampled_at: None,
                    };
                    let mut was_active = false;
                    let mut was_admitted = false;
                    let mut startup:Option<Source>=None;
                    let mut epoch = 0;
                    let now = || clock_origin_us.saturating_add(
                        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
                    while !revoked.load(Ordering::Acquire) {
                        let running = active.load(Ordering::Acquire);
                        let delivering = admitted.load(Ordering::Acquire);
                        if running && !was_active {
                            // Events that predate activation are not replayable.
                            source.cursor = None;
                        }
                        was_active = running;
                        if delivering&&!was_admitted {
                            // Restore the exact snapshot accepted by the Host,
                            // discarding any later unadmitted read's cursor.
                            if let Some(checkpoint)=startup.take(){source=checkpoint;}
                        }
                        was_admitted=delivering;
                        {
                            let state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
                            if state.blocked.contains(&source.spec.logical) { break; }
                            if state.loss_epoch != epoch {
                                epoch = state.loss_epoch;
                                source.cursor = None;
                                source.generation = source.generation.saturating_add(1);
                            }
                        }
                        if Instant::now() < source.retry {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        let Some(_permit) = ReadPermit::acquire() else {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        };
                        let baseline = !running || !delivering || source.cursor.is_none();
                        let result = if baseline {
                            source.baseline(io.as_ref(), now()).map(|frame| vec![frame])
                        } else {
                            source.page(io.as_ref(), now())
                        };
                        let after = now();
                        source.at = after;
                        if revoked.load(Ordering::Acquire) { break; }
                        let mut state = mailbox.lock().unwrap_or_else(|p| p.into_inner());
                        // Check again under the publication lock so Stop cannot
                        // clear the mailbox immediately before a late append.
                        if revoked.load(Ordering::Acquire) || state.blocked.contains(&source.spec.logical) { break; }
                        // An inactive read already in flight when Start armed
                        // the reader cannot supply the first live baseline.
                        if running != active.load(Ordering::Acquire) { continue; }
                        if delivering != admitted.load(Ordering::Acquire) { continue; }
                        if state.loss_epoch != epoch { continue; }
                        match result {
                            Ok(mut frames) => {
                                for frame in &mut frames { frame.captured_us = Counter(after); }
                                state.diagnostics.remove(&source.spec.logical);
                                state.liveness.insert(source.spec.logical.clone(),(source.generation,source.sequence,after));
                                if baseline {
                                    if let Some(frame) = frames.first() {
                                        state.baselines.insert(source.spec.logical.clone(), frame.clone());
                                    }
                                    if running&&!delivering {startup=Some(source.clone());}
                                }
                                if running&&delivering {
                                    if state.frames.len() + frames.len() > MAX_QUEUE {
                                        state.frames.clear();
                                        state.baselines.clear();
                                        state.liveness.clear();
                                        state.loss_epoch = state.loss_epoch.saturating_add(1);
                                        state.control();
                                        state.diagnostics.insert(source.spec.logical.clone(), "Panel frame queue overflowed; missing events were discarded and fresh baselines are required.".into());
                                    } else { state.frames.extend(frames); }
                                }
                                source.retry = Instant::now() + Duration::from_millis(if running {10} else {100});
                            }
                            Err(error) => {
                                state.liveness.remove(&source.spec.logical);
                                source.cursor = None;
                                source.retry = Instant::now() + Duration::from_millis(100);
                                state.baselines.remove(&source.spec.logical);
                                if running&&delivering {
                                    if let Ok(gap) = source.frame(after, Reason::Gap, vec![]) {
                                        if state.frames.len() == MAX_QUEUE {
                                            state.frames.clear();
                                            state.baselines.clear();
                                            state.loss_epoch = state.loss_epoch.saturating_add(1);
                                        state.control();
                                        } else { state.frames.push_back(gap); }
                                    }
                                }
                                state.diagnostics.insert(source.spec.logical.clone(), error.chars().filter(|c| !c.is_control()).take(320).collect());
                            }
                        }
                        state.notify();
                    }
}
