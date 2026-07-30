use super::*;
use std::time::Duration;

fn p(s: &str) -> RelPath {
    RelPath::new(s)
}

fn obs(v: u64) -> Observation {
    Observation { size: v, time: v }
}

fn fold(ops: &[Operation], known: &[&str]) -> Vec<Plan> {
    let known: Vec<RelPath> = known.iter().map(|s| p(s)).collect();
    coalesce([].iter(), ops, |q| {
        known.contains(q).then(|| obs(q.as_str().len() as u64))
    })
}

fn save(path: &str, replaces: Option<&str>, reuses: Option<&str>) -> Plan {
    Plan::Save {
        path: p(path),
        replaces: replaces.map(|r| obs(r.len() as u64)),
        reuses: reuses.map(p),
    }
}

fn mv(from: &str, to: &str) -> Plan {
    Plan::Move {
        from: p(from),
        to: p(to),
        moves: obs(from.len() as u64),
    }
}

fn rm(path: &str) -> Plan {
    Plan::Remove {
        path: p(path),
        dir: false,
    }
}

#[test]
fn observation_time_is_whole_seconds() {
    let fine = UNIX_EPOCH + Duration::from_millis(3_700);
    assert_eq!(
        Observation::new(5, Some(fine)),
        Observation { size: 5, time: 3 }
    );
    assert_eq!(Observation::new(0, None), Observation { size: 0, time: 0 });
}

#[test]
fn vim_dance_is_one_save() {
    let ops = [
        Operation::Rename(p("a"), p("a~")),
        Operation::Create(p("a")),
        Operation::Write(p("a")),
        Operation::Delete(p("a~")),
    ];
    assert_eq!(fold(&ops, &["a"]), vec![save("a", Some("a"), None)]);
}

#[test]
fn replacefile_dance_is_one_save() {
    let ops = [
        Operation::Create(p("t.tmp")),
        Operation::Write(p("t.tmp")),
        Operation::Rename(p("a"), p("a~RF.TMP")),
        Operation::Rename(p("t.tmp"), p("a")),
        Operation::Delete(p("a~RF.TMP")),
    ];
    assert_eq!(fold(&ops, &["a"]), vec![save("a", Some("a"), None)]);
}

#[test]
fn exiftool_keeps_its_backup() {
    let ops = [
        Operation::Create(p("x_tmp")),
        Operation::Write(p("x_tmp")),
        Operation::Rename(p("x"), p("x_original")),
        Operation::Rename(p("x_tmp"), p("x")),
    ];
    assert_eq!(
        fold(&ops, &["x"]),
        vec![mv("x", "x_original"), save("x", Some("x"), None)]
    );
}

#[test]
fn rename_then_edit_saves_with_provenance_then_removes() {
    let ops = [Operation::Rename(p("a"), p("b")), Operation::Write(p("b"))];
    assert_eq!(
        fold(&ops, &["a"]),
        vec![save("b", None, Some("a")), rm("a")]
    );
}

#[test]
fn temp_file_that_dies_is_nothing() {
    let ops = [
        Operation::Create(p("t.swp")),
        Operation::Write(p("t.swp")),
        Operation::Delete(p("t.swp")),
    ];
    assert_eq!(fold(&ops, &[]), vec![]);
}

#[test]
fn deleted_original_is_a_remove_even_when_edited_first() {
    let ops = [Operation::Write(p("a")), Operation::Delete(p("a"))];
    assert_eq!(fold(&ops, &["a"]), vec![rm("a")]);
}

#[test]
fn rename_chain_folds() {
    let ops = [
        Operation::Rename(p("a"), p("b")),
        Operation::Rename(p("b"), p("c")),
    ];
    assert_eq!(fold(&ops, &["a"]), vec![mv("a", "c")]);
}

#[test]
fn clobbering_chain_tombstones_the_vacated_name() {
    let ops = [
        Operation::Rename(p("c"), p("a")),
        Operation::Rename(p("a"), p("b")),
    ];
    assert_eq!(fold(&ops, &["a", "c"]), vec![mv("c", "b"), rm("a")]);
}

#[test]
fn plain_ops_pass_through() {
    let ops = [Operation::Rename(p("a"), p("b")), Operation::Delete(p("x"))];
    assert_eq!(fold(&ops, &["a", "x"]), vec![mv("a", "b"), rm("x")]);
}

#[test]
fn edit_survives_a_following_dance() {
    let ops = [
        Operation::Write(p("a")),
        Operation::Rename(p("a"), p("a~")),
        Operation::Create(p("a")),
        Operation::Write(p("a")),
        Operation::Delete(p("a~")),
    ];
    assert_eq!(fold(&ops, &["a"]), vec![save("a", Some("a"), None)]);
}

#[test]
fn unobserved_paths_never_earn_tombstones() {
    let ops = [
        Operation::Rename(p("a"), p("a~")),
        Operation::Delete(p("a~")),
    ];
    assert_eq!(fold(&ops, &["a"]), vec![rm("a")]);
}

