use osg_mapping::script::{Invocation, Ticket, Transaction, IPC_LIMIT};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Request {
    Compile { source: String },
    Invoke { request: Invocation },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Reply {
    Compiled,
    Completed {
        ticket: Ticket,
        result: Result<Transaction, String>,
    },
    Error {
        message: String,
    },
}
pub fn read<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> Result<T, String> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|e| e.to_string())?;
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > IPC_LIMIT {
        return Err("Script IPC size rejected.".into());
    }
    let mut bytes = vec![0; size];
    reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub fn write(value: &impl Serialize, writer: &mut impl Write) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > IPC_LIMIT {
        return Err("Script IPC size rejected.".into());
    }
    writer
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .and_then(|_| writer.write_all(&bytes))
        .and_then(|_| writer.flush())
        .map_err(|e| e.to_string())
}
