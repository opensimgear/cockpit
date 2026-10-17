//! Restricted Rhai evaluation runs only in a disposable child process.
pub mod evaluator;
mod resources;
pub mod wire;
pub mod worker;
pub use worker::helper_main;

pub mod replay;

#[cfg(any(target_os = "macos", test))]
mod heap;
#[cfg(target_os = "macos")]
#[global_allocator]
static HELPER_HEAP: heap::Bounded = heap::Bounded::new();

pub mod typed;
mod typed_evaluator;
pub mod typed_worker;

#[cfg(test)]
mod typed_test_support;
