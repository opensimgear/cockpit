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
    bios: Option<BiosWatch>,
    physics: Option<PhysicsWatch>,
    xplane: Option<XplaneWatch>,
}
impl SourceWakes {
    pub(super) fn clear(&mut self) {
        self.bios = None;
        self.physics = None;
        self.xplane = None;
    }
}
impl GameSources {
    pub(crate) fn set_wake(&mut self, signal: Option<Arc<NativeWake>>) {
        let same = match (&self.notifications.signal, &signal) {
            (Some(old), Some(next)) => Arc::ptr_eq(old, next),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.notifications.clear();
            self.notifications.signal = signal;
        }
    }
    pub(crate) fn notification_wake(&self) -> Option<Arc<NativeWake>> {
        self.notifications.signal.clone()
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
            _ => {}
        }
        Ok(())
    }
}
