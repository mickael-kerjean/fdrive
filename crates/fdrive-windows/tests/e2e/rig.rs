use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::sdk::Sdk;
use fdrive_testkit::FakeServer;
use fdrive_windows::adapter::Adapter;
use fdrive_windows::wire::{self, shell, watcher};

pub struct Rig {
    pub root: PathBuf,
    pub server: FakeServer,
    data: PathBuf,
    adapter: Arc<Adapter>,
    connection: Option<wire::Connection>,
    pump: tokio::task::JoinHandle<()>,
    id: String,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn start() -> Rig {
        Self::with(FakeServer::start())
    }

    pub fn with(server: FakeServer) -> Rig {
        static N: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir().join(format!(
            "fdrive-e2e-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let root = base.join("root");
        let data = base.join("data");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&data).unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let id = shell::sync_root_id("fdrive-e2e", "e2e@fake", &root).unwrap();
        shell::register(
            &root,
            &shell::Registration {
                id: id.clone(),
                display_name: "fdrive-e2e".to_string(),
                icon: shell::default_icon(),
                allow_pinning: true,
                provider_id: wire::PROVIDER_ID,
            },
        )
        .unwrap();
        let (adapter, connection, pump) = mount(&server, &rt, &root, &data);
        Rig {
            root,
            server,
            data,
            adapter,
            connection: Some(connection),
            pump,
            id,
            rt,
        }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn data(&self) -> &Path {
        &self.data
    }

    pub fn settle(&self) {
        std::thread::sleep(Duration::from_millis(500));
        self.rt
            .block_on(self.adapter.flush(Duration::from_secs(10)));
    }

    pub fn settle_for(&self, timeout: Duration) {
        std::thread::sleep(Duration::from_millis(500));
        self.rt.block_on(self.adapter.flush(timeout));
    }

    pub fn sweep(&self) {
        self.rt.block_on(self.adapter.recover()).unwrap();
    }

    pub fn restart(&mut self) {
        self.connection.take();
        self.pump.abort();
        let (adapter, connection, pump) = mount(&self.server, &self.rt, &self.root, &self.data);
        self.adapter = adapter;
        self.connection = Some(connection);
        self.pump = pump;
    }
}

fn mount(
    server: &FakeServer,
    rt: &tokio::runtime::Runtime,
    root: &Path,
    data: &Path,
) -> (Arc<Adapter>, wire::Connection, tokio::task::JoinHandle<()>) {
    let mut sdk = Sdk::new(server.url()).unwrap();
    sdk.set_token("TOKEN".into());
    let adapter =
        Adapter::new(Arc::new(sdk), rt.handle().clone(), root.to_path_buf(), data).unwrap();
    let connection = adapter.connect(root).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    watcher::spawn(root, tx).unwrap();
    let pump = {
        let adapter = adapter.clone();
        rt.spawn(async move {
            while let Some(path) = rx.recv().await {
                adapter.on_change(&path).await;
            }
        })
    };
    rt.block_on(adapter.recover()).unwrap();
    (adapter, connection, pump)
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.connection.take();
        self.pump.abort();
        let _ = shell::unregister(&self.id);
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}
