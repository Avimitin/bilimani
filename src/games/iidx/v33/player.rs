//! Read-only player snapshot for the verified image. See docs/game-analysis.md.
use crate::profiles::CardId;
use anyhow::{Result, ensure};

const INITIALIZED: usize = 0x10b90e8;
const JOINED: usize = 0xacd79b0;
const STRIDE: usize = 0x3b103a0;
const CARD: usize = 0x6c11808;
const PLAY_TYPE: usize = 0x6c11820;
const DATA_READY: usize = 0x312771e;

pub fn snapshot(
    base: usize,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Option<CardId>> {
    let once = || -> Result<Option<CardId>> {
        let flags = read(base + INITIALIZED, 8)?;
        ensure!(flags.len() == 8, "Short player initialization read");
        if flags[..4] == [0; 4] || flags[4..] == [0; 4] {
            return Ok(None);
        }
        let joined = read(base + JOINED, 8)?;
        ensure!(joined.len() == 8, "Short player participation read");
        let side = match (joined[..4] != [0; 4], joined[4..] != [0; 4]) {
            (true, false) => 0,
            (false, true) => 1,
            _ => return Ok(None),
        };
        let offset = base + side * STRIDE;
        let play_type = read(offset + PLAY_TYPE, 4)?;
        let ready = read(offset + DATA_READY, 1)?;
        ensure!(
            play_type.len() == 4 && ready.len() == 1,
            "Short player status read"
        );
        if play_type == 1u32.to_le_bytes() || ready[0] == 0 {
            return Ok(None);
        }
        let card = read(offset + CARD, 8)?;
        ensure!(card.len() == 8, "Short card read");
        let card = u64::from_le_bytes(card.try_into().unwrap());
        if card == 0 || card == u64::MAX {
            return Ok(None);
        }
        Ok(Some(CardId::parse(&format!("{card:016X}"))?))
    };
    let first = once()?;
    ensure!(first == once()?, "Player changed during snapshot");
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_torn_short_and_unreadable_snapshots() {
        assert!(snapshot(0, |_, _| anyhow::bail!("unreadable")).is_err());
        assert!(snapshot(0, |_, _| Ok(vec![1])).is_err());
        let reads = std::cell::Cell::new(0u64);
        assert!(
            snapshot(0, |address, _| {
                Ok(match address {
                    INITIALIZED => [1u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
                    JOINED => [1u32.to_le_bytes(), 0u32.to_le_bytes()].concat(),
                    PLAY_TYPE => 0u32.to_le_bytes().to_vec(),
                    DATA_READY => vec![1],
                    CARD => {
                        reads.set(reads.get() + 1);
                        (0xE004012345678900 + reads.get()).to_le_bytes().to_vec()
                    }
                    _ => unreachable!(),
                })
            })
            .is_err()
        );
    }
    #[test]
    fn recognizes_both_sides_and_rejects_guests_unready_and_ambiguous_players() {
        for side in 0..2 {
            for (joined, guest, ready, expected) in [
                (1 << side, false, true, true),
                (0, false, true, false),
                (3, false, true, false),
                (1 << side, true, true, false),
                (1 << side, false, false, false),
            ] {
                let result = snapshot(0, |address, size| {
                    Ok(if address == INITIALIZED {
                        [1u32.to_le_bytes(), 1u32.to_le_bytes()].concat()
                    } else if address == JOINED {
                        [
                            u32::from(joined & 1 != 0).to_le_bytes(),
                            u32::from(joined & 2 != 0).to_le_bytes(),
                        ]
                        .concat()
                    } else if address == PLAY_TYPE + side * STRIDE {
                        u32::from(guest).to_le_bytes().to_vec()
                    } else if address == DATA_READY + side * STRIDE {
                        vec![u8::from(ready)]
                    } else if address == CARD + side * STRIDE {
                        0xE0040123456789ABu64.to_le_bytes().to_vec()
                    } else {
                        panic!("unexpected read {address:x} {size}")
                    })
                })
                .unwrap();
                assert_eq!(result.is_some(), expected);
                if let Some(card) = result {
                    assert_eq!(card.as_str(), "E0040123456789AB");
                }
            }
        }
    }
}
