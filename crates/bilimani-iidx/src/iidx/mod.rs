//! IIDX command vocabulary, shared by its version-specific adapters.
use crate::game::{AvailableChart, Chart, ChartStyle, Mode};
pub const SP: Mode = Mode("SP");
pub const DP: Mode = Mode("DP");
pub const CHARTS: [Chart; 10] = [
    Chart {
        mode: SP,
        id: "SPB",
    },
    Chart {
        mode: SP,
        id: "SPN",
    },
    Chart {
        mode: SP,
        id: "SPH",
    },
    Chart {
        mode: SP,
        id: "SPA",
    },
    Chart {
        mode: SP,
        id: "SPL",
    },
    Chart {
        mode: DP,
        id: "DPB",
    },
    Chart {
        mode: DP,
        id: "DPN",
    },
    Chart {
        mode: DP,
        id: "DPH",
    },
    Chart {
        mode: DP,
        id: "DPA",
    },
    Chart {
        mode: DP,
        id: "DPL",
    },
];
pub fn chart_style(chart: Chart) -> ChartStyle {
    match difficulty(chart) {
        Some(0) => ChartStyle::Green,
        Some(1) => ChartStyle::Blue,
        Some(2) => ChartStyle::Amber,
        Some(3) => ChartStyle::Red,
        Some(4) => ChartStyle::Purple,
        _ => ChartStyle::Neutral,
    }
}
pub fn parse_chart(text: &str) -> Option<Chart> {
    CHARTS
        .iter()
        .copied()
        .find(|c| c.id.eq_ignore_ascii_case(text))
}
pub fn difficulty(chart: Chart) -> Option<u32> {
    CHARTS
        .iter()
        .position(|c| *c == chart)
        .map(|i| (i % 5) as u32)
}
pub fn available_charts(levels: [u8; 10]) -> Vec<AvailableChart> {
    CHARTS
        .into_iter()
        .zip(levels)
        .filter(|(_, level)| *level > 0)
        .map(|(chart, level)| AvailableChart {
            chart,
            level: level.to_string(),
        })
        .collect()
}
pub mod controls;
pub mod navigation;
