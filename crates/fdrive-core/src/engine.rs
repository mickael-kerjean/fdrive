mod cache;
mod delta;
mod download;
mod fs;
mod gates;
mod ledger;
mod lifecycle;
mod play;
mod scheduler;
mod state;
mod status;
mod system;
mod upload;
mod view;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use self::{ledger::Ledger, play::Outcome};
pub use self::{
    cache::Cache, download::Reader, fs::Fs, scheduler::UploadStatus, state::LedgerGuard,
    status::Status, system::System, view::View,
};
pub use crate::model::Observation;

pub struct Engine<T: crate::port::LocalStore> {
    local: T,
    sdk: Arc<crate::sdk::Sdk>,
    ignore: crate::config::Ignore,
    state: Mutex<state::State>,
    transfers: gates::Transfers,
    frozen: Mutex<BTreeSet<crate::path::RelPath>>,
    scheduler: scheduler::Handle,
    rt: tokio::runtime::Handle,
    activity: Arc<crate::activity::Activity>,
}

impl<T: crate::port::LocalStore> Engine<T> {
    pub fn start(rt: tokio::runtime::Handle, sdk: Arc<crate::sdk::Sdk>, local: T) -> Arc<Self> {
        Self::spin_up(rt, sdk, local)
    }

    pub fn fs(&self) -> Fs<'_, T> {
        Fs(self)
    }

    pub fn view(&self) -> View<'_, T> {
        View(self)
    }

    pub fn cache(&self) -> Cache<'_, T> {
        Cache(self)
    }

    pub fn system(&self) -> System<'_, T> {
        System(self)
    }

    pub fn status(&self) -> Status<'_, T> {
        Status(self)
    }

    pub fn ledger(&self) -> LedgerGuard<'_> {
        LedgerGuard(self.state())
    }

    pub fn local(&self) -> &T {
        &self.local
    }
}

#[cfg(test)]
mod tests;
