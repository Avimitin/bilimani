//! Record offsets and chart ordering match 33; the introduction-version bound is 34.
//! Evidence: docs/iidx34-0100917-analysis.md.
use crate::game::{Mode, NowPlaying, SongPhase};
pub use crate::versions::v33::b20260812::song_info::RECORD_SIZE;

pub fn decode(
    record: &[u8],
    mode: Mode,
    difficulties: [Option<u32>; 2],
    phase: SongPhase,
) -> anyhow::Result<NowPlaying> {
    crate::versions::v33::b20260812::song_info::decode_for_version(
        record,
        mode,
        difficulties,
        phase,
        34,
    )
}
