use std::sync::Arc;

use fdrive_core::engine::UploadStatus;
use tokio::sync::watch;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct Status<'a>(pub(super) &'a Arc<Adapter>);

impl Status<'_> {
    pub fn watch(self) -> watch::Receiver<UploadStatus> {
        self.0.engine.upload_status()
    }

    pub fn activity(self) -> Arc<fdrive_core::activity::Activity> {
        self.0.engine.activity()
    }
}
