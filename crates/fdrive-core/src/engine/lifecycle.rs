use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::model::Operation;
use crate::port::LocalStore;
use crate::sdk::Sdk;

use super::gates::Transfers;
use super::state::State;
use super::{scheduler, Engine};

impl<T: LocalStore> Engine<T> {
    pub(super) fn spin_up(rt: tokio::runtime::Handle, sdk: Arc<Sdk>, local: T) -> Arc<Self> {
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

    pub(super) fn record(&self, op: Operation) {
        self.state().record(op);
        self.kick();
    }

    pub(super) fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
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
}
