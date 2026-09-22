use std::io;
use std::time::Duration;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct System<'a>(pub(super) &'a Adapter);

impl System<'_> {
    pub async fn flush(self, timeout: Duration) {
        if let Err(err) = self.0.deletes.lock().await.flush(&self.0.engine).await {
            log::error!("flush directory deletes: {err}");
        }
        self.0.engine.system().flush(timeout).await;
    }

    pub fn vacuum(self) -> io::Result<()> {
        self.0.engine.local().meta.lock().unwrap().clear();
        self.0.prune()
    }

    pub async fn logout(self) {
        let _ = self.0.engine.system().logout().await;
    }
}
