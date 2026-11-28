use crate::{Field, ReportKind};

/// HID main-item attributes. The original bit mask is available on the field.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MainFlags {
    pub constant: bool,
    pub variable: bool,
    pub relative: bool,
    pub wrap: bool,
    pub nonlinear: bool,
    pub no_preferred_state: bool,
    pub null_state: bool,
    pub volatile: Option<bool>,
    pub buffered_bytes: bool,
}

impl Field {
    pub fn attributes(&self) -> MainFlags {
        let set = |bit: u32| self.flags & (1u32 << bit) != 0;
        MainFlags {
            constant: set(0),
            variable: set(1),
            relative: set(2),
            wrap: set(3),
            nonlinear: set(4),
            no_preferred_state: set(5),
            null_state: set(6),
            volatile: (self.kind != ReportKind::Input).then(|| set(7)),
            buffered_bytes: set(8),
        }
    }
}
