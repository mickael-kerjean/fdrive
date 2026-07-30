use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::path::RelPath;

use super::{Deletions, Ledger, Outcome};
use crate::model;
use crate::model::{Conflict, Fate, Observation, Operation, Plan};

const WINDOW_QUIET: Duration = Duration::from_millis(250);
const WINDOW_MAX: Duration = Duration::from_secs(2);
const WINDOW_DELETE_MAX: Duration = Duration::from_secs(30);
const OPEN_QUIET: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(10);
const RETRY_CAP: Duration = Duration::from_secs(300);
const BREAKER_BUDGET: u64 = 50;
const BREAKER_FLAG: &str = "breaker_tripped";

pub(super) struct State {
    pub(super) journal: Journal,
    pub(super) ledger: Ledger,
    deletions: Deletions,
    removed: u64,
    tripped: bool,
}

pub(super) struct Journal {
    window: Vec<(Instant, Operation)>,
    marks: BTreeSet<RelPath>,
    pending: BTreeMap<i64, Entry>,

    writing: BTreeMap<RelPath, usize>,
    inflight: BTreeSet<i64>,
}

pub(super) struct Entry {
    plan: Plan,
    attempts: u32,
    due: Instant,
}

pub(super) struct View {
    pub(super) fates: BTreeMap<RelPath, Fate>,
    pub(super) dirty: BTreeSet<RelPath>,
}

#[derive(Clone, Copy)]
enum Release {
    Now,
    At(Instant),
    OnEvent,
}

impl Release {
    fn and(self, other: Release) -> Release {
        match (self, other) {
            (Release::OnEvent, _) | (_, Release::OnEvent) => Release::OnEvent,
            (Release::At(a), Release::At(b)) => Release::At(a.max(b)),
            (Release::At(t), Release::Now) | (Release::Now, Release::At(t)) => Release::At(t),
            (Release::Now, Release::Now) => Release::Now,
        }
    }
}

struct Wake(Option<Instant>);

impl Wake {
    fn note(&mut self, release: Release) {
        if let Release::At(t) = release {
            self.0 = Some(self.0.map_or(t, |w| w.min(t)));
        }
    }
}

pub(super) struct Step {
    pub(super) plans: Vec<(i64, Plan)>,
    pub(super) wake: Option<Instant>,
    pub(super) idle: bool,
}

impl State {
    pub(super) fn open(file: &std::path::Path, deletions: Deletions) -> Self {
        let ledger = match Ledger::open(file) {
            Ok(ledger) => ledger,
            Err(()) => {
                let _ = std::fs::remove_file(file);
                let mut ledger = Ledger::open(file).unwrap_or_default();
                ledger.unreadable = true;
                ledger
            }
        };
        let (recovered, plans) = ledger.journal_load();
        if !plans.is_empty() {
            log::info!("recovered {} pending plans", plans.len());
        }
        let now = Instant::now();
        let mut journal = Journal {
            marks: recovered.iter().flat_map(op_paths).cloned().collect(),
            window: recovered.into_iter().map(|op| (now, op)).collect(),
            pending: BTreeMap::new(),
            writing: BTreeMap::new(),
            inflight: BTreeSet::new(),
        };
        journal.admit(plans);
        let tripped = ledger.meta(BREAKER_FLAG).is_some();
        if tripped {
            log::warn!(
                "deletion breaker was tripped before restart; server-side deletions stay held"
            );
        }
        Self {
            journal,
            ledger,
            deletions,
            removed: 0,
            tripped,
        }
    }

    pub(super) fn record(&mut self, op: Operation) {
        let j = &mut self.journal;
        if let Some(last) = j.window.last_mut() {
            if last.1 == op {
                last.0 = Instant::now();
                return;
            }
        }
        match &op {
            Operation::Create(p) | Operation::Write(p) => {
                if j.marks.insert(p.clone()) {
                    self.ledger.mark(p);
                }
                self.ledger.dirty.insert(p.clone());
            }
            Operation::Rename(a, b) => {
                if self.ledger.dirty.remove(a) {
                    self.ledger.dirty.insert(b.clone());
                }
                if j.marks.remove(a) {
                    j.marks.insert(b.clone());
                    self.ledger.mark_move(a, b);
                }
            }
            Operation::Delete(p) => {
                self.ledger.dirty.remove(p);
                if j.marks.remove(p) {
                    self.ledger.unmark(p);
                }
            }
        }
        j.window.push((Instant::now(), op));
    }

