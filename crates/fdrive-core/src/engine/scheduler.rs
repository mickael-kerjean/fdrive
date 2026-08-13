use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Weak;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::download::DownloadStatus;
use super::state::Step;
use super::{Engine, Outcome};
use crate::model::Observation;
use crate::path::RelPath;
use crate::port::LocalStore;

const CONCURRENCY: usize = 4;
const STALL: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStatus {
    Idle,
    Busy,
    Error,
}

enum Msg {
    Kick,
    Flush(oneshot::Sender<()>),
    Sweep,
    Stream(
        RelPath,
        PathBuf,
        watch::Sender<(u64, DownloadStatus)>,
        Observation,
    ),
}

pub(super) struct Handle {
    queue: mpsc::UnboundedSender<Msg>,
    status: watch::Receiver<UploadStatus>,
}

impl Handle {
    pub(super) fn kick(&self) {
        let _ = self.queue.send(Msg::Kick);
    }

    pub(super) async fn flush(&self, timeout: Duration) {
        let (reply, done) = oneshot::channel();
        if self.queue.send(Msg::Flush(reply)).is_ok() {
            let _ = tokio::time::timeout(timeout, done).await;
        }
    }

    pub(super) fn status(&self) -> watch::Receiver<UploadStatus> {
        self.status.clone()
    }

    pub(super) fn sweep(&self) {
        let _ = self.queue.send(Msg::Sweep);
    }

    pub(super) fn stream(
        &self,
        path: RelPath,
        tmp: PathBuf,
        tx: watch::Sender<(u64, DownloadStatus)>,
        current: Observation,
    ) {
        let _ = self.queue.send(Msg::Stream(path, tmp, tx, current));
    }
}

pub(super) struct Driver {
    rx: mpsc::UnboundedReceiver<Msg>,
    status: watch::Sender<UploadStatus>,
}

impl Driver {
    pub(super) fn spawn<T: LocalStore>(self, rt: &tokio::runtime::Handle, engine: Weak<Engine<T>>) {
        rt.spawn(run(engine, self.rx, self.status));
    }
}

pub(super) fn prepare() -> (Handle, Driver) {
    let (queue, rx) = mpsc::unbounded_channel();
    let (status_tx, status) = watch::channel(UploadStatus::Idle);
    (
        Handle { queue, status },
        Driver {
            rx,
            status: status_tx,
        },
    )
}

async fn run<T: LocalStore>(
    engine: Weak<Engine<T>>,
    mut rx: mpsc::UnboundedReceiver<Msg>,
    status: watch::Sender<UploadStatus>,
) {
    let mut running: JoinSet<(i64, Outcome)> = JoinSet::new();
    let mut spawned: HashMap<tokio::task::Id, i64> = HashMap::new();
    let mut flushes: Vec<oneshot::Sender<()>> = Vec::new();
    let mut failing = false;
    let mut rushed = false;
    let mut last_progress = Instant::now();
    let mut last_stall_log = Instant::now();
    loop {
        let wake = {
            let Some(engine) = engine.upgrade() else {
                return;
            };
            let step = engine.step(CONCURRENCY - running.len(), std::mem::take(&mut rushed));
            for (seq, plan) in step.plans {
                let engine = engine.clone();
                let handle = running.spawn(async move {
                    let result = engine.replay(&plan).await;
                    (seq, result)
                });
                spawned.insert(handle.id(), seq);
            }
            let idle = step.idle && running.is_empty();
            if idle {
                for reply in flushes.drain(..) {
                    let _ = reply.send(());
                }
                last_progress = Instant::now();
            } else if engine.pending() == 0 {
                last_progress = Instant::now();
            } else if last_progress.elapsed() >= STALL && last_stall_log.elapsed() >= STALL {
                last_stall_log = Instant::now();
                log::error!("sync stalled: {}", engine.stall_report());
            }
            let next = match (failing, idle) {
                (true, _) => UploadStatus::Error,
                (false, true) => UploadStatus::Idle,
                (false, false) => UploadStatus::Busy,
            };
            if *status.borrow() != next {
                let _ = status.send(next);
            }
            step.wake
        };
        tokio::select! {
            msg = rx.recv() => match msg {
                None => break,
                Some(Msg::Kick) => {}
                Some(Msg::Flush(reply)) => {
                    if let Some(engine) = engine.upgrade() {
                        engine.rush();
                    }
                    rushed = true;
                    flushes.push(reply);
                }
                Some(Msg::Sweep) => {
                    if let Some(engine) = engine.upgrade() {
                        tokio::spawn(engine.sweep_pins());
                    }
                }
                Some(Msg::Stream(path, tmp, tx, current)) => {
                    if let Some(engine) = engine.upgrade() {
                        tokio::spawn(engine.stream(path, tmp, tx, current));
                    }
                }
            },
            Some(joined) = running.join_next_with_id(), if !running.is_empty() => {
                let (seq, outcome) = match joined {
                    Ok((id, (seq, outcome))) => {
                        spawned.remove(&id);
                        (seq, outcome)
                    }
                    Err(err) => {
                        let Some(seq) = spawned.remove(&err.id()) else {
                            continue;
                        };
                        (seq, Outcome::Failed(io::Error::other("replay panicked")))
                    }
                };
                if !matches!(outcome, Outcome::Busy | Outcome::Failed(_)) {
                    last_progress = Instant::now();
                }
                if let Some(engine) = engine.upgrade() {
                    failing = engine.settle(seq, outcome);
                }
            },
            _ = tokio::time::sleep_until(wake.map(Instant::from_std).unwrap_or_else(Instant::now)), if wake.is_some() => {}
        }
    }
}

impl<T: LocalStore> Engine<T> {
    pub async fn flush(&self, timeout: Duration) {
        self.scheduler.flush(timeout).await;
    }

    pub fn upload_status(&self) -> watch::Receiver<UploadStatus> {
        self.scheduler.status()
    }

    pub(super) fn kick(&self) {
        self.scheduler.kick();
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
}
