use super::*;

#[test]
fn ls_serves_the_stale_listing_when_the_server_is_unreachable() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = std::env::temp_dir().join(format!("fdrive-stale-ls-{}", std::process::id()));
    fs::create_dir_all(&data).unwrap();
    let sdk = Sdk::new("http://127.0.0.1:9").unwrap();
    let adapter = Adapter::new(Arc::new(sdk), rt.handle().clone(), &data).unwrap();

    let dir = RelPath::new("d");
    let expired = Instant::now()
        .checked_sub(Duration::from_secs(600))
        .unwrap();
    adapter.engine.local().meta.lock().unwrap().insert(
        dir.clone(),
        (
            expired,
            vec![FileInfo {
                name: "a.txt".to_string(),
                kind: FileType::File,
                size: Some(1),
                mtime: None,
            }],
        ),
    );

    let listing = adapter.ls(&dir).unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name, "a.txt");

    adapter.engine.ledger().observe(
        &RelPath::new("pinned/manual.pdf"),
        fdrive_core::engine::Observation::new(9, None),
    );
    let remembered = adapter.ls(&RelPath::new("pinned")).unwrap();
    assert_eq!(remembered.len(), 1, "no stale listing, the ledger answers");
    assert_eq!(remembered[0].name, "manual.pdf");
    assert!(
        adapter.ls(&RelPath::new("never-seen")).unwrap().is_empty(),
        "an unknown dir is what the ledger remembers: nothing"
    );
    let _ = fs::remove_dir_all(&data);
}

#[test]
fn a_write_lands_on_the_replaced_file_not_the_dead_inode() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let data = std::env::temp_dir().join(format!("fdrive-fresh-write-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    fs::create_dir_all(&data).unwrap();
    let sdk = Sdk::new("http://127.0.0.1:9").unwrap();
    let adapter = Adapter::new(Arc::new(sdk), rt.handle().clone(), &data).unwrap();

    let path = RelPath::new("doc.txt");
    let backing = adapter.backing(&path);
    fs::create_dir_all(backing.parent().unwrap()).unwrap();
    fs::write(&backing, b"original").unwrap();
    let fh = adapter.opened(&path, true);

    let part = backing.with_extension("part");
    fs::write(&part, b"hydrated").unwrap();
    fs::rename(&part, &backing).unwrap();

    adapter.write(fh, &path, 0, b"edit").unwrap();
    adapter.closed(fh);

    assert_eq!(fs::read(&backing).unwrap(), b"editated");
    let _ = fs::remove_dir_all(&data);
}
