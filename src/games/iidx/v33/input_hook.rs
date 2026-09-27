//! Recognize the verified input poll or the x64 MinHook relay used by 2dxtra.
//! This only inspects the chain. The vtable wrapper still calls the game entry,
//! allowing the installed plugin and its original trampoline to run normally.
use anyhow::{Context, Result, ensure};

pub const RVA: usize = 0xa7a2f0;
const ORIGINAL: [u8; 16] = [
    0x48, 0x89, 0x4c, 0x24, 0x08, 0x55, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x55, 0x41, 0x56, 0x41,
];

pub fn inspect(
    address: usize,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
    executable: impl Fn(usize) -> bool,
    external_module: impl Fn(usize) -> Option<String>,
) -> Result<Option<String>> {
    let entry = read(address, ORIGINAL.len())?;
    if entry == ORIGINAL {
        return Ok(None);
    }
    // MinHook replaces exactly the first five-byte instruction in this build.
    ensure!(
        entry.len() == ORIGINAL.len() && entry[0] == 0xe9 && entry[5..] == ORIGINAL[5..],
        "Unsupported input entry patch: observed={}",
        hex::encode(&entry)
    );
    let displacement = i32::from_le_bytes(entry[1..5].try_into()?);
    let relay = address
        .checked_add(5)
        .and_then(|next| next.checked_add_signed(displacement as isize))
        .context("Input relay address overflow")?;
    ensure!(
        relay != address && executable(relay),
        "Input relay is not executable"
    );
    let code = read(relay, 14)?;
    ensure!(
        code.len() == 14 && code[..6] == [0xff, 0x25, 0, 0, 0, 0],
        "Unsupported input relay: observed={}",
        hex::encode(&code)
    );
    let target = usize::from_le_bytes(code[6..14].try_into()?);
    ensure!(
        target != address && target != relay && executable(target),
        "Invalid input detour target"
    );
    let module = external_module(target)
        .context("Input detour target is not in a loaded external module")?;
    Ok(Some(module))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ENTRY: usize = 0x10000;
    const RELAY: usize = 0x8000; // Backward rel32, as in a nearby MinHook allocation.
    const TARGET: usize = 0x700000;

    fn patched() -> Vec<u8> {
        let mut entry = ORIGINAL.to_vec();
        entry[0] = 0xe9;
        entry[1..5].copy_from_slice(&((RELAY as isize - ENTRY as isize - 5) as i32).to_le_bytes());
        entry
    }
    fn relay() -> Vec<u8> {
        let mut code = vec![0xff, 0x25, 0, 0, 0, 0];
        code.extend(TARGET.to_le_bytes());
        code
    }
    fn check(
        entry: &[u8],
        relay: &[u8],
        executable: bool,
        module: Option<&str>,
    ) -> Result<Option<String>> {
        inspect(
            ENTRY,
            |address, _| match address {
                ENTRY => Ok(entry.to_vec()),
                RELAY => Ok(relay.to_vec()),
                _ => anyhow::bail!("Unexpected read"),
            },
            |_| executable,
            |_| module.map(str::to_owned),
        )
    }
    #[test]
    fn accepts_native_and_chains_a_minhook_relay() {
        assert_eq!(check(&ORIGINAL, &[], false, None).unwrap(), None);
        assert_eq!(
            check(&patched(), &relay(), true, Some("2dxtra.dll")).unwrap(),
            Some("2dxtra.dll".into())
        );
    }
    #[test]
    fn rejects_unrecognized_patches_short_reads_and_invalid_targets() {
        assert!(check(&[], &[], true, Some("2dxtra.dll")).is_err());
        let mut entry = patched();
        entry[5] ^= 1;
        assert!(check(&entry, &relay(), true, Some("2dxtra.dll")).is_err());
        assert!(check(&patched(), &[0; 14], true, Some("2dxtra.dll")).is_err());
        assert!(check(&patched(), &relay()[..6], true, Some("2dxtra.dll")).is_err());
        assert!(check(&patched(), &relay(), false, Some("2dxtra.dll")).is_err());
        assert!(check(&patched(), &relay(), true, None).is_err());
        let mut cycle = relay();
        cycle[6..].copy_from_slice(&ENTRY.to_le_bytes());
        assert!(check(&patched(), &cycle, true, Some("2dxtra.dll")).is_err());
    }
}
