use super::*;

#[test]
fn a_transfer_goes_through_its_lifecycle() {
    let activity = Activity::default();
    let id = activity.begin("/video.mp4", Direction::Down, 1000);
    activity.mode(id, Mode::Delta);
    activity.wire(id, 12);
    activity.wire(id, 30);
    activity.progress(id, 500);

    let snap = activity.snapshot();
    let t = &snap.transfers[0];
    assert_eq!(t.path, "/video.mp4");
    assert_eq!(t.mode, Mode::Delta);
    assert_eq!(t.wire, 42);
    assert_eq!(t.progress, 500);
    assert_eq!(t.outcome, Outcome::Running);

    activity.finish(id, Ok(()));
    let t = activity.snapshot().transfers[0].clone();
    assert_eq!(t.outcome, Outcome::Done);
    assert_eq!(t.progress, 1000);
}

#[test]
fn a_failure_keeps_its_message() {
    let activity = Activity::default();
    let id = activity.begin("/doc.txt", Direction::Up, 10);
    activity.finish(id, Err("server said no".into()));
    assert_eq!(
        activity.snapshot().transfers[0].outcome,
        Outcome::Failed("server said no".into())
    );
}

#[test]
fn newest_transfers_come_first_and_old_ones_fall_off() {
    let activity = Activity::default();
    for i in 0..250 {
        activity.begin(&format!("/f{i}"), Direction::Up, 1);
    }
    let snap = activity.snapshot();
    assert_eq!(snap.transfers.len(), 200);
    assert_eq!(snap.transfers[0].path, "/f249");
    assert_eq!(snap.transfers.last().unwrap().path, "/f50");
}

#[test]
fn the_meter_accounts_every_wire_byte() {
    let activity = Activity::default();
    let down = activity.begin("/a", Direction::Down, 100);
    let up = activity.begin("/b", Direction::Up, 100);
    activity.wire(down, 70);
    activity.wire(up, 30);
    activity.wire(down, 5);

    let snap = activity.snapshot();
    let (up_total, down_total) = snap
        .meter
        .iter()
        .fold((0, 0), |(u, d), (bu, bd)| (u + bu, d + bd));
    assert_eq!(up_total, 30);
    assert_eq!(down_total, 75);
    assert_eq!(snap.meter.len(), METER_SECONDS);
}

#[test]
fn versions_move_with_every_mutation() {
    let activity = Activity::default();
    let v0 = activity.snapshot().version;
    let id = activity.begin("/a", Direction::Down, 1);
    let v1 = activity.snapshot().version;
    activity.wire(id, 1);
    let v2 = activity.snapshot().version;
    assert!(v0 < v1 && v1 < v2);
}
