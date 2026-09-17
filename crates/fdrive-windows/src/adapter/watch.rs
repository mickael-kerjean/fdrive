use std::collections::BTreeSet;
use std::sync::Arc;

use fdrive_core::engine::{RemoteChanges, Watch};
use fdrive_core::path::RelPath;

use super::Adapter;

pub struct RemoteWatch {
    _stream: Watch,
    worker: tokio::task::JoinHandle<()>,
}

impl Drop for RemoteWatch {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

impl Adapter {
    pub fn watch(self: &Arc<Self>) -> RemoteWatch {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RemoteChanges>();
        let stream = self.engine.watch(move |changes| {
            let _ = tx.send(changes);
        });
        let adapter = Arc::downgrade(self);
        let worker = tokio::spawn(async move {
            while let Some(changes) = rx.recv().await {
                let Some(adapter) = adapter.upgrade() else { break };
                let mut targets = BTreeSet::new();
                adapter.remote_targets(changes, &mut targets);
                while let Ok(changes) = rx.try_recv() {
                    adapter.remote_targets(changes, &mut targets);
                }
                for directory in targets {
                    if let Err(error) = adapter.fs().refresh_remote(&directory).await {
                        log::warn!("watch refresh {directory}: {error}");
                    }
                }
            }
        });
        RemoteWatch { _stream: stream, worker }
    }

    fn remote_targets(&self, changes: RemoteChanges, targets: &mut BTreeSet<RelPath>) {
        targets.extend(changes.directories().cloned());
        targets.extend(
            self.populated
                .lock()
                .unwrap()
                .iter()
                .filter(|directory| changes.affects(directory))
                .cloned(),
        );
    }
}
