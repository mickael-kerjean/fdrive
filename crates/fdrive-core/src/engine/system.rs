use std::time::Duration;

use crate::port::LocalStore;
use crate::sdk::Result;

use super::Engine;

pub struct System<'a, T: LocalStore>(pub(super) &'a Engine<T>);

impl<'a, T: LocalStore> System<'a, T> {
    pub async fn flush(&self, timeout: Duration) {
        self.0.scheduler.flush(timeout).await;
    }

    pub fn recover(&self) {
        self.0.kick();
        self.0.pin_sweep();
    }

    pub async fn logout(&self) -> Result<()> {
        self.0.sdk.logout().await
    }
}
