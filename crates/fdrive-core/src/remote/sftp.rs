use std::io::SeekFrom;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use russh::client::{self, Handle};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::Disconnect;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::io::{ReaderStream, StreamReader};
use url::Url;

use super::{Error, FileInfo, FileType, Permissions, Result};
use crate::ByteStream;

const TMP_SUFFIX: &str = ".fdrive.tmp";
const KEYS: [&str; 3] = ["id_ed25519", "id_ecdsa", "id_rsa"];

pub struct Sftp {
    host: String,
    port: u16,
    user: String,
    root: String,
    ssh: Option<PathBuf>,
    insecure: bool,
    connection: Mutex<Option<Connection>>,
}

impl Sftp {
    pub(crate) fn new(url: &str, insecure: bool) -> Result<Self> {
        let url = Url::parse(url)?;
        let host = url.host_str().ok_or_else(|| Error::Api("missing host".into()))?;
        #[cfg(unix)]
        let home = unsafe { libc::getpwuid(libc::getuid()).as_ref() }.map(|user| {
            let dir = unsafe { std::ffi::CStr::from_ptr(user.pw_dir) };
            PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()))
        });
        #[cfg(not(unix))]
        let home = std::env::home_dir();
        let root = url
            .path()
            .trim_start_matches('/')
            .trim_start_matches("~/")
            .trim_end_matches('/');
        Ok(Self {
            host: host.to_owned(),
            port: url.port().unwrap_or(22),
            user: url.username().to_owned(),
            root: root.to_owned(),
            ssh: home.map(|home| home.join(".ssh")),
            insecure,
            connection: Mutex::new(None),
        })
    }

    pub fn insecure(&self) -> bool {
        self.insecure
    }

    pub async fn probe(&self) -> Result<String> {
        self.session().await?;
        Ok("sftp".into())
    }

    pub async fn logout(&self) -> Result<()> {
        let Some(connection) = self.connection.lock().await.take() else {
            return Ok(());
        };
        let _ = connection.sftp.close().await;
        Ok(connection.ssh.disconnect(Disconnect::ByApplication, "", "en").await?)
    }

    pub async fn ls(&self, path: &str) -> Result<Vec<FileInfo>> {
        let sftp = self.session().await?;
        let mut entries = Vec::new();
        for entry in sftp.read_dir(self.remote(path)).await? {
            let name = entry.file_name();
            if name == "." || name == ".." || name.ends_with(TMP_SUFFIX) {
                continue;
            }
            let mut attrs = entry.metadata();
            if attrs.is_symlink() {
                let child = format!("{}/{name}", path.trim_end_matches('/'));
                let Ok(target) = sftp.metadata(self.remote(&child)).await else {
                    continue;
                };
                attrs = target;
            }
            entries.push(info(&name, &attrs));
        }
        Ok(entries)
    }

    pub async fn stat(&self, path: &str) -> Result<FileInfo> {
        let attrs = self.session().await?.metadata(self.remote(path)).await?;
        Ok(info(path, &attrs))
    }

    pub async fn cat(&self, path: &str) -> Result<(FileInfo, ByteStream)> {
        let sftp = self.session().await?;
        let remote = self.remote(path);
        let attrs = sftp.metadata(remote.clone()).await?;
        let file = sftp.open(remote).await?;
        Ok((info(path, &attrs), Box::pin(ReaderStream::new(file))))
    }

    pub async fn cat_range(&self, path: &str, start: u64, end: u64) -> Result<Vec<u8>> {
        let mut file = self.session().await?.open(self.remote(path)).await?;
        file.seek(SeekFrom::Start(start)).await?;
        let mut data = Vec::new();
        file.take(end.saturating_sub(start) + 1).read_to_end(&mut data).await?;
        Ok(data)
    }

    pub async fn save(&self, path: &str, body: ByteStream, since: Option<SystemTime>) -> Result<Option<SystemTime>> {
        let sftp = self.session().await?;
        let remote = self.remote(path);
        if let Some(since) = since {
            let since = since.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            match sftp.metadata(remote.clone()).await.map_err(Error::from) {
                Ok(attrs) if attrs.mtime.is_some_and(|mtime| u64::from(mtime) > since) => return Err(Error::PreconditionFailed),
                Ok(_) | Err(Error::NotFound) => {}
                Err(err) => return Err(err),
            }
        }
        let tmp = format!("{remote}{TMP_SUFFIX}");
        let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE;
        let mut file = sftp.open_with_flags(tmp.clone(), flags).await?;
        let copied = tokio::io::copy(&mut StreamReader::new(body), &mut file).await;
        if let Err(err) = copied.and(file.shutdown().await) {
            let _ = sftp.remove_file(tmp).await;
            return Err(err.into());
        }
        if sftp.rename(tmp.clone(), remote.clone()).await.is_err() {
            let _ = sftp.remove_file(remote.clone()).await;
            sftp.rename(tmp, remote.clone()).await?;
        }
        Ok(sftp.metadata(remote).await?.modified().ok())
    }

    pub async fn mkdir(&self, path: &str) -> Result<()> {
        Ok(self.session().await?.create_dir(self.remote(path)).await?)
    }

    pub async fn rm(&self, path: &str) -> Result<()> {
        let sftp = self.session().await?;
        if !path.ends_with('/') {
            return Ok(sftp.remove_file(self.remote(path)).await?);
        }
        let mut stack = vec![(path.to_owned(), false)];
        while let Some((dir, emptied)) = stack.pop() {
            if emptied {
                sftp.remove_dir(self.remote(&dir)).await?;
                continue;
            }
            let entries = sftp.read_dir(self.remote(&dir)).await?;
            stack.push((dir.clone(), true));
            for entry in entries {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                match entry.metadata().is_dir() {
                    true => stack.push((format!("{dir}{name}/"), false)),
                    false => sftp.remove_file(self.remote(&format!("{dir}{name}"))).await?,
                }
            }
        }
        Ok(())
    }

    pub async fn mv(&self, from: &str, to: &str) -> Result<()> {
        Ok(self.session().await?.rename(self.remote(from), self.remote(to)).await?)
    }

    async fn session(&self) -> Result<Arc<SftpSession>> {
        let mut connection = self.connection.lock().await;
        if let Some(open) = connection.as_ref().filter(|open| !open.ssh.is_closed()) {
            return Ok(open.sftp.clone());
        }
        let config = client::Config {
            inactivity_timeout: Some(Duration::from_secs(300)),
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        };
        let verifier = Verifier {
            host: self.host.clone(),
            port: self.port,
            insecure: self.insecure,
            known_hosts: self.ssh.as_ref().map(|dir| dir.join("known_hosts")),
        };
        let mut ssh = client::connect(Arc::new(config), (self.host.as_str(), self.port), verifier).await?;
        let mut authenticated = false;
        for path in self.ssh.iter().flat_map(|dir| KEYS.map(|name| dir.join(name))) {
            let Ok(key) = russh::keys::load_secret_key(path, None) else {
                continue;
            };
            let hash = ssh.best_supported_rsa_hash().await?.flatten();
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
            if ssh.authenticate_publickey(&self.user, key).await?.success() {
                authenticated = true;
                break;
            }
        }
        if !authenticated {
            return Err(Error::NotAuthenticated);
        }
        let channel = ssh.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = Arc::new(SftpSession::new(channel.into_stream()).await?);
        *connection = Some(Connection { ssh, sftp: sftp.clone() });
        Ok(sftp)
    }

    fn remote(&self, path: &str) -> String {
        let path = path.trim_matches('/');
        let joined = [self.root.as_str(), path]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        match joined.is_empty() {
            true => ".".into(),
            false => joined,
        }
    }
}

