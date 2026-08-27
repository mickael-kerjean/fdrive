use std::io;

use fdrive_core::engine::Observation;
use fdrive_core::path::RelPath;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct Cache<'a>(pub(super) &'a Adapter);

impl Cache<'_> {
    pub fn hydrate(self, path: &RelPath) -> io::Result<()> {
        let current = self.remote(path);
        if current.is_some_and(|current| self.0.engine.view().current(path, current)) {
            return Ok(());
        }
        self.0.engine.block_on(self.0.engine.cache().hydrate(path, current, None))
    }

    pub fn prefetch(self, path: &RelPath) -> io::Result<()> {
        let current = self.remote(path);
        if current.is_some_and(|current| self.0.engine.view().current(path, current)) {
            return Ok(());
        }
        self.0.engine.block_on(self.0.engine.cache().prefetch(path, current, None))
    }

    fn remote(self, path: &RelPath) -> Option<Observation> {
        self.0.entry(path).ok().flatten().map(|e| Observation::of(&e))
    }
}
