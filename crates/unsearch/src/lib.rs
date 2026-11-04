//! inVectus search verdicts and their calibration (SPEC §6.1–6.2). `unsearch` is an internal crate
//! name only; user-facing text says "inVectus search".
pub mod calibrate;
pub mod curve;
pub mod verdict;

pub use calibrate::{
    CalibrateOptions, CalibrationReport, Queries, SampleItem, paraphrase_sample, run_calibration,
    run_calibration_from, screen_paraphrases,
};
pub use curve::Curve;
pub use verdict::{CheckOptions, CheckResult, Verdict, check};
