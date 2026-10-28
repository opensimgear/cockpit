use super::{Desktop, Outcome, OUTCOME_LIMIT};
use osg_desktop_output::protocol::{Command, Request, FRAME_LIMIT, MAX_BATCH};
use osg_mapping::desktop::{Delivery, Intent};
use osg_mapping::engine::Counter;
use std::{collections::BTreeSet, time::Instant};

impl Desktop {
    pub(super) fn flush_dispatch(&mut self, now: u64) -> Result<(), String> {
        let started = Instant::now();
        let mut cancelled = BTreeSet::new();
        let mut pending = self
            .scheduler
            .drain()
            .into_iter()
            .collect::<std::collections::VecDeque<_>>();

        while let Some(delivery) = pending.pop_front() {
            if self.drop_reason(&delivery, &cancelled).is_some() {
                self.drop_delivery(delivery, &cancelled);
                continue;
            }

            if matches!(delivery.intent, Intent::Pointer { .. }) {
                // Reserve the largest possible sequence/deadline envelope.
                // Oversized candidates remain queued for the next request.
                let mut encoded_size = pointer_batch_envelope_size()?
                    + serde_json::to_vec(&delivery)
                        .map_err(|error| error.to_string())?
                        .len();
                let mut group = vec![delivery];
                while group.len() < MAX_BATCH {
                    let Some(next) = pending.front() else { break };
                    if !same_pointer_batch(&group[0], next)
                        || self.drop_reason(next, &cancelled).is_some()
                    {
                        break;
                    }
                    let next_size = encoded_size.saturating_add(1).saturating_add(
                        serde_json::to_vec(next)
                            .map_err(|error| error.to_string())?
                            .len(),
                    );
                    if next_size > FRAME_LIMIT {
                        break;
                    }
                    encoded_size = next_size;
                    group.push(pending.pop_front().expect("front was present"));
                }

                if group.len() > 1 {
                    // Every action must own its reducer slot before the worker sees motion.
                    for delivery in &group {
                        self.arm_if_needed(delivery)?;
                    }
                    let acknowledgements = self
                        .output
                        .submit_pointer_batch(group.clone(), effective_source_now(now, started))?;
                    if acknowledgements.len() != group.len() {
                        return Err(
                            "Desktop pointer batch returned an incomplete acknowledgement list."
                                .into(),
                        );
                    }
                    for (delivery, acknowledgement) in group.into_iter().zip(acknowledgements) {
                        self.record_outcome(delivery, acknowledgement, &mut cancelled)?;
                    }
                    continue;
                }
                let delivery = group.pop().expect("single pointer group");
                self.arm_if_needed(&delivery)?;
                let acknowledgement = self
                    .output
                    .submit(delivery.clone(), effective_source_now(now, started))?;
                self.record_outcome(delivery, acknowledgement, &mut cancelled)?;
                continue;
            }

            self.arm_if_needed(&delivery)?;
            let acknowledgement = self
                .output
                .submit(delivery.clone(), effective_source_now(now, started))?;
            self.record_outcome(delivery, acknowledgement, &mut cancelled)?;
        }

        if !cancelled.is_empty() {
            self.flush_dispatch(effective_source_now(now, started))?;
        }
        Ok(())
    }

    fn drop_reason(&self, delivery: &Delivery, cancelled: &BTreeSet<String>) -> Option<u64> {
        let cleanup = is_cleanup(delivery);
        if cleanup && self.armed.get(&delivery.action_id) != Some(&delivery.producer) {
            Some(1)
        } else if !cleanup && cancelled.contains(&delivery.action_id) {
            Some(2)
        } else {
            None
        }
    }

    fn drop_delivery(&self, delivery: Delivery, cancelled: &BTreeSet<String>) {
        let reason = self.drop_reason(&delivery, cancelled).unwrap_or(2);
        self.audit_drop(&delivery, reason);
    }

    fn arm_if_needed(&mut self, delivery: &Delivery) -> Result<(), String> {
        if !is_cleanup(delivery) && self.armed.get(&delivery.action_id) != Some(&delivery.producer)
        {
            self.output
                .arm(delivery.producer.clone(), delivery.action_id.clone())?;
            self.armed
                .insert(delivery.action_id.clone(), delivery.producer.clone());
        }
        Ok(())
    }

    fn record_outcome(
        &mut self,
        delivery: Delivery,
        acknowledgement: osg_desktop_output::protocol::Ack,
        cancelled: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if acknowledgement.collision || acknowledgement.skipped {
            if self.diagnostics.len() == 32 {
                self.diagnostics.pop_front();
            }
            self.diagnostics.push_back(if acknowledgement.collision {
                format!("Desktop action {} shares a held native control; its pulse cannot manufacture a release edge.",delivery.action_id)
            } else {
                format!("Desktop action {} expired before submission and was skipped.",delivery.action_id)
            });
        }
        if acknowledgement.skipped
            && matches!(&delivery.intent, Intent::Held { controls } if !controls.is_empty())
        {
            self.scheduler.cancel_action(
                &delivery.action_id,
                "Submission deadline expired; queued taps cancelled.",
            )?;
            cancelled.insert(delivery.action_id.clone());
        }
        if self.outcomes.len() == OUTCOME_LIMIT {
            self.outcomes.pop_front();
            self.dropped_outcomes = self.dropped_outcomes.saturating_add(1);
        }
        self.outcomes.push_back(Outcome {
            delivery,
            acknowledgement,
        });
        Ok(())
    }
}

fn pointer_batch_envelope_size() -> Result<usize, String> {
    serde_json::to_vec(&Request {
        sequence: Counter(u64::MAX),
        deadline_us: Some(Counter(u64::MAX)),
        command: Command::DeliverPointerBatch {
            deliveries: Vec::new(),
        },
    })
    .map(|bytes| bytes.len())
    .map_err(|error| error.to_string())
}

fn is_cleanup(delivery: &Delivery) -> bool {
    matches!(&delivery.intent, Intent::Cleanup)
        || matches!(&delivery.intent, Intent::Held { controls } if controls.is_empty())
}

fn same_pointer_batch(first: &Delivery, next: &Delivery) -> bool {
    matches!(first.intent, Intent::Pointer { .. })
        && matches!(next.intent, Intent::Pointer { .. })
        && first.at_us == next.at_us
        && first.producer.run == next.producer.run
}

fn effective_source_now(source_now_us: u64, started: Instant) -> u64 {
    source_now_us.saturating_add(started.elapsed().as_micros().min(u64::MAX as u128) as u64)
}

#[cfg(test)]
#[path = "dispatch_tests.rs"]
mod tests;
