//! Headless GPU renderer for the edits engine.
//!
//! ```no_run
//! use edits_render::{RenderOptions, Renderer};
//! # fn main() -> anyhow::Result<()> {
//! let text = std::fs::read_to_string("project.json")?;
//! let project = edits_core::Project::from_json(&text)?;
//! let mut renderer = Renderer::new(RenderOptions::default())?;
//! renderer.prepare(&project, std::path::Path::new("."))?;
//! let frame = renderer.render_frame(0, 1.0)?; // straight sRGB RGBA8
//! frame.save("frame.png")?;
//! # Ok(()) }
//! ```
//!
//! Everything is offscreen: no window or display server is needed, and a
//! software Vulkan driver (Mesa lavapipe) is enough. A frame is a pure
//! function of the project and the frame index, so the same input gives the
//! same bytes on the same adapter.

mod assets;
mod compose;
mod fx;
mod geometry;
mod gpu;
mod renderer;
mod transitions;

pub mod font;
pub mod shader;

pub use assets::placeholder;
pub use gpu::{policy_from_env, AdapterInfo, AdapterPolicy};
pub use renderer::{FrameStats, PassTiming, RenderOptions, Renderer};
