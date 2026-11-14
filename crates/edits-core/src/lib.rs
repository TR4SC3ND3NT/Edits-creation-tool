//! Core of the edits engine: the project document, keyframe evaluation,
//! the effect registry, validation, a text description for agents, and the
//! edit operations (with their shared undo history) that the MCP server and
//! the window both use.
//!
//! This crate has no GPU, windowing or media dependencies, so the same
//! evaluation runs in the renderer, the CLI, the MCP server and tests.

pub mod activity;
pub mod check;
pub mod comp;
pub mod cube;
pub mod describe;
pub mod ease;
pub mod edit;
pub mod effects;
pub mod eval;
pub mod jsonfmt;
pub mod mask;
pub mod model;
pub mod particles;
pub mod pointer;
pub mod preset;
pub mod shape;
pub mod beatops;
pub mod song;
pub mod text_select;
pub mod time;
pub mod ui;

pub use comp::{Comp, CompLayer};
pub use eval::{ClipInfo, Evaluator, ResolvedCamera, ResolvedEffect, ResolvedLayer, ResolvedTransition, SourceFrame};
pub use mask::{Mask, MaskMode, MaskShape, Matte, MatteMode, ResolvedMask};
pub use model::*;
