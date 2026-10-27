//! Regions (SPEC §6.4) and the display map (§6.5): exact kNN graph, Leiden fine regions, Ward
//! broad regions, c-TF-IDF labels, multi-seed stability and a deterministic 2-D layout.
pub mod hierarchy;
pub mod knn;
pub mod labels;
pub mod layout;
pub mod leiden;
pub mod map;
pub mod run;
pub mod stability;

pub use map::{MapData, MapPoint, map_data};
pub use run::{NearRegion, RegionOptions, RegionReport, STABLE_AT, nearest_regions, run_regions};
