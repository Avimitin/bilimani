//! Read-only IIDX 33/20260812 player snapshots. Evidence: docs/game-analysis.md.
use super::PARTICIPATION_RVA;
use crate::iidx::user::{
    ActivityCounts, ActivityPeriod, DanRank, StyleProfile, UserProfile, UserSnapshot,
    WeeklyActivity,
};

const ROOT: usize = 0x10b90f0;
const PLAYER: usize = 0x3101488;
const STRIDE: usize = 0x3b103a0;
const ACTIVITY_OBJECT: usize = 0xaab3cb0;
const WEEK_IDS: usize = ROOT + 0x206bc94;
const WEEK_DATES: usize = ROOT + 0x206bca8;
const WEEK_COUNTS: usize = ROOT + 0x206bcbc;
const ACTIVITY_READY: usize = ROOT + 0x206df8c;
const WEEK_READY: usize = ROOT + 0x206c194;
const WEEK_STRIDE: usize = 124;
const STYLE_STRIDE: usize = WEEK_STRIDE * 5;

#[cfg(windows)]
pub const GUARDS: &[(usize, &str)] = &[
    (0x5a64a0, "48895c240848896c2410488974241857"),
    (0x5a6800, "4883ec28e877dc010085c0754d4863c1"),
    (0x5a5650, "4883ec284d63d04c63c2e821ee010085"),
    (0x5a55c0, "4883ec284c63d2e8b4ee010085c07575"),
    (0x661c50, "4863c248ffc04c69c8f40000004963c0"),
    (0x661ca0, "4863c24c69c8f40000004963c0486bd0"),
];

#[derive(Debug, thiserror::Error)]
pub enum SampleError {
    #[error("player memory is unavailable")]
    Unreadable,
    #[error("player snapshot changed while being sampled")]
    Changed,
    #[error("invalid player data")]
    InvalidData,
}

type ReadResult = Result<Vec<u8>, SampleError>;
struct Memory<F> {
    base: usize,
    read: F,
}
impl<F: FnMut(usize, usize) -> ReadResult> Memory<F> {
    fn rva(&mut self, rva: usize, len: usize) -> ReadResult {
        let address = self.base.checked_add(rva).ok_or(SampleError::Unreadable)?;
        (self.read)(address, len)
    }
    fn absolute(&mut self, address: usize, len: usize) -> ReadResult {
        (self.read)(address, len)
    }
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, SampleError> {
    bytes
        .get(at..at + 4)
        .and_then(|v| v.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(SampleError::Unreadable)
}
fn pointer(bytes: &[u8]) -> Result<usize, SampleError> {
    bytes
        .try_into()
        .map(u64::from_le_bytes)
        .ok()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n >= 0x10000)
        .ok_or(SampleError::Unreadable)
}

/// Private identity token is used only to reject cross-login/torn samples.
fn identity(
    memory: &mut Memory<impl FnMut(usize, usize) -> ReadResult>,
    side: usize,
) -> ReadResult {
    let offset = STRIDE * side;
    let initialized = memory.rva(0x10b90e8, 8)?;
    let joined = memory.rva(PARTICIPATION_RVA + 4 * side, 4)?;
    let session = memory.rva(0x6c11808 + offset, 28)?;
    let ready = memory.rva(0x312771e + offset, 1)?;
    if u32_at(&initialized, 0)? == 0
        || u32_at(&initialized, 4)? == 0
        || u32_at(&joined, 0)? == 0
        || u32_at(&session, 24)? != 0
        || session.get(..8).is_none_or(|v| v.iter().all(|b| *b == 0))
        || ready.first() != Some(&1)
    {
        return Err(SampleError::Unreadable);
    }
    Ok(session)
}

fn profile(
    side: usize,
    bytes: &[u8],
    weekly: Option<WeeklyActivity>,
) -> Result<UserProfile, SampleError> {
    let name = bytes.get(0x20..0x28).ok_or(SampleError::Unreadable)?;
    let name = &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())];
    let (dj_name, invalid) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(name);
    if invalid || dj_name.trim().is_empty() || dj_name.chars().any(char::is_control) {
        return Err(SampleError::InvalidData);
    }
    let style = |index: usize| -> Result<StyleProfile, SampleError> {
        let points = u32_at(bytes, 0x34 + index * 4)?;
        if points > i32::MAX as u32 {
            return Err(SampleError::InvalidData);
        }
        Ok(StyleProfile {
            dan: DanRank::from_native(u32_at(bytes, 0x3c + index * 4)? as i32),
            dj_points: points,
        })
    };
    Ok(UserProfile {
        side: side as u8 + 1,
        dj_name: dj_name.into_owned(),
        sp: style(0)?,
        dp: style(1)?,
        weekly,
    })
}

