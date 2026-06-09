//! IPC command modules — JSON-file-backed domains served over Tauri.
//! Mirrors the frontend `src/shared/ipc` facade.

pub mod accounts;
pub mod activity;
pub mod bands;
pub mod cafes;
pub mod diagnostics;
pub mod excel;
pub mod log_batches;
pub mod posts;
pub mod queue;
pub mod queue_runner;
pub mod stats;
pub mod stocks;