    pub(super) fn step(
        &mut self,
        slots: usize,
        force: bool,
        ignore: &crate::config::Ignore,
        empty: impl Fn(&RelPath) -> bool,
    ) -> Step {
        let mut wake = Wake(None);
        self.compact(force, ignore, empty, &mut wake);
        let plans = self.schedule(slots, &mut wake);
        Step {
            plans,
            wake: wake.0,
            idle: self.idle(),
        }
    }

    fn compact(
        &mut self,
        force: bool,
        ignore: &crate::config::Ignore,
        empty: impl Fn(&RelPath) -> bool,
        wake: &mut Wake,
    ) {
        let drained = self.drain(force, empty, wake);
        if drained.is_empty() {
            return;
        }
        let seeds = self.journal.fold(&drained);
        let mut kept: Vec<RelPath> = Vec::new();
        let made: Vec<Plan> = model::coalesce(seeds.iter().map(|(_, i)| i), &drained, |p| {
            self.ledger.observations.get(p).copied()
        })
        .into_iter()
        .filter(|plan| match plan {
            Plan::Save { path, .. } if ignore.matches(path) => {
                kept.push(path.clone());
                false
            }
            _ => true,
        })
        .collect();
        log::info!(
            "journal [{}] -> [{}]",
            model::render(&drained),
            model::render(&made)
        );
        self.breaker_note(
            made.iter()
                .filter(|p| matches!(p, Plan::Remove { .. }))
                .count(),
        );
        let unmark: Vec<RelPath> = drained
            .iter()
            .flat_map(op_paths)
            .filter(|p| !kept.contains(p))
            .cloned()
            .collect();
        let retired: Vec<i64> = seeds.iter().map(|(seq, _)| *seq).collect();
        let rows = self.ledger.journal_swap(&unmark, &retired, &made);
        for p in &unmark {
            self.journal.marks.remove(p);
        }
        self.journal.admit(rows);
        self.refresh();
    }

    fn drain(
        &mut self,
        force: bool,
        empty: impl Fn(&RelPath) -> bool,
        wake: &mut Wake,
    ) -> Vec<Operation> {
        let stalled =
            |p: &RelPath| self.ledger.observations.get(p).is_some_and(|o| o.size > 0) && empty(p);
        let j = &mut self.journal;
        let now = Instant::now();
        let Some(&(newest, _)) = j.window.last() else {
            return Vec::new();
        };
        let oldest = j.window.first().map_or(newest, |(at, _)| *at);
        let capped = j
            .window
            .iter()
            .find(|(_, op)| !matches!(op, Operation::Delete(_)))
            .map_or(oldest + WINDOW_DELETE_MAX, |(at, _)| *at + WINDOW_MAX);
        let opens = (newest + WINDOW_QUIET).min(capped);
        if !force && now < opens {
            wake.note(Release::At(opens));
            return Vec::new();
        }
        let inflight: Vec<Plan> = j
            .inflight
            .iter()
            .filter_map(|s| j.pending.get(s).map(|e| e.plan.clone()))
            .collect();
        let mut held: Vec<(Instant, Operation)> = Vec::new();
        let mut held_paths: Vec<(RelPath, Release)> = Vec::new();
        let mut drained: Vec<Operation> = Vec::new();
        for (at, op) in std::mem::take(&mut j.window) {
            let mut release = Release::Now;
            for p in op_paths(&op) {
                if inflight.iter().any(|i| i.touches(p)) {
                    release = release.and(Release::OnEvent);
                }
                for (h, blocker) in &held_paths {
                    if h == p || p.is_descendant_of(h) || h.is_descendant_of(p) {
                        release = release.and(*blocker);
                    }
                }
                if !force && j.writing.contains_key(p) && now < at + OPEN_QUIET {
                    release = release.and(Release::At(at + OPEN_QUIET));
                }
                if !force && stalled(p) && now < at + WINDOW_MAX {
                    release = release.and(Release::At(at + WINDOW_MAX));
                }
            }
            if let Release::Now = release {
                drained.push(op);
            } else {
                wake.note(release);
                held_paths.extend(op_paths(&op).into_iter().cloned().map(|p| (p, release)));
                held.push((at, op));
            }
        }
        j.window = held;
        drained
    }

