use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const METER_SECONDS: usize = 120;
const MAX_RECORDS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Delta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Transfer {
    pub id: u64,
    pub path: String,
    pub direction: Direction,
    pub mode: Mode,
    pub size: u64,
    pub wire: u64,
    pub progress: u64,
    pub outcome: Outcome,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub version: u64,
    pub transfers: Vec<Transfer>,
    pub meter: Vec<(u64, u64)>,
}

#[derive(Default)]
struct Bucket {
    at: u64,
    up: u64,
    down: u64,
}

struct Inner {
    records: VecDeque<Transfer>,
    meter: Vec<Bucket>,
    version: u64,
    next: u64,
}

pub struct Activity {
    inner: Mutex<Inner>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner {
                records: VecDeque::new(),
                meter: (0..METER_SECONDS).map(|_| Bucket::default()).collect(),
                version: 0,
                next: 0,
            }),
        }
    }
}

impl Activity {
    pub fn begin(&self, path: &str, direction: Direction, size: u64) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        inner.version += 1;
        inner.next += 1;
        let id = inner.next;
        if inner.records.len() >= MAX_RECORDS {
            inner.records.pop_back();
        }
        inner.records.push_front(Transfer {
            id,
            path: path.to_string(),
            direction,
            mode: Mode::Full,
            size,
            wire: 0,
            progress: 0,
            outcome: Outcome::Running,
        });
        id
    }

    pub fn mode(&self, id: u64, mode: Mode) {
        self.update(id, |t| t.mode = mode);
    }

    pub fn progress(&self, id: u64, done: u64) {
        self.update(id, |t| t.progress = done);
    }

    pub fn finish(&self, id: u64, result: Result<(), String>) {
        self.update(id, |t| {
            t.outcome = match result {
                Ok(()) => {
                    t.progress = t.size;
                    Outcome::Done
                }
                Err(err) => Outcome::Failed(err),
            }
        });
    }

    pub fn wire(&self, id: u64, bytes: u64) {
        let mut inner = self.inner.lock().unwrap();
        inner.version += 1;
        let direction = match inner.records.iter_mut().find(|t| t.id == id) {
            Some(t) => {
                t.wire += bytes;
                t.direction
            }
            None => return,
        };
        let at = now_secs();
        let bucket = &mut inner.meter[(at % METER_SECONDS as u64) as usize];
        if bucket.at != at {
            *bucket = Bucket {
                at,
                ..Default::default()
            };
        }
        match direction {
            Direction::Up => bucket.up += bytes,
            Direction::Down => bucket.down += bytes,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().unwrap();
        let now = now_secs();
        let meter = (0..METER_SECONDS as u64)
            .map(|back| {
                let at = now - (METER_SECONDS as u64 - 1) + back;
                let bucket = &inner.meter[(at % METER_SECONDS as u64) as usize];
                if bucket.at == at {
                    (bucket.up, bucket.down)
                } else {
                    (0, 0)
                }
            })
            .collect();
        Snapshot {
            version: inner.version,
            transfers: inner.records.iter().cloned().collect(),
            meter,
        }
    }

    fn update(&self, id: u64, apply: impl FnOnce(&mut Transfer)) {
        let mut inner = self.inner.lock().unwrap();
        inner.version += 1;
        if let Some(t) = inner.records.iter_mut().find(|t| t.id == id) {
            apply(t);
        }
    }
}

pub fn sparkline(snap: &Snapshot, width: usize) -> String {
    const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let start = snap.meter.len().saturating_sub(width);
    let window = &snap.meter[start..];
    let max = window.iter().map(|(u, d)| u + d).max().unwrap_or(0).max(1);
    window
        .iter()
        .map(|(up, down)| {
            let v = up + down;
            if v == 0 {
                ' '
            } else {
                GLYPHS[((v * 7).div_ceil(max) as usize).min(7)]
            }
        })
        .collect()
}

pub fn rate_line(snap: &Snapshot) -> String {
    let n = 5.min(snap.meter.len()).max(1);
    let (up, down) = snap.meter[snap.meter.len() - n..]
        .iter()
        .fold((0, 0), |(u, d), (bu, bd)| (u + bu, d + bd));
    format!(
        "↓{}/s ↑{}/s",
        fmt_compact(down / n as u64),
        fmt_compact(up / n as u64)
    )
}

pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn fmt_compact(n: u64) -> String {
    fmt_bytes(n).replace(' ', "")
}

#[cfg(test)]
#[path = "activity_test.rs"]
mod tests;
