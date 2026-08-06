use fdrive_core::activity as core;
use serde::Serialize;

use crate::Adapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferMode {
    Full,
    Delta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transfer {
    pub id: u64,
    pub path: String,
    pub direction: TransferDirection,
    pub mode: TransferMode,
    pub size: u64,
    pub wire: u64,
    pub progress: u64,
    pub state: TransferState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Sample {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivitySnapshot {
    pub version: u64,
    pub transfers: Vec<Transfer>,
    pub meter: Vec<Sample>,
}

#[uniffi::export]
impl Adapter {
    pub fn activity_json(&self) -> String {
        let snapshot = ActivitySnapshot::from(self.engine.activity().snapshot());
        serde_json::to_string(&snapshot).unwrap_or_default()
    }
}

impl From<core::Direction> for TransferDirection {
    fn from(direction: core::Direction) -> Self {
        match direction {
            core::Direction::Up => Self::Up,
            core::Direction::Down => Self::Down,
        }
    }
}

impl From<core::Mode> for TransferMode {
    fn from(mode: core::Mode) -> Self {
        match mode {
            core::Mode::Full => Self::Full,
            core::Mode::Delta => Self::Delta,
        }
    }
}

impl From<core::Transfer> for Transfer {
    fn from(transfer: core::Transfer) -> Self {
        let (state, error) = match transfer.outcome {
            core::Outcome::Running => (TransferState::Running, None),
            core::Outcome::Done => (TransferState::Done, None),
            core::Outcome::Failed(msg) => (TransferState::Failed, Some(msg)),
        };
        Self {
            id: transfer.id,
            path: transfer.path,
            direction: transfer.direction.into(),
            mode: transfer.mode.into(),
            size: transfer.size,
            wire: transfer.wire,
            progress: transfer.progress,
            state,
            error,
        }
    }
}

impl From<core::Snapshot> for ActivitySnapshot {
    fn from(snapshot: core::Snapshot) -> Self {
        Self {
            version: snapshot.version,
            transfers: snapshot.transfers.into_iter().map(Transfer::from).collect(),
            meter: snapshot.meter.into_iter().map(|(up, down)| Sample { up, down }).collect(),
        }
    }
}
