use serde::Serialize;
use std::io::{self, Write};

pub(crate) enum EncodeError {
    TooLarge,
    Serialization(String),
}

pub(crate) fn encode_limited(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, EncodeError> {
    let mut writer = LimitedWriter {
        bytes: Vec::with_capacity(limit.min(4096)),
        limit,
    };
    serde_json::to_writer(&mut writer, value).map_err(|error| {
        if error.io_error_kind() == Some(io::ErrorKind::WriteZero) {
            EncodeError::TooLarge
        } else {
            EncodeError::Serialization(error.to_string())
        }
    })?;
    Ok(writer.bytes)
}

pub(crate) fn map_encode_error(
    error: EncodeError,
    too_large: crate::PublishError,
) -> crate::PublishError {
    match error {
        EncodeError::TooLarge => too_large,
        EncodeError::Serialization(error) => crate::PublishError::Serialization(error),
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for LimitedWriter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "encoded value exceeds limit",
            ));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
