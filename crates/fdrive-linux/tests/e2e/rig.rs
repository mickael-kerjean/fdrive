use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use fdrive_core::sdk::Sdk;
use fdrive_linux::adapter::Adapter;
use fdrive_linux::wire::MountFs;
use fuser::{Config, MountOption};

use fdrive_testkit::FakeServer;

pub struct Rig {
    pub mnt: PathBuf,
    pub server: FakeServer,
    data: PathBuf,
    adapter: Arc<Adapter>,
    session: Option<fuser::BackgroundSession>,
    rt: tokio::runtime::Runtime,
}

impl Rig {
    pub fn start() -> Option<Rig> {
        Self::with(FakeServer::start())
    }

    pub fn with(server: FakeServer) -> Option<Rig> {
        Self::with_state(server, |_| {})
    }

    pub fn with_state(server: FakeServer, prepare: impl FnOnce(&Path)) -> Option<Rig> {
        if !Path::new("/dev/fuse").exists() {
            eprintln!("skipped: /dev/fuse is not available");
            return None;
        }
        static N: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "fdrive-e2e-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let mnt = root.join("mnt");
        let data = root.join("data");
        fs::create_dir_all(&mnt).unwrap();
        fs::create_dir_all(&data).unwrap();
        prepare(&data);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let (adapter, session) = mount(&server, &rt, &mnt, &data);
        Some(Rig {
            mnt,
            server,
            data,
            adapter,
            session: Some(session),
            rt,
        })
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.mnt.join(rel)
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

    pub fn restart(&mut self) {
        self.session.take();
        let (adapter, session) = mount(&self.server, &self.rt, &self.mnt, &self.data);
        self.adapter = adapter;
        self.session = Some(session);
    }

    pub fn wipe_client_state(&mut self) {
        self.session.take();
        fs::remove_dir_all(&self.data).unwrap();
        fs::create_dir_all(&self.data).unwrap();
        let (adapter, session) = mount(&self.server, &self.rt, &self.mnt, &self.data);
        self.adapter = adapter;
        self.session = Some(session);
    }
}

fn mount(
    server: &FakeServer,
    rt: &tokio::runtime::Runtime,
    mnt: &Path,
    data: &Path,
) -> (Arc<Adapter>, fuser::BackgroundSession) {
    let mut sdk = Sdk::new(server.url()).unwrap();
    sdk.set_token("TOKEN".into());
    let adapter = Arc::new(Adapter::new(Arc::new(sdk), rt.handle().clone(), data).unwrap());
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::FSName("fdrive-e2e".to_string()),
        MountOption::DefaultPermissions,
    ];
    let session = fuser::spawn_mount2(MountFs::new(adapter.clone()), mnt, &config).unwrap();
    (adapter, session)
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.session.take();
        let _ = fs::remove_dir_all(self.mnt.parent().unwrap());
    }
}