#[test]
fn swap_degrades_to_saves() {
    let ops = [
        Operation::Rename(p("a"), p("t")),
        Operation::Rename(p("b"), p("a")),
        Operation::Rename(p("t"), p("b")),
    ];
    assert_eq!(
        fold(&ops, &["a", "b"]),
        vec![
            save("b", Some("b"), Some("a")),
            save("a", Some("a"), Some("b"))
        ]
    );
}

#[test]
fn pending_intents_fold_with_the_next_burst() {
    let pending = [save("b", None, Some("a")), rm("a")];
    let ops = [Operation::Delete(p("b"))];
    let folded = coalesce(pending.iter(), &ops, |q| (q == &p("a")).then(|| obs(1)));
    assert_eq!(
        folded,
        vec![Plan::Remove {
            path: p("a"),
            dir: false,
        }]
    );
}

#[test]
fn pending_save_supersedes_on_reedit() {
    let pending = [save("a", Some("a"), None)];
    let ops = [Operation::Write(p("a"))];
    let folded = coalesce(pending.iter(), &ops, |q| (q == &p("a")).then(|| obs(1)));
    assert_eq!(
        folded,
        vec![Plan::Save {
            path: p("a"),
            replaces: Some(obs(1)),
            reuses: None,
        }]
    );
}

#[test]
fn hazard_overlap_includes_reuses() {
    let save = save("b", None, Some("a"));
    let remove = rm("a");
    assert!(save.overlaps(&remove));
}

