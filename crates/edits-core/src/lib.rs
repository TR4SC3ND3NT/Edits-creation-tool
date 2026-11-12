//! Core of the edits engine: the project document, keyframe evaluation,
//! the effect registry, validation and a text description for agents.
//!
//! This crate has no GPU, windowing or media dependencies, so the same
//! evaluation runs in the renderer, the CLI, the MCP server and tests.

pub mod check;
pub mod describe;
pub mod ease;
pub mod effects;
pub mod eval;
pub mod model;

pub use eval::{Evaluator, ResolvedCamera, ResolvedEffect, ResolvedLayer};
pub use model::*;
