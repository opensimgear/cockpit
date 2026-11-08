//! Passive producer commit notifications. They schedule a serialized read;
//! neither a notification nor its registration grants capture/command authority.
use super::*;
use osg_streams::{NativeWake, NativeWatch};
use std::sync::Arc;

type BiosWatch = NativeWatch<crate::dcs_bios::decoded::Snapshot, crate::dcs_bios::decoded::Snapshot>;
type PhysicsWatch = NativeWatch<crate::dcs_physics::Snapshot, crate::dcs_physics::Snapshot>;
type XplaneWatch = NativeWatch<crate::xplane::NativeSnapshot, crate::xplane::NativeSnapshot>;

#[derive(Default)]
pub(super) struct SourceWakes {
    signal: Option<Arc<NativeWake>>,
    pub(super) process_exit: Option<crate::xplane::process_events::ExitWatch>,
    bios: Option<BiosWatch>,
    physics: Option<PhysicsWatch>,
    xplane: Option<XplaneWatch>,
    assetto: Option<crate::acr_windows::Watch>,
}
impl SourceWakes {
    pub(super) fn process_exited(&self) -> bool {
        self.process_exit.as_ref().is_some_and(crate::xplane::process_events::ExitWatch::exited)
    }

    pub(super) fn clear(&mut self) {
        self.process_exit = None;
        self.clear_captures();
    }

    fn clear_captures(&mut self) {
        self.bios = None;
        self.physics = None;
        self.xplane = None;
        self.assetto = None;
    }
}
impl GameSources {
    #[cfg(test)]
    pub(super) fn install_assetto_watch_fixture(&mut self, watch: crate::acr_windows::Watch) {
        self.notifications.assetto = Some(watch);
    }
    pub(crate) fn set_wake(&mut self, signal: Option<Arc<NativeWake>>) {
        let same = match (&self.notifications.signal, &signal) {
            (Some(old), Some(next)) => Arc::ptr_eq(old, next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.notifications.clear_captures();
            if let Some(exit) = &self.notifications.process_exit { exit.watch(signal.as_ref()); }
            let wake = signal.as_ref().map(Arc::downgrade);
            self.validation.set_wake(wake.clone());
            self.catalogue_validation.set_wake(wake.clone());
            if let Some(worker) = &self.command_worker { worker.set_wake(wake); }
            self.notifications.signal = signal;
        }
    }
    pub(crate) fn notification_wake(&self) -> Option<Arc<NativeWake>> {
        self.notifications.signal.clone()
    }
    pub(crate) fn validation_deadline(&self) -> Option<Instant> {
        [self.validation.next_deadline(), self.catalogue_validation.next_deadline()]
            .into_iter().flatten().min()
    }
    pub(crate) fn watched_game_source(&self) -> bool {
        self.acquired && !self.lost && self.validation.started()
            && match &self.reader {
                Some(Reader::Acr { source: Some(_), .. }) =>
                    self.notifications.assetto.as_ref().is_some_and(|watch| !watch.is_closed()),
                Some(Reader::Xplane { source: Some(_), .. }) =>
                    self.notifications.xplane.as_ref().is_some_and(|watch| !watch.is_closed()),
                Some(Reader::Dcs { transport: Some(_), physics_outputs, .. }) =>
                    self.bios_retry_at.is_none() && self.catalogue_validation.started()
                        && self.notifications.bios.as_ref().is_some_and(|watch| !watch.is_closed())
                        && (physics_outputs.is_empty()
                            || self.notifications.physics.as_ref().is_some_and(|watch| !watch.is_closed())),
                _ => false,
            }
    }
    pub(crate) fn assetto_capture_deadline(&self) -> Option<Instant> {
        if !self.fresh { return None; }
        match &self.reader {
            Some(Reader::Acr { decoder, .. }) => decoder.capture_deadline(),
            _ => None,
        }
    }
    pub(crate) fn baseline_wait_until(&self, deadline: Instant) -> Instant {
        let watched = match self.reader.as_ref() {
            Some(Reader::Dcs { transport, physics_outputs, .. }) => {
                (self.notifications.bios.as_ref().is_some_and(|watch| !watch.is_closed())
                    || transport.is_none() && self.bios_retry_at.is_some())
                    && (physics_outputs.is_empty()
                        || self.notifications.physics.as_ref().is_some_and(|watch| !watch.is_closed()))
            }
            Some(Reader::Xplane { .. }) => self.notifications.xplane.as_ref().is_some_and(|watch| !watch.is_closed()),
            Some(Reader::Acr { .. }) => self.notifications.assetto.as_ref().is_some_and(|watch| !watch.is_closed()),
            None => false,
        };
        if watched {
            self.bios_retry_at.map_or(deadline, |retry| retry.min(deadline))
        } else {
            // An unavailable producer watch cannot promise a future wake.
            deadline.min(Instant::now() + std::time::Duration::from_millis(5))
        }
    }
    pub(super) fn refresh_source_wakes(&mut self, games: &GamesRuntime) -> Result<(), String> {
        if !self.acquired || self.lost {
            self.notifications.clear();
            return Ok(());
        }
        let Some(wake) = self.notifications.signal.clone() else { return Ok(()); };
        match self.reader.as_ref() {
            Some(Reader::Dcs { transport, physics_outputs, .. }) => {
                if self.notifications.bios.as_ref().is_none_or(NativeWatch::is_closed) {
                    self.notifications.bios = transport.as_ref()
                        .map(|source| source.watch(wake.clone())).transpose()?;
                }
                if !physics_outputs.is_empty()
                    && self.notifications.physics.as_ref().is_none_or(NativeWatch::is_closed)
                {
                    self.notifications.physics = self.game.as_ref()
                        .map(|game| games.dcs_physics_watch(&game.proof.process, wake))
                        .transpose()?.flatten();
                }
            }
            Some(Reader::Xplane { source: Some(source), .. }) => {
                if self.notifications.xplane.as_ref().is_none_or(NativeWatch::is_closed) {
                    self.notifications.xplane = Some(source.watch(wake)?);
                }
            }
            Some(Reader::Acr { source: Some(source), .. }) => {
                if self.notifications.assetto.as_ref().is_none_or(crate::acr_windows::Watch::is_closed) {
                    self.notifications.assetto = source.watch(wake)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
