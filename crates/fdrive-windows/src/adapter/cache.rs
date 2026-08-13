use std::io;
use std::sync::Arc;
use std::time::SystemTime;

use fdrive_core::engine::Observation;
use fdrive_core::path::RelPath;
use futures_util::TryStreamExt;

use crate::wire;

use super::Adapter;

#[derive(Clone, Copy)]
pub struct Cache<'a>(pub(super) &'a Arc<Adapter>);

impl Cache<'_> {
    pub fn fetch(self, path: &RelPath, expected: i64, sink: wire::SinkFn) -> io::Result<u64> {
        const ALIGN: usize = 4096;
        const FLUSH_AT: usize = 1 << 20;
        let sdk = self.0.engine.sdk().clone();
        let api = path.as_file();
        let info = self.0.engine.rt().block_on(sdk.stat(&api))?;
        let size = info.size.unwrap_or(0);
        if size as i64 != expected {
            let mtime = info.mtime.unwrap_or_else(SystemTime::now);
            log::info!(
                "{path}: placeholder said {expected} bytes, server has {size}; failing this read and healing"
            );
            let this = self.0.clone();
            let what = path.clone();
            self.0.engine.rt().spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                let _ = tokio::task::spawn_blocking(move || {
                    if let Err(err) = this.reconcile().rebuild(&what, size, mtime) {
                        log::warn!("heal {what}: {err}; will retry on the next read");
                    }
                })
                .await;
            });
            return Err(io::Error::other(format!(
                "{path}: size changed on the server; healing the placeholder"
            )));
        }
        let activity = self.0.engine.activity();
        let act = activity.begin(
            &path.as_file(),
            fdrive_core::activity::Direction::Down,
            size,
        );
        let result = (|| {
            let mut sent: u64 = 0;
            let mut buf: Vec<u8> = Vec::with_capacity(FLUSH_AT + ALIGN);
            self.0.engine.rt().block_on(async {
                let (_, mut stream) = sdk.cat(&api).await?;
                while let Some(chunk) = stream.try_next().await? {
                    buf.extend_from_slice(&chunk);
                    if buf.len() >= FLUSH_AT {
                        let aligned = buf.len() & !(ALIGN - 1);
                        sink(sent, &buf[..aligned])?;
                        sent += aligned as u64;
                        activity.wire(act, aligned as u64);
                        activity.progress(act, sent);
                        buf.drain(..aligned);
                    }
                }
                Ok::<(), io::Error>(())
            })?;
            if !buf.is_empty() {
                sink(sent, &buf)?;
                sent += buf.len() as u64;
                activity.wire(act, buf.len() as u64);
                activity.progress(act, sent);
            }
            if sent != size {
                return Err(io::Error::other(format!(
                    "{path}: short download ({sent} of {size} bytes)"
                )));
            }
            self.0.engine.ledger().observe(path, Observation::of(&info));
            Ok(sent)
        })();
        activity.finish(
            act,
            result
                .as_ref()
                .map(|_| ())
                .map_err(std::string::ToString::to_string),
        );
        result
    }
}
