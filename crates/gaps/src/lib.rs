//! Gap scan (SPEC §5): coverage gaps, stale coverage and bridge gaps over the latest regions,
//! with momentum, saturation, z-scored composite scores, explanations and human verdicts.
pub mod export;
pub mod momentum;
pub mod review;
pub mod scan;

pub use review::{GapDetail, GapList, GapView, detail, list, set_verdict};
pub use scan::{Gap, GapKind, ScanOptions, ScanReport, scan};
