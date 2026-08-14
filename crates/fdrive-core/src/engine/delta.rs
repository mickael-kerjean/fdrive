const DELTA_MAGIC: u32 = 0x72730236;

fn num(delta: &[u8], at: &mut usize, width: usize) -> Option<u64> {
    let bytes = delta.get(*at..at.checked_add(width)?)?;
    *at += width;
    Some(bytes.iter().fold(0u64, |v, b| v << 8 | *b as u64))
}

fn skip(at: &mut usize, len: u64) -> Option<()> {
    *at = at.checked_add(usize::try_from(len).ok()?)?;
    Some(())
}

pub(super) fn copy_map(delta: &[u8]) -> Option<Vec<(u64, u64, u64)>> {
    let mut at = 0usize;
    if num(delta, &mut at, 4)? != DELTA_MAGIC as u64 {
        return None;
    }
    let mut copies = Vec::new();
    let mut pos = 0u64;
    loop {
        if at >= delta.len() {
            return None;
        }
        let op = delta[at];
        at += 1;
        match op {
            0x00 => break,
            0x01..=0x40 => {
                skip(&mut at, op as u64)?;
                pos += op as u64;
            }
            0x41..=0x44 => {
                let len = num(delta, &mut at, 1 << (op - 0x41))?;
                skip(&mut at, len)?;
                pos += len;
            }
            0x45..=0x54 => {
                let i = op - 0x45;
                let start = num(delta, &mut at, 1 << (i / 4))?;
                let len = num(delta, &mut at, 1 << (i % 4))?;
                copies.push((start, pos, len));
                pos += len;
            }
            _ => return None,
        }
    }
    Some(copies)
}

pub(super) fn missing_ranges(
    copies: &[(u64, u64, u64)],
    size: u64,
    merge_gap: u64,
) -> Vec<(u64, u64)> {
    let mut have: Vec<(u64, u64)> = copies
        .iter()
        .map(|(start, _, len)| (*start, (start + len).min(size)))
        .collect();
    have.sort_unstable();
    let mut missing = Vec::new();
    let mut cursor = 0u64;
    for (start, end) in have {
        if start > cursor {
            missing.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < size {
        missing.push((cursor, size));
    }
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (start, end) in missing {
        match merged.last_mut() {
            Some((_, last)) if start - *last <= merge_gap => *last = end,
            _ => merged.push((start, end)),
        }
    }
    merged
}
