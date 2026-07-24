use crate::engine::delta::{copy_map, missing_ranges};

fn data(size: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..size)
        .map(|_| {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            (x >> 24) as u8
        })
        .collect()
}

fn diff_of(server: &[u8], local: &[u8]) -> Vec<u8> {
    let sig = fast_rsync::Signature::calculate(
        server,
        fast_rsync::SignatureOptions {
            block_size: 4096,
            crypto_hash_size: 16,
        },
    );
    let mut delta = Vec::new();
    fast_rsync::diff(&sig.index(), local, &mut delta).unwrap();
    delta
}

#[test]
fn identical_files_have_no_missing_ranges() {
    let server = data(256 * 1024, 1);
    let copies = copy_map(&diff_of(&server, &server)).unwrap();
    assert_eq!(missing_ranges(&copies, server.len() as u64, 0), vec![]);
}

#[test]
fn an_edit_maps_to_its_blocks() {
    let server_size = 256 * 1024;
    let mut local = data(server_size, 1);
    let server = {
        let mut s = local.clone();
        s[100 * 1024..104 * 1024].copy_from_slice(&data(4 * 1024, 9));
        s
    };
    let copies = copy_map(&diff_of(&server, &local)).unwrap();
    let missing = missing_ranges(&copies, server_size as u64, 0);
    let missing_bytes: u64 = missing.iter().map(|(a, b)| b - a).sum();
    assert!(missing_bytes >= 4 * 1024 && missing_bytes <= 12 * 1024, "{missing:?}");
    for (start, end) in &missing {
        assert!(*start >= 96 * 1024 && *end <= 108 * 1024, "{missing:?}");
    }
}

#[test]
fn unrelated_files_are_fully_missing() {
    let server = data(64 * 1024, 1);
    let local = data(64 * 1024, 200);
    let copies = copy_map(&diff_of(&server, &local)).unwrap();
    let missing = missing_ranges(&copies, server.len() as u64, 0);
    let missing_bytes: u64 = missing.iter().map(|(a, b)| b - a).sum();
    assert_eq!(missing_bytes, server.len() as u64);
}

#[test]
fn nearby_gaps_are_merged() {
    let copies = vec![(0, 0, 4096), (8192, 4096, 4096), (32768, 8192, 16384)];
    let merged = missing_ranges(&copies, 131072, 8192);
    assert_eq!(merged, vec![(4096, 32768), (49152, 131072)]);
    let exact = missing_ranges(&copies, 131072, 0);
    assert_eq!(exact, vec![(4096, 8192), (12288, 32768), (49152, 131072)]);
}

#[test]
fn garbage_is_rejected() {
    assert!(copy_map(b"garbage").is_none());
    assert!(copy_map(&[0x72, 0x73, 0x02, 0x36, 0x99]).is_none());
    assert!(copy_map(&[0x72, 0x73, 0x02, 0x36, 0x45, 0x00]).is_none());
}

#[test]
fn reassembly_from_copies_and_ranges_is_exact() {
    let server = data(300 * 1024, 3);
    let mut local = server.clone();
    local[200 * 1024..201 * 1024].fill(0xAA);
    local.extend_from_slice(&data(10 * 1024, 7));
    let copies = copy_map(&diff_of(&server, &local)).unwrap();
    let missing = missing_ranges(&copies, server.len() as u64, 16 * 1024);
    let mut out = vec![0u8; server.len()];
    for (s_off, l_off, len) in &copies {
        let (s, l, n) = (*s_off as usize, *l_off as usize, *len as usize);
        if s + n <= out.len() {
            out[s..s + n].copy_from_slice(&local[l..l + n]);
        }
    }
    for (start, end) in &missing {
        let (a, b) = (*start as usize, *end as usize);
        out[a..b].copy_from_slice(&server[a..b]);
    }
    assert_eq!(out, server);
}
