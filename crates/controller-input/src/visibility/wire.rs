//! Released HidHide persistent IOCTL contract, isolated from Windows transport.
use super::{
    journal::Backend,
    policy::{State, LIMIT},
};
pub trait Io {
    fn call(&mut self, function: u32, input: Option<&[u8]>) -> Result<Vec<u8>, String>;
}
pub struct Driver<T: Io>(pub T);
fn list(data: &[u8]) -> Result<Vec<String>, String> {
    if data.len() < 2 || data.len() > LIMIT || data.len() % 2 != 0 {
        return Err("Invalid HidHide multi-string length.".into());
    }
    let words: Vec<_> = data
        .chunks_exact(2)
        .map(|p| u16::from_le_bytes([p[0], p[1]]))
        .collect();
    if words.last() != Some(&0) {
        return Err("Unterminated HidHide multi-string.".into());
    }
    let mut values = vec![];
    let mut end = false;
    for part in words.split(|v| *v == 0) {
        if part.is_empty() {
            end = true;
            continue;
        }
        if end {
            return Err("Trailing data in HidHide multi-string.".into());
        }
        values.push(String::from_utf16(part).map_err(|_| "Invalid HidHide UTF-16.")?);
    }
    if !values.is_empty() && (words.len() < 2 || words[words.len() - 2] != 0) {
        return Err("Missing multi-string terminator.".into());
    }
    Ok(values)
}
fn encode(values: &[String]) -> Vec<u8> {
    let mut words = vec![];
    for value in values {
        words.extend(value.encode_utf16());
        words.push(0);
    }
    if words.is_empty() {
        words.push(0);
    }
    words.push(0);
    words.into_iter().flat_map(u16::to_le_bytes).collect()
}
impl<T: Io> Driver<T> {
    fn boolean(&mut self, function: u32) -> Result<bool, String> {
        match self.0.call(function, None)?.as_slice() {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err("Invalid HidHide boolean.".into()),
        }
    }
    fn snapshot(&mut self) -> Result<State, String> {
        let state = State {
            applications: list(&self.0.call(2048, None)?)?,
            hidden: list(&self.0.call(2050, None)?)?,
            active: self.boolean(2052)?,
            inverse: self.boolean(2054)?,
        };
        state.validate()?;
        Ok(state)
    }
}
impl<T: Io> Backend for Driver<T> {
    fn read(&mut self) -> Result<State, String> {
        let first = self.snapshot()?;
        if self.snapshot()? != first {
            return Err("HidHide changed while inspecting its global policy.".into());
        }
        Ok(first)
    }
    fn change(&mut self, expected: &State, desired: &State) -> Result<(), String> {
        expected.validate()?;
        desired.validate()?;
        let mut changes = vec![];
        if expected.applications != desired.applications {
            changes.push((2049, encode(&desired.applications)));
        }
        if expected.hidden != desired.hidden {
            changes.push((2051, encode(&desired.hidden)));
        }
        if expected.active != desired.active {
            changes.push((2053, vec![u8::from(desired.active)]));
        }
        if expected.inverse != desired.inverse {
            changes.push((2055, vec![u8::from(desired.inverse)]));
        }
        if changes.len() != 1 {
            return Err("Visibility writes must change exactly one journaled property.".into());
        }
        if self.read()? != *expected {
            return Err("External HidHide policy changed before mutation.".into());
        }
        let (function, data) = changes.pop().unwrap();
        if !self.0.call(function, Some(&data))?.is_empty() {
            return Err("Unexpected HidHide write response.".into());
        }
        if self.read()? != *desired {
            return Err("HidHide write readback failed.".into());
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
