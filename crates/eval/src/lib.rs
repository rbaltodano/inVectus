//! Evaluation (SPEC §9): the backtest harness that checks whether ranked gaps were later filled.
pub mod backtest;
pub mod tune;

pub use backtest::{BacktestOptions, BacktestReport, CutoffResult, RankScore, backtest};
pub use tune::{TuneReport, tune};
