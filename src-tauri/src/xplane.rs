//! X-Plane 11/12 read-only RREF UDP provider. No simulator files are changed.
use crate::dcs_profile::ExportSnapshot;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    time::{Duration, Instant},
};

pub(crate) const PROVIDER: &str = "x-plane";
#[path = "xplane_datarefs.rs"]
mod datarefs;
const PATH_BYTES: usize = 128;
// 1024 selected components plus the fixed identity guards fit in one bounded
// response even when the simulator coalesces them into a single UDP datagram.
const MAX_PACKET: usize = 16 * 1024;
const UNSUBSCRIBED: &str = "Unsubscribed X-Plane dataref index.";

include!("xplane/catalogue.rs");
include!("xplane/wire.rs");
include!("xplane/session.rs");
include!("xplane/source.rs");
mod subscriptions;

#[cfg(test)]
mod tests;