fn xorshift(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

const ALPHABET: [&str; 6] = ["a", "b", "c", "d/x", "d/y", "e"];

fn random_ops(seed: &mut u64) -> Vec<Operation> {
    let count = 1 + (xorshift(seed) % 20) as usize;
    (0..count)
        .map(|_| {
            let path = p(ALPHABET[(xorshift(seed) % 6) as usize]);
            match xorshift(seed) % 4 {
                0 => Operation::Create(path),
                1 => Operation::Write(path),
                2 => Operation::Delete(path),
                _ => Operation::Rename(path, p(ALPHABET[(xorshift(seed) % 6) as usize])),
            }
        })
        .collect()
}

fn random_known(seed: &mut u64) -> Vec<RelPath> {
    ALPHABET
        .iter()
        .filter(|_| xorshift(seed) % 2 == 0)
        .map(|s| p(s))
        .collect()
}

fn random_pending(seed: &mut u64) -> Vec<Plan> {
    let count = (xorshift(seed) % 4) as usize;
    (0..count)
        .map(|_| {
            let at = (xorshift(seed) % 6) as usize;
            let path = ALPHABET[at];
            match xorshift(seed) % 4 {
                0 => save(path, None, None),
                1 => save(path, Some(path), None),
                2 => rm(path),
                _ => mv(path, ALPHABET[(at + 1 + (xorshift(seed) % 5) as usize) % 6]),
            }
        })
        .collect()
}

fn history(
    pending: &[Plan],
    ops: &[Operation],
) -> BTreeMap<RelPath, (Option<usize>, Option<usize>)> {
    let mut events: BTreeMap<RelPath, (Option<usize>, Option<usize>)> = BTreeMap::new();
    let left = |map: &mut BTreeMap<RelPath, (Option<usize>, Option<usize>)>,
                path: &RelPath,
                at: usize| map.entry(path.clone()).or_default().0 = Some(at);
    let came = |map: &mut BTreeMap<RelPath, (Option<usize>, Option<usize>)>,
                path: &RelPath,
                at: usize| map.entry(path.clone()).or_default().1 = Some(at);
    let mut at = 0;
    for plan in pending {
        match plan {
            Plan::Save { path, .. } => came(&mut events, path, at),
            Plan::Move { from, to, .. } => {
                left(&mut events, from, at);
                came(&mut events, to, at);
            }
            Plan::Remove { path, .. } => left(&mut events, path, at),
        }
        at += 1;
    }
    for op in ops {
        match op {
            Operation::Create(path) | Operation::Write(path) => came(&mut events, path, at),
            Operation::Delete(path) => left(&mut events, path, at),
            Operation::Rename(from, to) if from == to => {}
            Operation::Rename(from, to) => {
                left(&mut events, from, at);
                came(&mut events, to, at);
            }
        }
        at += 1;
    }
    events
}

fn assert_removes_earned(pending: &[Plan], ops: &[Operation], known: &[RelPath], out: &[Plan]) {
    let events = history(pending, ops);
    let pending_dirs: Vec<&RelPath> = pending
        .iter()
        .filter_map(|plan| match plan {
            Plan::Remove { path, dir: true } => Some(path),
            _ => None,
        })
        .collect();
    let removed: Vec<&RelPath> = out
        .iter()
        .filter_map(|plan| match plan {
            Plan::Remove { path, dir: false } => Some(path),
            _ => None,
        })
        .collect();
    for plan in out {
        match plan {
            Plan::Save { path, .. } => {
                assert!(
                    !removed.contains(&path),
                    "{path} is both saved and removed\npending: {pending:?}\nops: {ops:?}"
                );
            }
            Plan::Move { from, to, .. } => {
                assert!(
                    !removed.contains(&from) && !removed.contains(&to),
                    "move {from}->{to} collides with a remove\npending: {pending:?}\nops: {ops:?}"
                );
            }
            Plan::Remove { path, dir: false } => {
                assert!(
                    known.contains(path),
                    "removed {path} without an observation\npending: {pending:?}\nops: {ops:?}"
                );
                let (leaving, arriving) = events.get(path).copied().unwrap_or((None, None));
                let leaving = leaving.unwrap_or_else(|| {
                    panic!("removed {path} which nothing left\npending: {pending:?}\nops: {ops:?}")
                });
                assert!(
                    arriving.is_none_or(|came| came < leaving),
                    "removed {path} after something recreated it\npending: {pending:?}\nops: {ops:?}"
                );
            }
            Plan::Remove { path, dir: true } => {
                assert!(
                    pending_dirs.contains(&path),
                    "invented a dir remove for {path}\npending: {pending:?}\nops: {ops:?}"
                );
            }
        }
    }
}

#[test]
fn a_remove_is_always_earned() {
    for round in 0..4000u64 {
        let mut seed = 0x9e3779b97f4a7c15 ^ (round + 1);
        let ops = random_ops(&mut seed);
        let known = random_known(&mut seed);
        let out = coalesce([].iter(), &ops, |q| {
            known.contains(q).then(|| obs(q.as_str().len() as u64))
        });
        assert_removes_earned(&[], &ops, &known, &out);
    }
}

#[test]
fn a_remove_is_always_earned_with_pending_plans() {
    for round in 0..4000u64 {
        let mut seed = 0xdeadbeefcafef00d ^ (round + 1);
        let pending = random_pending(&mut seed);
        let ops = random_ops(&mut seed);
        let known = random_known(&mut seed);
        let out = coalesce(pending.iter(), &ops, |q| {
            known.contains(q).then(|| obs(q.as_str().len() as u64))
        });
        assert_removes_earned(&pending, &ops, &known, &out);
    }
}

#[test]
fn a_recreated_path_is_never_removed() {
    for round in 0..2000u64 {
        let mut seed = 0x5bf03635f0c1e7a9 ^ (round + 1);
        let mut ops = random_ops(&mut seed);
        let target = p(ALPHABET[(xorshift(&mut seed) % 6) as usize]);
        ops.push(Operation::Create(target.clone()));
        let out = fold(&ops, &ALPHABET);
        assert!(
            !out.iter()
                .any(|plan| matches!(plan, Plan::Remove { path, dir: false } if *path == target)),
            "{target} was recreated last yet removed\nops: {ops:?}"
        );
    }
}

fn simulate(ops: &[Operation], known: &[RelPath]) -> BTreeMap<RelPath, bool> {
    let mut exists: BTreeMap<RelPath, bool> =
        known.iter().map(|path| (path.clone(), true)).collect();
    for op in ops {
        match op {
            Operation::Create(path) | Operation::Write(path) => {
                exists.insert(path.clone(), true);
            }
            Operation::Delete(path) => {
                exists.insert(path.clone(), false);
            }
            Operation::Rename(from, to) if from == to => {}
            Operation::Rename(from, to) => {
                exists.insert(from.clone(), false);
                exists.insert(to.clone(), true);
            }
        }
    }
    exists
}

#[test]
fn a_delete_is_never_swallowed() {
    for round in 0..4000u64 {
        let mut seed = 0x2545f4914f6cdd1d ^ (round + 1);
        let ops = random_ops(&mut seed);
        let known = random_known(&mut seed);
        let out = coalesce([].iter(), &ops, |q| {
            known.contains(q).then(|| obs(q.as_str().len() as u64))
        });
        let exists = simulate(&ops, &known);
        for path in &known {
            if exists.get(path).copied().unwrap_or(true) {
                continue;
            }
            let accounted = out.iter().any(|plan| match plan {
                Plan::Remove {
                    path: q,
                    dir: false,
                } => q == path,
                Plan::Move { from, .. } => from == path,
                Plan::Save { reuses, .. } => reuses.as_ref() == Some(path),
                _ => false,
            });
            assert!(
                accounted,
                "{path} is gone but no plan accounts for it\nops: {ops:?}\nout: {out:?}"
            );
        }
    }
}

#[test]
fn refolding_never_invents_removes() {
    for round in 0..4000u64 {
        let mut seed = 0x6c62272e07bb0142 ^ (round + 1);
        let ops = random_ops(&mut seed);
        let known = random_known(&mut seed);
        let know = |q: &RelPath| known.contains(q).then(|| obs(q.as_str().len() as u64));
        let first = coalesce([].iter(), &ops, know);
        let second = coalesce(first.iter(), &[], know);
        let removes = |plans: &[Plan]| -> Vec<RelPath> {
            plans
                .iter()
                .filter_map(|plan| match plan {
                    Plan::Remove { path, dir: false } => Some(path.clone()),
                    _ => None,
                })
                .collect()
        };
        let before = removes(&first);
        for path in removes(&second) {
            assert!(
                before.contains(&path),
                "refolding invented a remove for {path}\nops: {ops:?}\nfirst: {first:?}\nsecond: {second:?}"
            );
        }
    }
}
