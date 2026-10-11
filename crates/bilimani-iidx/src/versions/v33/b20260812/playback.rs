//! Read-only chart clock, in the same time origin as density's .1 timestamps.
//! See docs/game-analysis.md. Never call the sequencer or advance a browser timer.
use crate::game::{PlaybackProgress, SongPhase};
use anyhow::{Result, ensure};

pub const SEQUENCE_RVA: usize = 0xa7f1da0;
pub const CURRENT_FRAME: usize = 0x2a29f8;
pub const TIMELINE_RVA: usize = 0xaaad6c0;
pub const END_FRAME: usize = 96;

// Accessors, load/reset, tick, ms-to-frame conversion, end marker and callbacks.
pub const GUARDS: &[(usize, &str)] = &[
    (0x824af0, "488d05a9d2fc09c3cccccccccccccccc"),
    (0x8245c0, "48895c241048896c2418565741544155"),
    (0x824b00, "48895c2420574883ec50488bf9e8ce4c"),
    (0x8268b0, "48895c240848896c2410488974241857"),
    (0x828cd0, "48895c2410488974241848897c242041"),
    (0x8297e0, "488d05d93e280ac3cccccccccccccccc"),
    (0x9336a0, "48895c24084889742410574883ec2049"),
    (0x8d2350, "48895c24084889742410574883ec2049"),
];

/// Called after the native stage callback. Only owned values leave this thread.
pub fn capture(
    base: usize,
    phase: SongPhase,
    read: impl Fn(usize, usize) -> Result<Vec<u8>>,
) -> Result<Option<PlaybackProgress>> {
    if phase != SongPhase::Playing {
        return Ok(None);
    }
    let clock = read(base + SEQUENCE_RVA + CURRENT_FRAME, 28)?;
    let end = read(base + TIMELINE_RVA + END_FRAME, 4)?;
    ensure!(
        clock.len() == 28 && end.len() == 4,
        "Incomplete playback clock"
    );
    let current = i32::from_le_bytes(clock[..4].try_into().unwrap());
    let fps = f32::from_le_bytes(clock[24..28].try_into().unwrap());
    let end = i32::from_le_bytes(end.try_into().unwrap());
    // The FPS captured at chart load is also the rate used to convert events.
    // A separate live calibration value can change at song end; do not use it.
    ensure!(
        fps.is_finite() && (30.0..=1000.0).contains(&fps),
        "Invalid playback rate"
    );
    ensure!(
        end > 0 && current >= 0 && i64::from(current) <= i64::from(end) + 1,
        "Invalid playback frame"
    );
    let duration = f64::from(end) * 1000.0 / f64::from(fps);
    ensure!(duration <= 3_600_000.0, "Invalid playback duration");
    Ok(Some(PlaybackProgress {
        position_ms: (f64::from(current.min(end)) * 1000.0 / f64::from(fps)).round() as u32,
        duration_ms: duration.round() as u32,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(current: i32, end: i32, fps: f32) -> Result<Option<PlaybackProgress>> {
        capture(0x1000, SongPhase::Playing, |address, size| {
            if address == 0x1000 + SEQUENCE_RVA + CURRENT_FRAME {
                assert_eq!(size, 28);
                let mut bytes = vec![0; size];
                bytes[..4].copy_from_slice(&current.to_le_bytes());
                bytes[24..28].copy_from_slice(&fps.to_le_bytes());
                Ok(bytes)
            } else {
                assert_eq!((address, size), (0x1000 + TIMELINE_RVA + END_FRAME, 4));
                Ok(end.to_le_bytes().to_vec())
            }
        })
    }

    #[test]
    fn follows_chart_clock_including_pause_and_retry() {
        let at = |frame| sample(frame, 18000, 120.0).unwrap().unwrap();
        assert_eq!(
            at(0),
            PlaybackProgress {
                position_ms: 0,
                duration_ms: 150000
            }
        );
        assert_eq!(at(600).position_ms, 5000);
        assert_eq!(at(600), at(600)); // No wall clock extrapolation while stopped.
        assert_eq!(at(0).position_ms, 0); // Same-stage retry may move backwards.
        assert_eq!(at(18001).position_ms, 150000);
        assert_eq!(
            sample(600, 7200, 59.94).unwrap().unwrap().position_ms,
            10010
        );
    }

    #[test]
    fn rejects_unloaded_corrupt_or_incomplete_clock() {
        for (current, end, fps) in [
            (0, 0, 120.0),
            (-1, 18000, 120.0),
            (18002, 18000, 120.0),
            (0, 18000, 0.0),
            (0, 18000, f32::NAN),
            (0, 18000, f32::INFINITY),
            (0, i32::MAX, 120.0),
        ] {
            assert!(sample(current, end, fps).is_err());
        }
        assert!(capture(0, SongPhase::Playing, |_, _| Ok(vec![0])).is_err());
        for phase in [SongPhase::Idle, SongPhase::Selecting] {
            assert_eq!(
                capture(0, phase, |_, _| panic!("No reads outside gameplay")).unwrap(),
                None
            );
        }
    }
}
