use std::io;
use std::time::Duration;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct System<'a>(pub(super) &'a Adapter);

impl System<'_> {
    pub async fn flush(self, timeout: Duration) {
        self.0.engine.flush(timeout).await;
    }

    pub fn vacuum(self) -> io::Result<()> {
        self.0.engine.local().meta.lock().unwrap().clear();
        self.0.prune()
    }

    pub async fn logout(self) {
        let _ = self.0.engine.sdk().logout().await;
    }
}
