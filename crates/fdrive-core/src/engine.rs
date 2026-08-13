mod cache;
mod download;
mod facade;
mod ledger;
mod scheduler;
mod view;

#[path = "engine/internal/delta.rs"]
mod delta;
#[path = "engine/internal/gates.rs"]
mod gates;
#[path = "engine/internal/play.rs"]
mod play;
#[path = "engine/internal/state.rs"]
mod state;
#[path = "engine/internal/upload.rs"]
mod upload;

pub use self::{download::Download, scheduler::UploadStatus, state::LedgerGuard};
use self::{gates::Frozen, ledger::Ledger, play::Outcome};
pub use crate::model::Observation;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::activity::Activity;
use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::Sdk;

use self::{gates::Transfers, state::State};

pub struct Engine<T: LocalStore> {
    local: T,
    sdk: Arc<Sdk>,
    ignore: crate::config::Ignore,

    state: Mutex<State>,

    transfers: Transfers,
    frozen: Mutex<BTreeSet<RelPath>>,

    scheduler: scheduler::Handle,
    rt: tokio::runtime::Handle,
    activity: Arc<Activity>,
}

#[cfg(test)]
mod tests;
