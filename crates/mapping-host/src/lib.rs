//! Native mapping coordination. Captured reports never pass through UI polling.
pub mod router;
pub mod desktop;
pub mod service;
pub mod settings;

mod learn;
pub use learn::Learn;

pub use osg_logic::helper_main as logic_helper_main;
pub use osg_logic::typed_worker::TypedWorker;

pub mod panels;

pub mod companions;
pub mod typed_sources;
