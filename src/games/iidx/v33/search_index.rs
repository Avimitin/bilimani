//! Read-only snapshot of IIDX 33's CMusicTitleDictionary map after its XML load.
//! Layout is tied to the exact game profile; no C++ objects escape the callback.
use anyhow::{Context, Result, ensure};
use std::collections::HashSet;

fn word(bytes: &[u8], offset: usize) -> usize {
    usize::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

pub(super) fn snapshot(
    dictionary: usize,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Vec<(u32, String)>> {
    let map = read(
        dictionary
            .checked_add(8)
            .context("Invalid dictionary pointer")?,
        16,
    )?;
    let head = word(&map, 0);
    let count = word(&map, 8);
    ensure!(
        (1..=100_000).contains(&count),
        "Invalid search index size: {count}"
    );
    let sentinel = read(head, 32)?;
    ensure!(sentinel[25] == 1, "Invalid search index sentinel");
    let mut stack = vec![word(&sentinel, 8)];
    let mut seen = HashSet::with_capacity(count);
    let mut entries = Vec::with_capacity(count);
    while let Some(address) = stack.pop() {
        if address == head {
            continue;
        }
        ensure!(
            seen.len() < count && seen.insert(address),
            "Search index has a cycle or invalid node count"
        );
        let node = read(address, 80)?;
        ensure!(node[25] == 0, "Invalid search index node");
        let len = word(&node, 48);
        let capacity = word(&node, 56);
        ensure!(
            len <= 256 && len <= capacity,
            "Invalid search keyword length: {len}"
        );
        let key = if capacity < 8 {
            node[32..32 + len * 2].to_vec()
        } else {
            read(word(&node, 32), len * 2)?
        };
        let item = read(word(&node, 64), 12)?;
        let id = u32::from_le_bytes(item[8..12].try_into().unwrap());
        ensure!(id < 100_000, "Invalid search music ID: {id}");
        let units: Vec<u16> = key
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        entries.push((
            id,
            String::from_utf16(&units).context("Invalid search keyword UTF-16")?,
        ));
        stack.push(word(&node, 0));
        stack.push(word(&node, 16));
    }
    ensure!(entries.len() == count, "Incomplete native search index");
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    const DICT: usize = 0x100;
    const HEAD: usize = 0x200;
    const ROOT: usize = 0x300;
    const CHILD: usize = 0x400;
    const ITEM: usize = 0x500;
    const TEXT: usize = 0x600;
    fn put(data: &mut [u8], offset: usize, value: usize) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        let mut data = vec![0; 0x800];
        put(&mut data, DICT + 8, HEAD);
        put(&mut data, DICT + 16, 2);
        put(&mut data, HEAD + 8, ROOT);
        data[HEAD + 25] = 1;
        for address in [ROOT, CHILD] {
            put(&mut data, address, HEAD);
            put(&mut data, address + 16, HEAD);
            put(&mut data, address + 64, ITEM);
        }
        put(&mut data, ROOT + 16, CHILD);
        let short: Vec<u8> = "冥".encode_utf16().flat_map(u16::to_le_bytes).collect();
        data[ROOT + 32..ROOT + 34].copy_from_slice(&short);
        put(&mut data, ROOT + 48, 1);
        put(&mut data, ROOT + 56, 7);
        let long: Vec<u8> = "mei another reading"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        data[TEXT..TEXT + long.len()].copy_from_slice(&long);
        put(&mut data, CHILD + 32, TEXT);
        put(&mut data, CHILD + 48, long.len() / 2);
        put(&mut data, CHILD + 56, 31);
        data[ITEM + 8..ITEM + 12].copy_from_slice(&12001u32.to_le_bytes());
        data
    }
    fn capture(data: &[u8]) -> Result<Vec<(u32, String)>> {
        snapshot(DICT, |address, size| {
            data.get(address..address.checked_add(size).context("Overflow")?)
                .map(|b| b.to_vec())
                .context("Unreadable pointer")
        })
    }
    #[test]
    fn snapshots_inline_and_heap_unicode_keys_with_shared_music_ids() {
        let data = fixture();
        let entries = capture(&data).unwrap();
        assert_eq!(
            entries,
            vec![(12001, "冥".into()), (12001, "mei another reading".into())]
        );
    }
    #[test]
    fn rejects_cycles_truncation_and_bad_string_layouts() {
        let mut data = fixture();
        put(&mut data, CHILD, ROOT);
        assert!(capture(&data).is_err());
        let mut data = fixture();
        put(&mut data, DICT + 16, 3);
        assert!(capture(&data).is_err());
        let mut data = fixture();
        put(&mut data, ROOT + 48, 8);
        assert!(capture(&data).is_err());
        let mut data = fixture();
        put(&mut data, CHILD + 32, 0x10000);
        assert!(capture(&data).is_err());
        let mut data = fixture();
        put(&mut data, CHILD + 48, 257);
        assert!(capture(&data).is_err());
    }
}
