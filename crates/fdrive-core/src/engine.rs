mod cache;
mod conflict;
mod delta;
mod download;
mod facade;
mod gates;
mod ledger;
mod play;
mod scheduler;
mod state;
mod upload;
mod view;

pub use self::{download::Download, ledger::Ledger, scheduler::UploadStatus, state::LedgerGuard};
use self::{gates::Frozen, play::Outcome};
pub use crate::model::{Conflict, Fate, Observation, Operation, Plan, Resolution};

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::activity::Activity;
use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::Sdk;

use self::{conflict::Conflicts, gates::Transfers, state::State};

#[derive(Clone, Copy, PartialEq)]
pub enum Deletions {
    Authoritative,
    Inferred,
}

pub struct Engine<T: LocalStore> {
    local: T,
    sdk: Arc<Sdk>,
    ignore: crate::config::Ignore,
    deletions: Deletions,

    state: Mutex<State>,

    transfers: Transfers,
    frozen: Mutex<BTreeSet<RelPath>>,
    conflicts: Conflicts,

    scheduler: scheduler::Handle,
    rt: tokio::runtime::Handle,
    activity: Arc<Activity>,
}

#[cfg(test)]
mod tests;
