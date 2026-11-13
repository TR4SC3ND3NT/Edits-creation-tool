//! Shape layers (`edits_core::shape`) on the renderer.
//!
//! The shapes of a layer at an instant are evaluated in the core
//! (`shape::resolve`), built into kurbo paths (`geom.rs`), changed by their
//! operators (`ops.rs`), stroked by kurbo and rasterised on the CPU by zeno
//! into a frame-sized picture at the output resolution (`raster.rs`). That
//! picture goes to the GPU and is handed back as a quad that covers the
//! frame, like a particles layer's, so masks, effects, the track matte,
//! opacity, blend mode, transitions and motion blur treat a shape layer
//! like any other.
//!
//! Why zeno and not tiny-skia: zeno is already in the build for glyphs,
//! gives exactly what is needed here (anti-aliased coverage of a path,
//! nonzero or even-odd), and the colour work stays ours, in linear light
//! like the rest of the renderer, instead of tiny-skia's sRGB-space
//! blending. Stroking, dashing and offsetting are kurbo's (MIT OR
//! Apache-2.0), so the rasteriser only ever fills.
//!
//! Caching: a layer's picture is kept with everything it depends on (the
//! shapes' values at the instant, the layer's matrix and the output size).
//! A shape that does not change (no keyed value, no moving wiggle or jitter)
//! on a layer that does not move is rasterised once, however many frames
//! show it; [`Renderer::shape_rasterisations`] counts the rasterisations.

pub(crate) mod geom;
pub(crate) mod ops;
mod raster;

use crate::renderer::{Quad, Renderer, Tex};
use anyhow::Result;
use edits_core::shape::{self, ResolvedShape, Shapes};
use edits_core::{Evaluator, Layer};
use glam::DMat4;

/// Pictures of shape layers kept on the GPU.
const CACHED: usize = 8;

/// The pictures of shape layers drawn lately, and how many were rasterised.
#[derive(Default)]
pub(crate) struct ShapeCache {
    /// (layer id, what the picture depends on, picture: `None` when nothing
    /// lands in the frame); most recent last.
    entries: Vec<(String, Vec<u64>, Option<Tex>)>,
    rasterised: u64,
}

impl Renderer {
    /// A shape layer's picture at `t` through `mvp` (the layer's matrix
    /// through the camera), as a quad that covers the frame.
    pub(crate) fn shape_quads(&mut self, ev: &Evaluator, layer: &Layer, shapes: &Shapes, mvp: DMat4, t: f64) -> Result<(Vec<Quad>, bool)> {
        let (w, h) = (self.ctx.w, self.ctx.h);
        let resolved = shape::resolve(ev, layer, shapes, ev.layer_time(layer, t));
        let mut key = ResolvedShape::bits(&resolved);
        key.extend([w as u64, h as u64]);
        key.extend(mvp.to_cols_array().iter().map(|v| v.to_bits()));
        if let Some(i) = self.shapes.entries.iter().position(|(id, k, _)| *id == layer.id && *k == key) {
            let entry = self.shapes.entries.remove(i);
            let tex = entry.2.clone();
            self.shapes.entries.push(entry);
            return Ok((tex.map(Quad::frame).into_iter().collect(), false));
        }
        self.shapes.rasterised += 1;
        let tex = raster::rasterise(&resolved, mvp, w, h).map(|data| self.upload_work(w, h, &data));
        // One picture per layer: the one it had before is out of date.
        if let Some(i) = self.shapes.entries.iter().position(|(id, _, _)| *id == layer.id) {
            let (_, _, old) = self.shapes.entries.remove(i);
            if let Some(old) = old {
                self.forget_texture(old.id);
            }
        }
        self.shapes.entries.push((layer.id.clone(), key, tex.clone()));
        if self.shapes.entries.len() > CACHED {
            if let (_, _, Some(old)) = self.shapes.entries.remove(0) {
                self.forget_texture(old.id);
            }
        }
        Ok((tex.map(Quad::frame).into_iter().collect(), false))
    }

    /// How many times shape layers have been rasterised since the renderer
    /// was made. A still shape on a still layer counts once, however many
    /// frames show it.
    pub fn shape_rasterisations(&self) -> u64 {
        self.shapes.rasterised
    }
}
