use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use fdrive_core::sdk::{normalize_server, Sdk};
use fdrive_linux::adapter::Adapter;
use fdrive_linux::wire::MountFs;
use fuser::{Config, MountOption, SessionACL};

const MOUNTS: &str = "/mnt/volumes";
const DATA: &str = "/var/lib/fdrive";
const STATE: &str = "/var/lib/fdrive/volumes.json";

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Volume {
    server: String,
    token: String,
    #[serde(skip)]
    users: HashSet<String>,
    #[serde(skip)]
    session: Option<Session>,
}

struct Session {
    adapter: Arc<Adapter>,
    fuse: fuser::BackgroundSession,
    _watch: fdrive_core::engine::Watch,
}

pub struct Driver {
    rt: tokio::runtime::Handle,
    volumes: Mutex<BTreeMap<String, Volume>>,
}

impl Driver {
    pub fn new(rt: tokio::runtime::Handle) -> std::io::Result<Self> {
        std::fs::create_dir_all(DATA)?;
        let volumes = std::fs::read(STATE)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Ok(Self { rt, volumes: Mutex::new(volumes) })
    }

    pub fn create(&self, name: &str, mut opts: HashMap<String, String>) -> Result<(), String> {
        let (Some(server), Some(token)) = (opts.remove("server"), opts.remove("token")) else {
            return Err("driver_opts needs server and token".into());
        };
        let mut volumes = self.lock();
        volumes.entry(name.to_owned()).or_insert_with(|| Volume {
            server: normalize_server(&server),
            token,
            ..Default::default()
        });
        save(&volumes)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        let mut volumes = self.lock();
        if volumes.get(name).is_some_and(|volume| volume.session.is_some()) {
            return Err(format!("volume {name} is in use"));
        }
        volumes.remove(name);
        let _ = std::fs::remove_dir_all(Path::new(DATA).join(name));
        save(&volumes)
    }

    pub fn mount(&self, name: &str, id: &str) -> Result<PathBuf, String> {
        let mut volumes = self.lock();
        let volume = volumes.get_mut(name).ok_or(format!("no such volume: {name}"))?;
        if volume.session.is_none() {
            let _guard = self.rt.enter();
            let session = self.connect(name, volume).map_err(|err| format!("mount {name}: {err}"))?;
            volume.session = Some(session);
        }
        volume.users.insert(id.to_owned());
        Ok(mountpoint(name))
    }

    pub fn unmount(&self, name: &str, id: &str) -> Result<(), String> {
        let mut volumes = self.lock();
        let volume = volumes.get_mut(name).ok_or(format!("no such volume: {name}"))?;
        volume.users.remove(id);
        if let Some(session) = volume.session.take_if(|_| volume.users.is_empty()) {
            self.disconnect(session);
        }
        Ok(())
    }

    pub fn mountpoint(&self, name: &str) -> Result<Option<PathBuf>, String> {
        let volumes = self.lock();
        let volume = volumes.get(name).ok_or(format!("no such volume: {name}"))?;
        Ok(volume.session.as_ref().map(|_| mountpoint(name)))
    }

    pub fn names(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    pub fn shutdown(&self) {
        for volume in self.lock().values_mut() {
            if let Some(session) = volume.session.take() {
                self.disconnect(session);
            }
        }
    }

    fn connect(&self, name: &str, volume: &Volume) -> Result<Session, Box<dyn std::error::Error>> {
        let (target, data) = (mountpoint(name), Path::new(DATA).join(name));
        if std::fs::metadata(&target).is_err_and(|err| err.raw_os_error() == Some(libc::ENOTCONN)) {
            let _ = std::process::Command::new("fusermount3").arg("-uz").arg(&target).status();
        }
        std::fs::create_dir_all(&target)?;
        std::fs::create_dir_all(&data)?;
        let sdk = Sdk::builder(&volume.server).token(volume.token.clone())?;
        let adapter = Arc::new(Adapter::new(self.rt.clone(), Arc::new(sdk), &data)?);
        let mut config = Config::default();
        config.mount_options = vec![MountOption::FSName("filestash".into()), MountOption::DefaultPermissions];
        config.acl = SessionACL::All;
        let filesystem = MountFs::new(adapter.clone(), self.rt.clone());
        let fuse = fuser::spawn_mount2(filesystem.clone(), &target, &config)?;
        log::info!("mounted {name}");
        Ok(Session { _watch: filesystem.watch(fuse.notifier()), adapter, fuse })
    }

    fn disconnect(&self, session: Session) {
        drop(session._watch);
        if let Err(err) = session.fuse.umount_and_join() {
            log::warn!("unmount: {err}");
        }
        self.rt.block_on(session.adapter.system().flush(Duration::from_secs(30)));
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, Volume>> {
        self.volumes.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn mountpoint(name: &str) -> PathBuf {
    Path::new(MOUNTS).join(name)
}

fn save(volumes: &BTreeMap<String, Volume>) -> Result<(), String> {
    let bytes = serde_json::to_vec(volumes).map_err(|err| err.to_string())?;
    fdrive_core::write_atomic(Path::new(STATE), &bytes).map_err(|err| format!("save: {err}"))
}