struct Connection {
    ssh: Handle<Verifier>,
    sftp: Arc<SftpSession>,
}

struct Verifier {
    host: String,
    port: u16,
    insecure: bool,
    known_hosts: Option<PathBuf>,
}

impl client::Handler for Verifier {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> std::result::Result<bool, Self::Error> {
        Ok(match key {
            _ if self.insecure => true,
            PublicKeyOrCertificate::PublicKey { key, .. } => self
                .known_hosts
                .iter()
                .any(|path| russh::keys::check_known_hosts_path(&self.host, self.port, key, path).unwrap_or(false)),
            PublicKeyOrCertificate::Certificate(_) => false,
        })
    }
}

fn info(path: &str, attrs: &FileAttributes) -> FileInfo {
    let dir = attrs.is_dir();
    FileInfo {
        name: path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_owned(),
        kind: if dir { FileType::Directory } else { FileType::File },
        size: (!dir).then(|| attrs.len()),
        mtime: attrs.modified().ok(),
        perms: Permissions::default(),
    }
}

impl From<russh::Error> for Error {
    fn from(err: russh::Error) -> Self {
        Error::Api(format!("ssh: {err}"))
    }
}

impl From<SftpError> for Error {
    fn from(err: SftpError) -> Self {
        match err {
            SftpError::Status(status) => match status.status_code {
                StatusCode::NoSuchFile => Error::NotFound,
                StatusCode::PermissionDenied => Error::PermissionDenied,
                _ => Error::Api(format!("sftp: {}", status.error_message)),
            },
            err => Error::Api(format!("sftp: {err}")),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound,
            std::io::ErrorKind::PermissionDenied => Error::PermissionDenied,
            _ => Error::Api(err.to_string()),
        }
    }
}
