use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tiny_http::{Header, Method, Response};

const DELTA_MEDIA_TYPE: &str = "application/vnd.filestash.delta.rdiff";

fn apply_delta(base: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    if body.len() < 32 {
        return None;
    }
    let (diff, sha) = body.split_at(body.len() - 32);
    let mut full = Vec::new();
    fast_rsync::apply(base, diff, &mut full).ok()?;
    use sha2::Digest;
    (sha2::Sha256::digest(&full).as_slice() == sha).then_some(full)
}

#[derive(Default)]
struct Store {
    files: BTreeMap<String, (Vec<u8>, SystemTime)>,
    dirs: BTreeSet<String>,
    log: Vec<String>,
}

#[derive(Default)]
struct State {
    store: Mutex<Store>,
    offline: AtomicBool,
    throttle: Mutex<Option<Duration>>,
}

#[derive(Clone)]
pub struct FakeServer {
    state: Arc<State>,
    url: String,
}

impl FakeServer {
    pub fn start() -> Self {
        let inner = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
        let url = format!("http://{}", inner.server_addr());
        let state = Arc::new(State::default());
        for _ in 0..4 {
            let inner = inner.clone();
            let state = state.clone();
            std::thread::spawn(move || {
                while let Ok(req) = inner.recv() {
                    handle(&state, req);
                }
            });
        }
        Self { state, url }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn put(&self, path: &str, data: &[u8]) {
        self.put_at(path, data, now());
    }

    pub fn put_at(&self, path: &str, data: &[u8], mtime: SystemTime) {
        let secs = mtime.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mtime = UNIX_EPOCH + Duration::from_secs(secs);
        let mut store = self.state.store.lock().unwrap();
        make_parents(&mut store, path);
        store
            .files
            .insert(norm(path).to_string(), (data.to_vec(), mtime));
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        let store = self.state.store.lock().unwrap();
        store.files.get(norm(path)).map(|(data, _)| data.clone())
    }

    pub fn names(&self, dir: &str) -> Vec<String> {
        let store = self.state.store.lock().unwrap();
        let mut names: Vec<String> = store
            .files
            .keys()
            .chain(store.dirs.iter())
            .filter(|p| parent(p) == norm(dir))
            .map(|p| p.rsplit('/').next().unwrap().to_string())
            .collect();
        names.sort();
        names
    }

    pub fn rm(&self, path: &str) {
        let path = norm(path).to_string();
        let mut store = self.state.store.lock().unwrap();
        store.files.remove(&path);
        let prefix = format!("{path}/");
        store.files.retain(|p, _| !p.starts_with(&prefix));
        store.dirs.retain(|p| p != &path && !p.starts_with(&prefix));
    }

    pub fn offline(&self, down: bool) {
        self.state.offline.store(down, Ordering::SeqCst);
    }

    pub fn throttle(&self, pause: Option<Duration>) {
        *self.state.throttle.lock().unwrap() = pause;
    }

    pub fn log(&self) -> Vec<String> {
        self.state.store.lock().unwrap().log.clone()
    }
}

fn now() -> SystemTime {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn norm(path: &str) -> &str {
    path.trim_end_matches('/')
}

fn parent(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

fn make_parents(store: &mut Store, path: &str) {
    let mut dir = parent(norm(path)).to_string();
    while !dir.is_empty() {
        store.dirs.insert(dir.clone());
        dir = parent(&dir).to_string();
    }
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(url: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some((_, qs)) = url.split_once('?') {
        for pair in qs.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                out.insert(decode(k), decode(v));
            }
        }
    }
    out
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn ok_json(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body).with_header(header("content-type", "application/json"))
}

struct Slow<R> {
    inner: R,
    pause: Duration,
}

impl<R: Read> Read for Slow<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::thread::sleep(self.pause);
        let cap = buf.len().min(16 * 1024);
        self.inner.read(&mut buf[..cap])
    }
}