    fn schedule(&mut self, slots: usize, wake: &mut Wake) -> Vec<(i64, Plan)> {
        let j = &mut self.journal;
        let now = Instant::now();
        let mut plans: Vec<(i64, Plan)> = Vec::new();
        'candidates: for (seq, entry) in &j.pending {
            if j.inflight.contains(seq) {
                continue;
            }
            for (_, earlier) in j.pending.range(..seq) {
                if earlier.plan.overlaps(&entry.plan) {
                    continue 'candidates;
                }
            }
            if entry.due > now {
                wake.note(Release::At(entry.due));
            } else if plans.len() < slots {
                plans.push((*seq, entry.plan.clone()));
            }
        }
        for (seq, _) in &plans {
            j.inflight.insert(*seq);
        }
        plans
    }

    pub(super) fn settle(&mut self, seq: i64, outcome: Outcome) -> (bool, Option<Conflict>) {
        self.journal.inflight.remove(&seq);
        let Some(plan) = self.journal.pending.get(&seq).map(|e| e.plan.clone()) else {
            return (false, None);
        };
        match outcome {
            Outcome::Saved { obs, sig, reedited } => {
                if let Plan::Save { path, .. } = &plan {
                    if let Some(obs) = obs {
                        self.ledger.observe(path, obs);
                    }
                    if let Some(sig) = sig {
                        self.ledger.sign_set(path, &sig);
                    }
                    if reedited {
                        self.record(Operation::Write(path.clone()));
                    }
                }
                self.retire(seq);
                (false, None)
            }
            Outcome::Diverted {
                theirs,
                copy,
                obs,
                sig,
                conflict,
            } => {
                if let Plan::Save { path, .. } = &plan {
                    if let Some(theirs) = theirs {
                        self.ledger.observe(path, theirs);
                    }
                }
                if let Some(obs) = obs {
                    self.ledger.observe(&copy, obs);
                }
                if let Some(sig) = sig {
                    self.ledger.sign_set(&copy, &sig);
                }
                self.retire(seq);
                (false, Some(conflict))
            }
            Outcome::Moved => {
                if let Plan::Move { from, to, .. } = &plan {
                    self.ledger.remap(from, to);
                    log::info!("moved {from} -> {to}");
                }
                self.retire(seq);
                (false, None)
            }
            Outcome::MoveLost {
                theirs,
                resurrect,
                conflict,
            } => {
                if let Plan::Move { from, .. } = &plan {
                    match theirs {
                        Some(theirs) => self.ledger.observe(from, theirs),
                        None => self.ledger.unobserve(from),
                    }
                }
                if let Some(to) = resurrect {
                    self.record(Operation::Write(to));
                }
                self.retire(seq);
                (false, conflict)
            }
            Outcome::Removed => {
                if let Plan::Remove { path, dir } = &plan {
                    self.ledger.forget(path);
                    log::info!("removed {path}{}", if *dir { "/" } else { "" });
                }
                self.retire(seq);
                (false, None)
            }
            Outcome::Busy => {
                if let Some(entry) = self.journal.pending.get_mut(&seq) {
                    entry.due = Instant::now() + Duration::from_secs(1);
                }
                (false, None)
            }
            Outcome::Failed(err) => {
                if let Some(entry) = self.journal.pending.get_mut(&seq) {
                    entry.attempts += 1;
                    let n = entry.attempts;
                    if n == 1 {
                        log::error!("replay {}: {err}", entry.plan);
                    } else {
                        log::warn!("replay {} (attempt {n}): {err}", entry.plan);
                    }
                    entry.due = Instant::now()
                        + RETRY.saturating_mul(1u32 << (n - 1).min(5)).min(RETRY_CAP);
                }
                (true, None)
            }
        }
    }

    fn retire(&mut self, seq: i64) {
        self.journal.pending.remove(&seq);
        self.ledger.journal_retire(seq);
        self.refresh();
    }

    fn held(&self, plan: &Plan) -> bool {
        self.tripped && matches!(plan, Plan::Remove { .. })
    }

    pub(super) fn pending_unheld(&self) -> usize {
        self.journal
            .pending
            .values()
            .filter(|e| !self.held(&e.plan))
            .count()
    }

    pub(super) fn pending_sample(&self, n: usize) -> Vec<String> {
        self.journal
            .pending
            .values()
            .filter(|e| !self.held(&e.plan))
            .take(n)
            .map(|e| e.plan.to_string())
            .collect()
    }

    pub(super) fn plan_delete_dir(&mut self, path: &RelPath) {
        let under = |p: &RelPath| p == path || p.is_descendant_of(path);
        self.journal
            .window
            .retain(|(_, op)| !op_paths(op).into_iter().all(under));
        let subsumed: Vec<i64> = self
            .journal
            .pending
            .iter()
            .filter(|(seq, e)| {
                !self.journal.inflight.contains(seq)
                    && matches!(e.plan, Plan::Remove { .. } | Plan::Save { .. })
                    && e.plan.paths().into_iter().all(under)
            })
            .map(|(seq, _)| *seq)
            .collect();
        for seq in subsumed {
            self.journal.pending.remove(&seq);
            self.ledger.journal_retire(seq);
        }
        let doomed: Vec<RelPath> = self
            .journal
            .marks
            .iter()
            .filter(|p| under(p))
            .cloned()
            .collect();
        for p in doomed {
            self.journal.marks.remove(&p);
            self.ledger.unmark(&p);
        }
        self.breaker_note(1);
        let plan = Plan::Remove {
            path: path.clone(),
            dir: true,
        };
        let rows = self.ledger.journal_swap(&[], &[], &[plan]);
        self.journal.admit(rows);
        self.refresh();
    }

    fn breaker_note(&mut self, gestures: usize) {
        if self.deletions != Deletions::Inferred || gestures == 0 {
            return;
        }
        self.removed += gestures as u64;
        if !self.tripped && self.removed >= BREAKER_BUDGET {
            self.tripped = true;
            self.ledger.meta_set(BREAKER_FLAG, "1");
            log::error!(
                "{} deletions this session; further server-side deletions are held",
                self.removed
            );
        }
    }

    pub(super) fn breaker_holds(&self) -> bool {
        self.tripped
    }

    pub(super) fn breaker_tripped(&self) -> bool {
        self.tripped
    }

    pub(super) fn held_deletes(&self) -> usize {
        self.journal
            .pending
            .values()
            .filter(|e| matches!(e.plan, Plan::Remove { .. }))
            .count()
    }

    pub(super) fn breaker_release(&mut self) {
        self.tripped = false;
        self.removed = 0;
        self.ledger.meta_clear(BREAKER_FLAG);
    }

    pub(super) fn breaker_cancel(&mut self) -> usize {
        self.journal
            .window
            .retain(|(_, op)| !matches!(op, Operation::Delete(_)));
        let doomed: Vec<i64> = self
            .journal
            .pending
            .iter()
            .filter(|(seq, e)| {
                !self.journal.inflight.contains(seq) && matches!(e.plan, Plan::Remove { .. })
            })
            .map(|(seq, _)| *seq)
            .collect();
        let cancelled = doomed.len();
        for seq in doomed {
            self.retire(seq);
        }
        self.breaker_release();
        cancelled
    }

    pub(super) fn idle(&self) -> bool {
        self.journal.window.is_empty() && self.journal.pending.is_empty()
    }

    pub(super) fn rush(&mut self) {
        let now = Instant::now();
        for entry in self.journal.pending.values_mut() {
            entry.due = now;
        }
    }

    pub(super) fn write_opened(&mut self, path: &RelPath) {
        *self.journal.writing.entry(path.clone()).or_insert(0) += 1;
    }

    pub(super) fn write_closed(&mut self, path: &RelPath) {
        if let Some(n) = self.journal.writing.get_mut(path) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.journal.writing.remove(path);
            }
        }
    }

    pub(super) fn owed(&self) -> BTreeSet<RelPath> {
        self.journal
            .pending
            .values()
            .flat_map(|e| e.plan.paths().into_iter().cloned().collect::<Vec<_>>())
            .collect()
    }

    pub(super) fn view(&self) -> View {
        let mut fates: BTreeMap<RelPath, Fate> = BTreeMap::new();
        let mut dirty = self.journal.marks.clone();
        let arrive = |fates: &mut BTreeMap<RelPath, Fate>,
                      from: &RelPath,
                      to: &RelPath,
                      was: Option<Observation>| {
            let (root, was) = match fates.get(from) {
                Some(Fate::Arrived { from: root, was }) => (root.clone(), Some(*was)),
                _ => (from.clone(), was),
            };
            fates.insert(from.clone(), Fate::Gone);
            if let Some(was) = was {
                fates.insert(to.clone(), Fate::Arrived { from: root, was });
            }
        };
        for entry in self.journal.pending.values() {
            match &entry.plan {
                Plan::Save { path, .. } => {
                    dirty.insert(path.clone());
                }
                Plan::Move { from, to, moves } => {
                    arrive(&mut fates, from, to, Some(*moves));
                }
                Plan::Remove { path, .. } => {
                    fates.insert(path.clone(), Fate::Gone);
                }
            }
        }
        for (_, op) in &self.journal.window {
            match op {
                Operation::Create(p) | Operation::Write(p) => {
                    fates.remove(p);
                    dirty.insert(p.clone());
                }
                Operation::Rename(from, to) => {
                    let was = self.ledger.observations.get(from).copied();
                    arrive(&mut fates, from, to, was);
                    if dirty.remove(from) {
                        dirty.insert(to.clone());
                    }
                }
                Operation::Delete(p) => {
                    fates.insert(p.clone(), Fate::Gone);
                    dirty.remove(p);
                }
            }
        }
        View { fates, dirty }
    }

    pub(super) fn refresh(&mut self) {
        self.ledger.dirty = self.view().dirty;
    }
}

