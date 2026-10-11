//! Head-less re-export surface for the CLI binary.
//!
//! The batch/persist surface used to live here in one 1400-line file; it is
//! now split by concern (`batch`, `paths`, `detect`, `preview`, `report`).
//! The Tauri commands are registered straight from their own modules in
//! `lib.rs`, so this module only re-exports what the head-less CLI consumes.

pub use super::batch::{
    get_image_files_impl, process_batch_with, BatchEvent, BatchStats, EventEmitter, ProgressUpdate,
};
pub use super::report::write_report_csv;