fn handle(state: &State, mut req: tiny_http::Request) {
    if state.offline.load(Ordering::SeqCst) {
        let _ = req.respond(Response::from_string("down").with_status_code(500));
        return;
    }
    let q = query(req.url());
    let route = req.url().split('?').next().unwrap_or("").to_string();
    let method = req.method().clone();
    let mut body = Vec::new();
    let _ = req.as_reader().read_to_end(&mut body);
    macro_rules! find {
        ($name:literal) => {
            req.headers()
                .iter()
                .find(|h| h.field.equiv($name))
                .map(|h| h.value.as_str().to_string())
        };
    }
    let since = find!("if-unmodified-since").and_then(|v| httpdate::parse_http_date(&v).ok());
    let is_delta = find!("content-type").as_deref() == Some(DELTA_MEDIA_TYPE);
    let copy_source = find!("x-copy-source");

    let respond = |req: tiny_http::Request, code: u32| {
        let _ = req.respond(Response::from_string("").with_status_code(code as u16));
    };

    match (method, route.as_str()) {
        (Method::Get, "/api/files/ls") => {
            let dir = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let store = state.store.lock().unwrap();
            if !dir.is_empty() && !store.dirs.contains(&dir) {
                return respond(req, 404);
            }
            let mut results = Vec::new();
            for d in &store.dirs {
                if parent(d) == dir {
                    results.push(serde_json::json!({
                        "name": d.rsplit('/').next().unwrap(),
                        "type": "directory", "size": 0, "time": 0,
                    }));
                }
            }
            for (f, (data, mtime)) in &store.files {
                if parent(f) == dir {
                    let ms = mtime.duration_since(UNIX_EPOCH).unwrap().as_millis();
                    results.push(serde_json::json!({
                        "name": f.rsplit('/').next().unwrap(),
                        "type": "file", "size": data.len(), "time": ms,
                    }));
                }
            }
            let body = serde_json::json!({"status": "ok", "results": results});
            let _ = req.respond(ok_json(body.to_string()));
        }
        (Method::Head, "/api/files/cat") => {
            let path = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let store = state.store.lock().unwrap();
            match store.files.get(&path) {
                Some((data, mtime)) => {
                    let resp = Response::new(
                        200.into(),
                        vec![header("last-modified", &httpdate::fmt_http_date(*mtime))],
                        std::io::empty(),
                        Some(data.len()),
                        None,
                    );
                    let _ = req.respond(resp);
                }
                None => respond(req, 404),
            }
        }
        (Method::Get, "/api/files/cat") => {
            let path = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let found = state.store.lock().unwrap().files.get(&path).cloned();
            match found {
                Some((data, mtime)) => {
                    if find!("accept").is_some_and(|a| a.contains(DELTA_MEDIA_TYPE)) {
                        let mut body = fast_rsync::Signature::calculate(
                            &data,
                            fast_rsync::SignatureOptions {
                                block_size: 4096,
                                crypto_hash_size: 16,
                            },
                        )
                        .into_serialized();
                        use sha2::Digest;
                        body.extend_from_slice(&sha2::Sha256::digest(&data));
                        state.store.lock().unwrap().log.push(format!("sig {path}"));
                        let len = body.len();
                        let resp = Response::new(
                            200.into(),
                            vec![
                                header("last-modified", &httpdate::fmt_http_date(mtime)),
                                header("content-type", DELTA_MEDIA_TYPE),
                            ],
                            std::io::Cursor::new(body),
                            Some(len),
                            None,
                        );
                        let _ = req.respond(resp);
                        return;
                    }
                    if let Some(r) = find!("range").and_then(|r| r.strip_prefix("bytes=").map(String::from)) {
                        let (a, b) = r.split_once('-').unwrap_or((r.as_str(), ""));
                        let start: usize = a.parse().unwrap_or(0);
                        let end = b
                            .parse::<usize>()
                            .map(|e| e + 1)
                            .unwrap_or(data.len())
                            .min(data.len());
                        if start >= end {
                            return respond(req, 416);
                        }
                        let slice = data[start..end].to_vec();
                        state
                            .store
                            .lock()
                            .unwrap()
                            .log
                            .push(format!("range {path} {start}-{}", end - 1));
                        let len = slice.len();
                        let resp = Response::new(
                            206.into(),
                            vec![
                                header("last-modified", &httpdate::fmt_http_date(mtime)),
                                header(
                                    "content-range",
                                    &format!("bytes {start}-{}/{}", end - 1, data.len()),
                                ),
                            ],
                            std::io::Cursor::new(slice),
                            Some(len),
                            None,
                        );
                        let _ = req.respond(resp);
                        return;
                    }
                    let len = data.len();
                    let pause = *state.throttle.lock().unwrap();
                    let reader = Slow {
                        inner: std::io::Cursor::new(data),
                        pause: pause.unwrap_or(Duration::ZERO),
                    };
                    let resp = Response::new(
                        200.into(),
                        vec![header("last-modified", &httpdate::fmt_http_date(mtime))],
                        reader,
                        Some(len),
                        None,
                    );
                    let _ = req.respond(resp);
                }
                None => respond(req, 404),
            }
        }
        (Method::Post, "/api/files/cat") => {
            let path = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let mut store = state.store.lock().unwrap();
            if let (Some((_, mtime)), Some(since)) = (store.files.get(&path), since) {
                if *mtime > since {
                    return respond(req, 412);
                }
            }
            let verb = if is_delta { "delta" } else { "save" };
            let body = if is_delta {
                let base = copy_source.as_deref().map(norm).unwrap_or(&path);
                let Some((base_data, _)) = store.files.get(base) else {
                    return respond(req, 422);
                };
                match apply_delta(base_data, &body) {
                    Some(full) => full,
                    None => return respond(req, 422),
                }
            } else {
                body
            };
            let mtime = now();
            make_parents(&mut store, &path);
            store.files.insert(path.clone(), (body, mtime));
            store.log.push(format!("{verb} {path}"));
            let resp = ok_json(r#"{"status":"ok"}"#.to_string())
                .with_header(header("last-modified", &httpdate::fmt_http_date(mtime)));
            let _ = req.respond(resp);
        }
        (Method::Post, "/api/files/mkdir") => {
            let path = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let mut store = state.store.lock().unwrap();
            make_parents(&mut store, &path);
            store.dirs.insert(path.clone());
            store.log.push(format!("mkdir {path}"));
            let _ = req.respond(ok_json(r#"{"status":"ok"}"#.to_string()));
        }
        (Method::Post, "/api/files/rm") => {
            let path = q
                .get("path")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let mut store = state.store.lock().unwrap();
            let sub = format!("{path}/");
            if store.dirs.remove(&path) {
                store.files.retain(|f, _| !f.starts_with(&sub));
                store.dirs.retain(|d| !d.starts_with(&sub));
            } else if store.files.remove(&path).is_none() {
                return respond(req, 404);
            }
            store.log.push(format!("rm {path}"));
            let _ = req.respond(ok_json(r#"{"status":"ok"}"#.to_string()));
        }
        (Method::Post, "/api/files/mv") => {
            let from = q
                .get("from")
                .map(|p| norm(p).to_string())
                .unwrap_or_default();
            let to = q.get("to").map(|p| norm(p).to_string()).unwrap_or_default();
            let mut store = state.store.lock().unwrap();
            let sub = format!("{from}/");
            if store.dirs.remove(&from) {
                store.dirs.insert(to.clone());
                let moved: Vec<String> = store
                    .files
                    .keys()
                    .filter(|f| f.starts_with(&sub))
                    .cloned()
                    .collect();
                for f in moved {
                    let entry = store.files.remove(&f).unwrap();
                    store.files.insert(f.replacen(&from, &to, 1), entry);
                }
            } else if let Some(entry) = store.files.remove(&from) {
                make_parents(&mut store, &to);
                store.files.insert(to.clone(), entry);
            } else {
                return respond(req, 404);
            }
            store.log.push(format!("mv {from} {to}"));
            let _ = req.respond(ok_json(r#"{"status":"ok"}"#.to_string()));
        }
        (Method::Options, "/api/files/save") => {
            let resp =
                Response::from_string("").with_header(header("Accept-Post", DELTA_MEDIA_TYPE));
            let _ = req.respond(resp);
        }
        _ => respond(req, 404),
    }
}