impl Journal {
    fn fold(&mut self, drained: &[Operation]) -> Vec<(i64, Plan)> {
        let mut seeds: Vec<(i64, Plan)> = Vec::new();
        loop {
            let next = self.pending.iter().find(|(seq, entry)| {
                !self.inflight.contains(seq)
                    && !matches!(entry.plan, Plan::Remove { dir: true, .. })
                    && !seeds.iter().any(|(s, _)| s == *seq)
                    && (drained
                        .iter()
                        .flat_map(op_paths)
                        .any(|p| entry.plan.touches(p))
                        || seeds.iter().any(|(_, i)| i.overlaps(&entry.plan)))
            });
            match next {
                Some((seq, _)) => {
                    let seq = *seq;
                    seeds.push((seq, self.pending.remove(&seq).unwrap().plan));
                }
                None => return seeds,
            }
        }
    }

    fn admit(&mut self, rows: Vec<(i64, Plan)>) {
        let now = Instant::now();
        for (seq, plan) in rows {
            self.pending.insert(
                seq,
                Entry {
                    plan,
                    attempts: 0,
                    due: now,
                },
            );
        }
    }
}

fn op_paths(op: &Operation) -> Vec<&RelPath> {
    match op {
        Operation::Create(p) | Operation::Write(p) | Operation::Delete(p) => vec![p],
        Operation::Rename(a, b) => vec![a, b],
    }
}

pub struct LedgerGuard<'a>(pub(super) std::sync::MutexGuard<'a, State>);

impl std::ops::Deref for LedgerGuard<'_> {
    type Target = Ledger;
    fn deref(&self) -> &Ledger {
        &self.0.ledger
    }
}

impl std::ops::DerefMut for LedgerGuard<'_> {
    fn deref_mut(&mut self) -> &mut Ledger {
        &mut self.0.ledger
    }
}
