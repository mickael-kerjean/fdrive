use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::sync::watch;

use crate::model::Operation;
use crate::path::RelPath;
use crate::port::LocalStore;
use crate::sdk::Sdk;

use super::gates::Transfers;
use super::state::{LedgerGuard, State, Step};
use super::{scheduler, Engine, Frozen, Outcome, UploadStatus};

impl<T: LocalStore> Engine<T> {
    pub fn start(rt: tokio::runtime::Handle, sdk: Arc<Sdk>, local: T) -> Arc<Self> {
        let ledger_file = local.ledger();
        let ignore = crate::config::ignore(ledger_file.parent().unwrap_or(Path::new("")));
        let state = State::open(&ledger_file);
        let (scheduler, driver) = scheduler::prepare();
        let engine = Arc::new(Self {
            local,
            sdk,
            ignore,
            state: Mutex::new(state),
            transfers: Transfers::default(),
            frozen: Mutex::new(BTreeSet::new()),
            scheduler,
            rt: rt.clone(),
            activity: Arc::default(),
        });
        driver.spawn(&rt, Arc::downgrade(&engine));
        engine
    }

    pub fn activity(&self) -> Arc<crate::activity::Activity> {
        self.activity.clone()
    }

    pub async fn flush(&self, timeout: Duration) {
        self.scheduler.flush(timeout).await;
    }

    pub fn upload_status(&self) -> watch::Receiver<UploadStatus> {
        self.scheduler.status()
    }

    pub fn recover(&self) {
        self.kick();
        self.pin_sweep();
    }

    pub(super) fn step(&self, slots: usize, force: bool) -> Step {
        self.state().step(slots, force, &self.ignore, |p| {
            fs::metadata(self.local.backing(p)).is_ok_and(|md| md.len() == 0)
        })
    }

    pub(super) fn settle(&self, seq: i64, outcome: Outcome) -> bool {
        let (failing, conflict) = self.state().settle(seq, outcome);
        if let Some(c) = conflict {
            log::warn!("conflict on {}", c.op);
        }
        failing
    }

    pub(super) fn rush(&self) {
        self.state().rush();
    }

    pub(super) fn pending(&self) -> usize {
        self.state().pending()
    }

    pub(super) fn stall_report(&self) -> String {
        let state = self.state();
        let mut sample = state.pending_sample(5);
        let total = state.pending();
        if total > sample.len() {
            sample.push(format!("... {} more", total - sample.len()));
        }
        format!(
            "{total} pending plans, none retired: [{}]",
            sample.join(", ")
        )
    }

    pub(super) fn record(&self, op: Operation) {
        self.state().record(op);
        self.kick();
    }

    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    pub(super) fn kick(&self) {
        self.scheduler.kick();
    }

    pub(super) fn freeze(&self, paths: &[&RelPath]) -> Frozen<'_> {
        let paths: Vec<RelPath> = paths.iter().map(|p| (*p).clone()).collect();
        let mut set = self.frozen.lock().unwrap();
        for path in &paths {
            set.insert(path.clone());
        }
        Frozen {
            set: &self.frozen,
            paths,
        }
    }

    pub(super) fn is_frozen(&self, path: &RelPath) -> bool {
        self.frozen
            .lock()
            .unwrap()
            .iter()
            .any(|p| path == p || path.is_descendant_of(p))
    }

    pub(super) async fn wait_uploads(&self, path: &RelPath, subtree: bool) {
        let gates: Vec<Arc<tokio::sync::Mutex<()>>> = self
            .transfers
            .uploading
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| *p == path || (subtree && p.is_descendant_of(path)))
            .map(|(_, gate)| gate.clone())
            .collect();
        for gate in gates {
            let _gate = gate.lock().await;
        }
    }

    pub fn block_on<F: std::future::Future>(&self, fut: F) -> F::Output {
        self.rt.block_on(fut)
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.rt.spawn(fut);
    }

    pub fn local(&self) -> &T {
        &self.local
    }

    pub fn ledger(&self) -> LedgerGuard<'_> {
        LedgerGuard(self.state())
    }
}
