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
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::{Mutation, Sdk};
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

#[derive(Default)]
pub struct RemoteChanges {
    directories: BTreeSet<RelPath>,
    paths: BTreeSet<RelPath>,
}

impl RemoteChanges {
    pub fn directories(&self) -> impl Iterator<Item = &RelPath> {
        self.directories.iter()
    }

    pub fn affects(&self, directory: &RelPath) -> bool {
        self.directories.contains(directory)
            || self
                .paths
                .iter()
                .any(|path| path.is_root() || directory == path || directory.is_descendant_of(path))
    }

    fn add(&mut self, mutation: Mutation) {
        let valid = |path: &str| !path.contains('\0') && !path.split('/').any(|part| part == "..");
        if !valid(&mutation.path) {
            return;
        }
        let target = match mutation.operation.as_str() {
            "save" | "touch" | "mkdir" | "rm" => None,
            "mv" => match mutation.target.filter(|target| !target.is_empty() && valid(target)) {
                Some(target) => Some(target),
                None => return,
            },
            _ => return,
        };
        for path in std::iter::once(mutation.path).chain(target) {
            let path = RelPath::new(&path);
            self.directories.insert(path.parent_or_root());
            self.paths.insert(path);
        }
    }
}

pub struct Watch(JoinHandle<()>);

impl Drop for Watch {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<T: LocalStore> Engine<T> {
    pub fn watch(&self, notify: impl Fn(RemoteChanges) + Send + 'static) -> Watch {
        Watch(self.rt.spawn(watch(self.sdk.clone(), notify)))
    }
}

async fn watch(sdk: Arc<Sdk>, notify: impl Fn(RemoteChanges)) {
    let mut cursor: Option<String> = None;
    let mut backoff = Duration::from_secs(1);
    loop {
        let connected = Instant::now();
        let mut pending = RemoteChanges::default();
        let mut deadline = None;
        match sdk.watch(cursor.as_deref()).await {
            Ok(mut stream) => loop {
                tokio::select! {
                    event = stream.next() => match event {
                        Ok(Some(event)) => {
                            if let Some(id) = event.id {
                                cursor = (!id.is_empty()).then_some(id);
                            }
                            if let Some(mutation) = event.mutation {
                                pending.add(mutation);
                                if !pending.paths.is_empty() {
                                    deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(200));
                                }
                                if pending.paths.len() >= 256 {
                                    notify(std::mem::take(&mut pending));
                                    deadline = None;
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            log::debug!("watch disconnected: {error}");
                            break;
                        }
                    },
                    _ = tokio::time::sleep_until(deadline.unwrap_or_else(Instant::now)), if deadline.is_some() => {
                        notify(std::mem::take(&mut pending));
                        deadline = None;
                    }
                }
            },
            Err(error) => log::debug!("watch unavailable: {error}"),
        }
        if !pending.paths.is_empty() {
            notify(pending);
        }
        if connected.elapsed() >= Duration::from_secs(30) {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

#[cfg(test)]
mod tests;
