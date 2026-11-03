//! Existing authored replay fixtures admit at their scheduled capture time.
//! Live adapters must obtain admission independently from the native clock.
use osg_mapping::engine::{typed::{Frame, Phase}, Counter};

pub(crate) fn frame(phase: &mut Phase, frame: Frame) -> Result<(), String> {
    let admitted = Counter(phase.at_us().0.max(frame.captured_us.0));
    phase.accept(frame, admitted)
}

pub(crate) fn batch(phase: &mut Phase, frames: impl IntoIterator<Item = Frame>) -> Result<Vec<String>, String> {
    let frames = frames.into_iter().collect::<Vec<_>>();
    let admitted = Counter(frames.iter().map(|frame| frame.captured_us.0).max().unwrap_or(0).max(phase.at_us().0));
    phase.accept_batch(frames, admitted)
}
