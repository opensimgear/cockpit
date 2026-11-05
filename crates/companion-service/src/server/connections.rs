//! Shutdown handles only. TLS and application bytes keep one peer owner.
use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct State {
    stopped: bool,
    peers: BTreeMap<String, Arc<socket_io::Cancel>>,
}

#[derive(Default)]
pub(super) struct Connections(Mutex<State>);

impl Connections {
    pub(super) fn register(
        &self,
        id: &str,
        cancel: Arc<socket_io::Cancel>,
    ) -> std::io::Result<()> {
        let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if state.stopped {
            drop(state);
            cancel.cancel();
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "Companion listener stopped",
            ));
        }
        if state.peers.len() >= 16 || state.peers.contains_key(id) {
            return Err(std::io::Error::other(
                "Companion peer shutdown registry is full or duplicated",
            ));
        }
        state.peers.insert(id.into(), cancel);
        Ok(())
    }

    pub(super) fn remove(&self, id: &str) {
        let peer = {
            self.0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .peers
                .remove(id)
        };
        drop(peer);
    }

    pub(super) fn close(&self) {
        let peers = {
            let mut state = self.0.lock().unwrap_or_else(|p| p.into_inner());
            state.stopped = true;
            std::mem::take(&mut state.peers)
        };
        // Never hold the registry while waking peer I/O.
        for peer in peers.into_values() {
            peer.cancel();
        }
    }
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;
