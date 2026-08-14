use std::sync::Arc;

use tokio::sync::watch;

use crate::port::LocalStore;

use super::{Engine, UploadStatus};

pub struct Status<'a, T: LocalStore>(pub(super) &'a Engine<T>);

impl<'a, T: LocalStore> Status<'a, T> {
    pub fn watch(&self) -> watch::Receiver<UploadStatus> {
        self.0.scheduler.status()
    }

    pub fn activity(&self) -> Arc<crate::activity::Activity> {
        self.0.activity.clone()
    }
}
