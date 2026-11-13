//! Core of the edits engine: the project document, keyframe evaluation,
//! the effect registry, validation, a text description for agents, and the
//! edit operations (with their shared undo history) that the MCP server and
//! the window both use.
//!
//! This crate has no GPU, windowing or media dependencies, so the same
//! evaluation runs in the renderer, the CLI, the MCP server and tests.

pub mod check;
pub mod describe;
pub mod ease;
pub mod edit;
pub mod effects;
pub mod eval;
pub mod jsonfmt;
pub mod model;
pub mod pointer;

pub use eval::{Evaluator, ResolvedCamera, ResolvedEffect, ResolvedLayer};
pub use model::*;
