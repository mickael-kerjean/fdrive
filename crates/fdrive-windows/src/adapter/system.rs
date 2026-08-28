use std::io;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::path::RelPath;

use crate::wire;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct System<'a>(pub(super) &'a Arc<Adapter>);

impl System<'_> {
    pub async fn flush(self, timeout: Duration) {
        self.0.engine.system().flush(timeout).await;
    }

    pub fn connect(self, root: &Path) -> io::Result<wire::Connection> {
        let fetch = self.0.clone();
        let populate = self.0.clone();
        let delete = self.0.clone();
        let rename = self.0.clone();
        wire::connect(
            root,
            wire::Callbacks {
                fetch: Box::new(move |path, expected, sink| {
                    let _busy = fetch.working();
                    fetch.cache().fetch(path, expected, sink)
                }),
                populate: Box::new(move |dir| {
                    let _busy = populate.working();
                    populate.fs().populate(dir)
                }),
                delete: Box::new(move |path, is_dir| delete.fs().on_delete(path, is_dir)),
                rename: Box::new(move |from, to, is_dir| rename.fs().on_rename(from, to, is_dir)),
            },
        )
    }

    pub async fn recover(self) -> io::Result<()> {
        self.0.engine.system().recover();
        let adapter = self.0.clone();
        for path in tokio::task::spawn_blocking(move || adapter.reconcile().sweep()).await? {
            log::info!("recovered pending upload: {path}");
            self.0.engine.fs().released(&path);
        }
        Ok(())
    }

    pub fn vacuum(self) -> io::Result<()> {
        let root = RelPath::root();
        let emptied = self
            .0
            .engine
            .local()
            .suppress(&root, || self.0.reconcile().vacuum(&root))?;
        if !emptied {
            log::warn!("vacuum: {} not emptied", self.0.root.display());
        }
        Ok(())
    }
}
