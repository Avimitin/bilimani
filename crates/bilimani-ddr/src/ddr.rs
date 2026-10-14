use crate::game::{Chart, Mode};
pub const SP: Mode = Mode("SP");
pub const DP: Mode = Mode("DP");
pub const CHARTS: [Chart; 9] = [
    Chart {
        mode: SP,
        id: "bSP",
    },
    Chart {
        mode: SP,
        id: "BSP",
    },
    Chart {
        mode: SP,
        id: "DSP",
    },
    Chart {
        mode: SP,
        id: "ESP",
    },
    Chart {
        mode: SP,
        id: "CSP",
    },
    Chart {
        mode: DP,
        id: "BDP",
    },
    Chart {
        mode: DP,
        id: "DDP",
    },
    Chart {
        mode: DP,
        id: "EDP",
    },
    Chart {
        mode: DP,
        id: "CDP",
    },
];
pub fn parse_chart(id: &str) -> Option<Chart> {
    CHARTS.iter().copied().find(|c| c.id == id)
}
pub fn native_chart(mode: Mode, difficulty: u32) -> Option<Chart> {
    match (mode, difficulty) {
        (SP, 0..=4) => Some(CHARTS[difficulty as usize]),
        (DP, 1..=4) => Some(CHARTS[difficulty as usize + 4]),
        _ => None,
    }
}
pub mod user {
    use serde::Serialize;
    #[derive(Clone, Debug, Default, Serialize)]
    pub struct UserSnapshot {
        pub players: Vec<Player>,
    }
    #[derive(Clone, Debug, Serialize)]
    pub struct Player {
        pub side: u8,
        pub name: String,
    }
}