fn weekly(
    memory: &mut Memory<impl FnMut(usize, usize) -> ReadResult>,
    side: usize,
) -> Result<WeeklyActivity, SampleError> {
    let offset = side * STRIDE;
    if memory.rva(ACTIVITY_READY + offset, 1)?.first() != Some(&1)
        || memory.rva(WEEK_READY + offset, 1)?.first() != Some(&1)
    {
        return Err(SampleError::Unreadable);
    }
    let week = memory.rva(WEEK_IDS + offset + 4 * 4, 4)?;
    let date = memory.rva(WEEK_DATES + offset + 4 * 4, 4)?;
    let object_bytes = memory.rva(ACTIVITY_OBJECT, 8)?;
    let object = pointer(&object_bytes)?;
    let delta_address = object
        .checked_add(128 + 244 * side)
        .ok_or(SampleError::Unreadable)?;
    let delta = memory.absolute(delta_address, 244)?;
    let counts = (0..2)
        .map(|style| -> Result<Option<ActivityCounts>, SampleError> {
            let at = WEEK_COUNTS + offset + STYLE_STRIDE * style + WEEK_STRIDE * 4;
            let bytes = memory.rva(at, WEEK_STRIDE)?;
            if bytes.get(120) != Some(&1) {
                return Ok(None);
            }
            let songs = u32_at(&bytes, 0)?;
            let keys = u32_at(&bytes, 112)?;
            let extra_songs = u32_at(&delta, 4 + 120 * style)?;
            let extra_keys = u32_at(&delta, 116 + 120 * style)?;
            if [songs, keys, extra_songs, extra_keys]
                .iter()
                .any(|n| *n > i32::MAX as u32)
            {
                return Err(SampleError::InvalidData);
            }
            if memory.rva(at, WEEK_STRIDE)? != bytes {
                return Err(SampleError::Changed);
            }
            Ok(Some(ActivityCounts {
                songs: u64::from(songs) + u64::from(extra_songs),
                keys: u64::from(keys) + u64::from(extra_keys),
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if memory.absolute(delta_address, 244)? != delta
        || memory.rva(ACTIVITY_OBJECT, 8)? != object_bytes
        || memory.rva(WEEK_IDS + offset + 4 * 4, 4)? != week
    {
        return Err(SampleError::Changed);
    }
    Ok(WeeklyActivity {
        period: ActivityPeriod::GameWeek,
        week_id: u32_at(&week, 0)?,
        date: u32_at(&date, 0)?,
        sp: counts[0],
        dp: counts[1],
    })
}

/// ReadProcessMemory is injected at the boundary; tests use synthetic memory.
pub fn sample(read: impl FnMut(usize, usize) -> ReadResult, base: usize) -> UserSnapshot {
    let mut memory = Memory { base, read };
    let players = (0..2)
        .filter_map(|side| {
            let before = identity(&mut memory, side).ok()?;
            let bytes = memory.rva(PLAYER + STRIDE * side, 0x44).ok()?;
            let activity = weekly(&mut memory, side).ok();
            let user = profile(side, &bytes, activity).ok()?;
            (identity(&mut memory, side).ok()? == before
                && memory.rva(PLAYER + STRIDE * side, 0x44).ok()? == bytes)
                .then_some(user)
        })
        .collect::<Vec<_>>();
    UserSnapshot {
        active_side: players.first().map(|p| p.side),
        players,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    const BASE: usize = 0x180000000;
    const HEAP: usize = 0x100000;
    struct Fixture(BTreeMap<usize, u8>);
    impl Fixture {
        fn set(&mut self, address: usize, bytes: &[u8]) {
            self.0
                .extend(bytes.iter().enumerate().map(|(i, b)| (address + i, *b)));
        }
        fn rva(&mut self, rva: usize, bytes: &[u8]) {
            self.set(BASE + rva, bytes);
        }
        fn read(&self, address: usize, len: usize) -> ReadResult {
            (address..address + len)
                .map(|a| self.0.get(&a).copied().ok_or(SampleError::Unreadable))
                .collect()
        }
        fn new(side: usize) -> Self {
            let mut f = Self(BTreeMap::new());
            let offset = STRIDE * side;
            f.rva(0x10b90e8, &[1, 0, 0, 0, 1, 0, 0, 0]);
            f.rva(PARTICIPATION_RVA + 4 * side, &1u32.to_le_bytes());
            f.rva(0x6c11808 + offset, &[0; 28]);
            f.rva(0x6c11808 + offset, &123u64.to_le_bytes());
            f.rva(0x312771e + offset, &[1]);
            f.rva(PLAYER + offset, &[0; 0x44]);
            f.rva(PLAYER + offset + 0x20, b"PLAYER\0\0");
            f.rva(PLAYER + offset + 0x34, &12345u32.to_le_bytes());
            f.rva(PLAYER + offset + 0x38, &543u32.to_le_bytes());
            f.rva(PLAYER + offset + 0x3c, &18u32.to_le_bytes());
            f.rva(PLAYER + offset + 0x40, &(-1i32).to_le_bytes());
            f.rva(ACTIVITY_READY + offset, &[1]);
            f.rva(WEEK_READY + offset, &[1]);
            f.rva(WEEK_IDS + offset + 16, &77u32.to_le_bytes());
            f.rva(WEEK_DATES + offset + 16, &20261005u32.to_le_bytes());
            f.rva(ACTIVITY_OBJECT, &(HEAP as u64).to_le_bytes());
            f.set(HEAP + 128 + 244 * side, &[0; 244]);
            f.set(HEAP + 132 + 244 * side, &3u32.to_le_bytes());
            f.set(HEAP + 244 + 244 * side, &4567u32.to_le_bytes());
            for style in 0..2 {
                let at = WEEK_COUNTS + offset + style * STYLE_STRIDE + 4 * WEEK_STRIDE;
                f.rva(at, &[0; 124]);
                f.rva(at, &100u32.to_le_bytes());
                f.rva(at + 112, &100_000u32.to_le_bytes());
                f.rva(at + 120, &[1]);
            }
            f
        }
    }
    #[test]
    fn right_side_profile_includes_native_week_and_session_delta() {
        let f = Fixture::new(1);
        let result = sample(|a, n| f.read(a, n), BASE);
        assert_eq!(result.active_side, Some(2));
        let user = &result.players[0];
        assert_eq!(user.dj_name, "PLAYER");
        assert_eq!(user.sp.dan, Some(DanRank::Kaiden));
        assert_eq!(user.dp.dan, None);
        assert_eq!(user.sp.dj_points, 12345);
        let week = user.weekly.as_ref().unwrap();
        assert_eq!(
            week.sp,
            Some(ActivityCounts {
                songs: 103,
                keys: 104567
            })
        );
        assert_eq!(
            week.dp,
            Some(ActivityCounts {
                songs: 100,
                keys: 100000
            })
        );
    }
    #[test]
    fn absent_statistics_are_not_zero_and_guests_have_no_profile() {
        let mut f = Fixture::new(0);
        f.rva(WEEK_READY, &[0]);
        assert!(
            sample(|a, n| f.read(a, n), BASE).players[0]
                .weekly
                .is_none()
        );
        f.rva(0x6c11820, &1u32.to_le_bytes());
        assert!(sample(|a, n| f.read(a, n), BASE).players.is_empty());
        f.rva(0x6c11820, &0u32.to_le_bytes());
        f.rva(PARTICIPATION_RVA, &0u32.to_le_bytes());
        assert!(sample(|a, n| f.read(a, n), BASE).players.is_empty());
    }
    #[test]
    fn reject_card_change_during_read_and_invalid_name() {
        let f = Fixture::new(0);
        let mut card_reads = 0;
        let result = sample(
            |a, n| {
                let mut bytes = f.read(a, n)?;
                if a == BASE + 0x6c11808 {
                    card_reads += 1;
                    if card_reads > 1 {
                        bytes[0] ^= 1;
                    }
                }
                Ok(bytes)
            },
            BASE,
        );
        assert!(result.players.is_empty());
        let mut f = Fixture::new(0);
        f.rva(PLAYER + 0x20, &[0; 8]);
        assert!(sample(|a, n| f.read(a, n), BASE).players.is_empty());
        assert_eq!(DanRank::from_native(19), None);
    }
}
