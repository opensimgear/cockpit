use super::super::windows::ProcessScope;
use crate::game_discovery::ProcessIdentity;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) const ARGUMENT: &str = "--osg-process-events";
const MAX_FRAME: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub nonce: String,
    pub parent_pid: u32,
    pub parent_epoch: u64,
}
impl Request {
    pub(super) fn parse(args: &[String]) -> Result<Self, String> {
        if args.len() != 4 || args[0] != ARGUMENT || args.iter().any(|arg| arg.len() > 64) {
            return Err("Invalid process-events helper arguments.".into());
        }
        let parent_pid = args[2].parse::<u32>().map_err(|_| "Invalid parent PID.")?;
        let parent_epoch = args[3]
            .parse::<u64>()
            .map_err(|_| "Invalid parent creation time.")?;
        if !valid_nonce(&args[1])
            || parent_pid == 0
            || parent_epoch == 0
            || parent_pid.to_string() != args[2]
            || parent_epoch.to_string() != args[3]
        {
            return Err("Process-events helper arguments are not canonical.".into());
        }
        Ok(Self {
            nonce: args[1].clone(),
            parent_pid,
            parent_epoch,
        })
    }
    pub(super) fn arguments(&self) -> String {
        format!(
            "{ARGUMENT} {} {} {}",
            self.nonce, self.parent_pid, self.parent_epoch
        )
    }
    pub(super) fn pipe(&self) -> String {
        format!(r"\\.\pipe\osg-process-events-{}", self.nonce)
    }
}
fn valid_nonce(nonce: &str) -> bool {
    nonce.len() == 64
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub(super) enum Frame {
    Begin {
        scope: ProcessScope,
        nonce: String,
        pid: u32,
        epoch: u64,
        path: String,
    },
    Ready {
        nonce: String,
    },
    Failed {
        nonce: String,
        error: String,
    },
}
impl Frame {
    pub(super) fn begin(request: &Request, expected: &ProcessIdentity, scope: ProcessScope) -> Self {
        Self::Begin {
            scope,
            nonce: request.nonce.clone(),
            pid: expected.pid,
            epoch: expected.epoch,
            path: expected.path.clone(),
        }
    }
    pub(super) fn nonce(&self) -> &str {
        match self {
            Self::Begin { nonce, .. } | Self::Ready { nonce } | Self::Failed { nonce, .. } => nonce,
        }
    }
    pub(super) fn into_process(self) -> Result<(ProcessIdentity, ProcessScope), String> {
        let Self::Begin {
            pid, epoch, path, scope, ..
        } = self
        else {
            return Err("Expected a single Begin request.".into());
        };
        let expected = ProcessIdentity { pid, epoch, path };
        validate_process(&expected, scope)?;
        Ok((expected, scope))
    }
}
pub(super) fn validate_process(expected: &ProcessIdentity, scope: ProcessScope) -> Result<(), String> {
    if expected.pid == 0
        || expected.epoch == 0
        || expected.path.len() > 32768
        || expected.path.chars().any(char::is_control)
        || !std::path::Path::new(&expected.path).is_absolute()
        || !expected
            .path
            .rsplit(['\\', '/'])
            .next()
            .is_some_and(|name| scope.images().iter().any(|image| name.eq_ignore_ascii_case(image)))
    {
        return Err(
            "Choose one valid native process identity for this monitoring scope.".into(),
        );
    }
    Ok(())
}
pub(super) async fn read<R: AsyncRead + Unpin>(
    stream: &mut R,
    nonce: &str,
) -> Result<Frame, String> {
    let length = stream
        .read_u32_le()
        .await
        .map_err(|e| format!("Process-events pipe closed: {e}"))? as usize;
    if !(1..=MAX_FRAME).contains(&length) {
        return Err("Process-events frame exceeds its bounds.".into());
    }
    let mut bytes = vec![0; length];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|e| format!("Incomplete process-events frame: {e}"))?;
    let frame: Frame =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid process-events frame: {e}"))?;
    if frame.nonce() != nonce || !valid_nonce(frame.nonce()) {
        return Err("Process-events nonce does not match this request.".into());
    }
    if matches!(&frame, Frame::Failed { error, .. } if error.is_empty() || error.len() > 4096) {
        return Err("Invalid process-events failure message.".into());
    }
    Ok(frame)
}
pub(super) async fn write<W: AsyncWrite + Unpin>(
    stream: &mut W,
    frame: &Frame,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(frame).map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err("Process-events frame exceeds its bounds.".into());
    }
    stream
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(&bytes).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())
}
pub(super) fn failure(nonce: &str, error: String) -> Frame {
    // Bound UTF-8 by bytes without cutting through a character.
    let end = error
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= 4000)
        .last()
        .unwrap_or(0);
    let error = if error.len() > 4000 {
        error[..end].to_string()
    } else {
        error
    };
    Frame::Failed {
        nonce: nonce.into(),
        error: if error.is_empty() {
            "Native process monitoring failed.".into()
        } else {
            error
        },
    }
}
