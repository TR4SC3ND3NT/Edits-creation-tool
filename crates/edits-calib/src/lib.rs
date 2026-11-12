//! Round-trip calibration for the edits engine, with the renderer as ground truth.
//!
//! - [`recipe`]: random recipes of this genre as `edits_core::Project` values
//!   with known parameters, over procedurally generated pictures ([`pictures`]);
//! - [`render`]: in-process rendering into analysis frames, and a
//!   platform-like degradation (H.264 at a low bitrate);
//! - [`score`] and [`report`]: every `edits-analyze` estimator against the
//!   truth: bias, spread, hit and miss rates by picture kind and condition;
//! - [`harness`]: the whole loop, the worst cases saved for replay, and the
//!   draft round trip (a project drafted from an analysis must reproduce it).
//!
//! See `docs/CALIBRATION.md`.

#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod harness;
pub mod pictures;
pub mod recipe;
pub mod render;
pub mod report;
pub mod rng;
pub mod score;

pub use harness::{replay, run, run_case, RunOpts};
pub use recipe::{Case, CaseSpec};
pub use render::{Degrade, Stage};
pub use report::Report;
