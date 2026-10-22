//! Read-only snapshots of IIDX's lane transform. See docs/iidx34-0100917-analysis.md.
//! Never call 80efa0: its S-RANDOM/BEGINNER branches consume RNG and mutate state.
use crate::game::{LaneOrder, LaneOrderStatus, NowPlaying, RandomMode, SongPhase};
use anyhow::{Result, ensure};

pub const TABLE_RVA: usize = 0xb1259a0; // Lane transform singleton +16.
pub const GENERATOR_RVA: usize = 0x80fba0;
pub const OPTIONS_RVA: usize = 0xb3e9f28;
pub const OPTIONS_VTABLE: usize = 0xd23de0;
pub const CONTEXT_RVA: usize = 0xb60b3a0;
const ASSIST_RVA: usize = 0xb3ea090;
pub const OPTIONS_SIZE: usize = 572;

// File evidence for generator, consuming lookup, options and post-transforms.
// Runtime sampling tolerates a patched generator: only its output is read.
pub const GUARDS: &[(usize, &str)] = &[
    (0x80f970, "488d051960910ac3cccccccccccccccc"),
    (0x80efe0, "48896c2410488974241848897c242041"),
    (0x80fba0, "4c8bdc55535657488bec4883ec78488b"),
    (0x871950, "488b05d185b70ac3cccccccccccccccc"),
    (0x8846c0, "40534883ec204863da8bd3e890ffffff"),
    (0x884660, "40534883ec20488bd9e8c2ffffff4863"),
    (0x884630, "4183f8017505418d4001c34585c07508"),
    (0x80f900, "40574883ec208bfae893b0000084c074"),
    (0x884ba0, "40574883ec208bfae8b3faffff83b898"),
    (0x91a3d0, "48895c24084889742410574883ec2049"),
    (0x8beb00, "48895c24084889742410574883ec2049"),
];

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn pointer(bytes: &[u8]) -> usize {
    u64::from_le_bytes(bytes.try_into().unwrap()) as usize
}

/// Native table is source -> destination; OBS needs source at each destination.
pub fn invert(table: &[u8], reflect: bool) -> Result<[u8; 7]> {
    ensure!(table.len() == 32, "Incomplete lane table");
    ensure!(word(table, 28) == 7, "Scratch is not fixed");
    let mut keys = [0; 7];
    for source in 0..7 {
        let destination = word(table, source * 4) as usize;
        ensure!(destination < 7, "Lane outside keyboard");
        let destination = if reflect {
            6 - destination
        } else {
            destination
        };
        ensure!(keys[destination] == 0, "Duplicate lane");
        keys[destination] = source as u8 + 1;
    }
    Ok(keys)
}

/// Call only after the original scene callback, never from HTTP or a worker.
/// Selection exposes OFF/MIRROR immediately; random layouts wait for stage load.
pub fn capture(
    base: usize,
    phase: SongPhase,
    players: &[crate::game::PlayerChart],
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Vec<LaneOrder>> {
    if phase == SongPhase::Idle || players.is_empty() {
        return Ok(Vec::new());
    }
    let bounded = |at, size| -> Result<Vec<u8>> {
        let bytes = read(at, size)?;
        ensure!(bytes.len() == size, "Incomplete lane snapshot");
        Ok(bytes)
    };
    let options = pointer(&bounded(base + OPTIONS_RVA, 8)?);
    ensure!(options != 0, "Options unavailable");
    let options = bounded(options, OPTIONS_SIZE)?;
    ensure!(
        pointer(&options[..8]) == base + OPTIONS_VTABLE,
        "Unknown options layout"
    );
    let context = bounded(base + CONTEXT_RVA, 24)?;
    let double_play = word(&context, 4) != 0;
    let joined = [word(&context, 16), word(&context, 20)];
    ensure!(joined.iter().all(|&n| n <= 1), "Invalid joined sides");
    let single = joined.iter().sum::<u32>() == 1 || double_play;
    let playing = phase == SongPhase::Playing;
    // 0xb3e7e2e also becomes 1 during successful sound loading and normal
    // stage transitions (8faeb0/8fb290). It cannot gate map availability.
    // Scene lifetime and validation of the current table determine readiness.
    let table = if playing {
        bounded(base + TABLE_RVA, 64).ok()
    } else {
        None
    };
    let mut result = Vec::new();
    for side in 0..2 {
        if !double_play && !players.iter().any(|p| usize::from(p.side) == side + 1) {
            continue;
        }
        // 884620: DP uses block 2; SP uses its own side's block.
        let block = 8 + 188 * if double_play { 2 } else { side };
        let random = match word(&options, block + 48 + side * 4) {
            0 => RandomMode::Off,
            1 => RandomMode::Random,
            2 => RandomMode::RRandom,
            3 => RandomMode::SRandom,
            _ => continue,
        };
        let mirror = word(&options, block + 56 + side * 4);
        let battle_reflect = word(&options, block + 152);
        if mirror > 1 || battle_reflect > 1 {
            continue;
        }
        let mut lane = LaneOrder {
            side: side as u8 + 1,
            random,
            mirror: mirror != 0,
            status: LaneOrderStatus::Pending,
            keys: None,
        };
        if random == RandomMode::SRandom {
            lane.status = LaneOrderStatus::Dynamic;
        } else if !playing {
            if random == RandomMode::Off {
                lane.keys = Some(if lane.mirror {
                    [7, 6, 5, 4, 3, 2, 1]
                } else {
                    [1, 2, 3, 4, 5, 6, 7]
                });
                lane.status = LaneOrderStatus::Ready;
            }
        } else {
            // BEGINNER's optional helper has its own per-note transform. A set
            // helper flag is conservatively unavailable, never a fake fixed map.
            let beginner = !double_play
                && players
                    .iter()
                    .any(|p| usize::from(p.side) == side + 1 && p.chart.id == "SPB");
            let assist_off = !beginner
                || (|| -> Result<bool> {
                    let assist = pointer(&bounded(base + ASSIST_RVA, 8)?);
                    ensure!(assist != 0, "Assist state unavailable");
                    Ok(bounded(assist + 8 + side, 1)?[0] == 0)
                })()
                .unwrap_or(false);
            // 80efa0 applies these reflections after the generated table.
            // Ordinary MIRROR is already baked in by 80fb60; do not apply twice.
            let special = matches!(word(&context, 0), 7 | 8) && single && side == 1 && !double_play;
            let battle = battle_reflect == 1 && !single && !double_play;
            let reflect = random != RandomMode::Off && (special ^ battle);
            lane.keys = table
                .as_ref()
                .filter(|_| assist_off)
                .and_then(|t| invert(&t[side * 32..side * 32 + 32], reflect).ok());
            lane.status = if lane.keys.is_some() {
                LaneOrderStatus::Ready
            } else {
                LaneOrderStatus::Unavailable
            };
        }
        result.push(lane);
    }
    Ok(result)
}

pub fn apply(
    base: usize,
    playing: &mut NowPlaying,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) {
    playing.lane_order = capture(base, playing.phase, &playing.players, read).unwrap_or_default();
}
