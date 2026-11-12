//! Reference analysis for the edits engine.
//!
//! Given a finished edit, measure its recipe (timing, motion, flashes, tone
//! hits, blur, static layers) independently of the pictures in it, draft a
//! project that replays the recipe, and compare a render with the reference
//! in signature space. See `docs/ANALYSIS.md`.
//!
//! Entry points: [`pipeline::analyze_video`], [`pipeline::analyze_frames`],
//! [`draft::draft`], [`compare::compare`], [`signals::signatures`] (probe).
//! Each stage is also a plain function in its own module.

// Numeric kernels index several parallel buffers at once; iterator chains would hide the maths.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

pub mod beats;
pub mod blur;
pub mod compare;
pub mod curves;
pub mod draft;
pub mod exposure;
pub mod img;
pub mod ingest;
pub mod jsonfmt;
pub mod layers;
pub mod measure;
pub mod media;
pub mod pipeline;
pub mod schema;
pub mod shots;
pub mod signals;
#[doc(hidden)]
pub mod synth;
pub mod tone;
pub mod track;

pub use pipeline::{analyze_frames, analyze_video, AnalyzeOpts};
pub use schema::Analysis;
