//! Masks and track mattes on the GPU (see `edits_core::mask` for what they
//! mean, and `shaders/mask.wgsl`).
//!
//! A mask is evaluated per pixel as a signed distance in the layer's plane:
//! each pixel of the frame is mapped back onto the layer through the inverse
//! of the layer's projection (a homography, so it works for layers turned in
//! 3D too), then into the mask's own frame. One full-frame pass per mask
//! combines its coverage with the coverage so far; the last one multiplies
//! the layer's picture. A matte is one more pass that multiplies the picture
//! by the matte source's alpha or luminance.

use crate::renderer::{Pipe, Renderer, Tex};
use edits_core::{MaskMode, MaskShape, MatteMode, ResolvedMask};
use glam::{DMat3, DMat4, DVec3};

/// Uniforms of one mask pass (see `shaders/mask.wgsl`).
fn mask_uniforms(m: &ResolvedMask, to_layer: &[[f32; 4]; 3], before: f32) -> [[f32; 4]; 24] {
    let mut p = [[0.0f32; 4]; 24];
    p[..3].copy_from_slice(to_layer);
    let shape = match m.shape {
        MaskShape::Rect | MaskShape::RoundedRect => 0.0,
        MaskShape::Ellipse => 1.0,
        MaskShape::Polygon => 2.0,
    };
    let mode = match m.mode {
        MaskMode::Add => 0.0,
        MaskMode::Subtract => 1.0,
        MaskMode::Intersect => 2.0,
        MaskMode::Difference => 3.0,
    };
    p[3] = [shape, mode, if m.inverted { 1.0 } else { 0.0 }, m.opacity as f32];
    let n = m.points.len().min(edits_core::mask::MAX_POLYGON_POINTS);
    p[4] = [before, n as f32, 0.0, 0.0];
    let (sn, cs) = m.rotation.to_radians().sin_cos();
    p[5] = [m.position[0] as f32, m.position[1] as f32, cs as f32, sn as f32];
    let radius = if m.shape == MaskShape::RoundedRect { m.radius } else { 0.0 };
    p[6] = [(m.size[0].abs() * 0.5) as f32, (m.size[1].abs() * 0.5) as f32, radius as f32, 0.0];
    p[7] = [m.feather as f32, m.expansion as f32, 0.0, 0.0];
    for (i, pt) in m.points.iter().take(n).enumerate() {
        let v = &mut p[8 + i / 2];
        let o = (i % 2) * 2;
        v[o] = pt[0] as f32;
        v[o + 1] = pt[1] as f32;
    }
    p
}

/// The rows of the map from the frame (NDC x, y, 1) to the plane z = 0 of
/// layer space (homogeneous: divide by the third value, which is positive in
/// front of the camera). `mvp` takes layer space to clip space. A layer seen
/// edge-on maps every pixel behind the camera.
pub(crate) fn frame_to_layer(mvp: DMat4) -> [[f32; 4]; 3] {
    let (c0, c1, c3) = (mvp.col(0), mvp.col(1), mvp.col(3));
    let h = DMat3::from_cols(DVec3::new(c0.x, c0.y, c0.w), DVec3::new(c1.x, c1.y, c1.w), DVec3::new(c3.x, c3.y, c3.w));
    let scale = h.x_axis.length() * h.y_axis.length() * h.z_axis.length();
    let det = h.determinant();
    if !det.is_finite() || det.abs() <= 1e-9 * scale {
        return [[0.0; 4], [0.0; 4], [0.0, 0.0, -1.0, 0.0]];
    }
    let inv = h.inverse();
    // A positive scale keeps the sign of the third value and keeps f32 happy.
    let big = inv.to_cols_array().iter().fold(0.0f64, |a, v| a.max(v.abs())).max(1e-300);
    let row = |i: usize| {
        let r = inv.row(i) / big;
        [r.x as f32, r.y as f32, r.z as f32, 0.0]
    };
    [row(0), row(1), row(2)]
}

impl Renderer {
    /// `src` multiplied by the combined coverage of `masks` (none: `src`
    /// itself). `mvp` takes the layer's space to clip space. `src` is
    /// released when a new texture is returned.
    pub(crate) fn apply_masks(&mut self, src: Tex, masks: &[ResolvedMask], mvp: DMat4) -> Tex {
        let Some(first) = masks.first() else { return src };
        let to_layer = frame_to_layer(mvp);
        let (w, h) = (src.w, src.h);
        let white = self.white.clone();
        let mut so_far: Option<Tex> = None;
        for (i, m) in masks.iter().enumerate() {
            let last = i + 1 == masks.len();
            let before = if i == 0 { MaskMode::start(first.mode) as f32 } else { -1.0 };
            let p = mask_uniforms(m, &to_layer, before);
            let out = self.acquire(w, h);
            let base = if last { &src } else { &white };
            self.pass(Pipe::Pass("fs_mask"), &out, false, base, so_far.as_ref(), &p);
            if let Some(old) = so_far.replace(out) {
                self.release(old);
            }
        }
        self.release(src);
        so_far.expect("at least one mask")
    }

    /// `content` multiplied by a matte: `matte` is the source's picture (or
    /// `None` when it shows nothing at this instant) and `opacity` its
    /// opacity. `content` (and `matte`) are released.
    pub(crate) fn apply_matte(&mut self, content: Tex, matte: Option<Tex>, mode: MatteMode, opacity: f32) -> Tex {
        let inverted = matches!(mode, MatteMode::AlphaInverted | MatteMode::LumaInverted);
        if matte.is_none() && inverted {
            // Nothing to cut out.
            return content;
        }
        let code = match mode {
            MatteMode::Alpha => 0.0,
            MatteMode::AlphaInverted => 1.0,
            MatteMode::Luma => 2.0,
            MatteMode::LumaInverted => 3.0,
        };
        // Without a picture the source counts as transparent: opacity 0.
        let opacity = if matte.is_some() { opacity } else { 0.0 };
        let out = self.acquire(content.w, content.h);
        let source = matte.clone().unwrap_or_else(|| content.clone());
        self.pass(Pipe::Pass("fs_matte"), &out, false, &content, Some(&source), &[[code, opacity, 0.0, 0.0]]);
        self.release(content);
        if let Some(m) = matte {
            self.release(m);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry;
    use edits_core::{ResolvedCamera, ResolvedLayer};

    #[test]
    fn frame_pixels_map_back_onto_the_layer() {
        let cam = ResolvedCamera { fov: 39.6, position: [0.0; 3], rotation: [0.0; 3] };
        let vp = geometry::view_projection(&cam, 16.0 / 9.0);
        let layer = ResolvedLayer { position: [0.1, -0.05, 0.3], scale: [0.5, 0.7, 1.0], rotation: [20.0, -30.0, 15.0], anchor: [0.02, 0.0], opacity: 1.0 };
        let mvp = vp * geometry::model(&layer);
        let rows = frame_to_layer(mvp);
        for p in [[0.0, 0.0], [0.3, -0.2], [-0.4, 0.25]] {
            let c = mvp * glam::DVec4::new(p[0], p[1], 0.0, 1.0);
            let ndc = [c.x / c.w, c.y / c.w, 1.0];
            let h: Vec<f64> = rows.iter().map(|r| (0..3).map(|i| r[i] as f64 * ndc[i]).sum()).collect();
            assert!(h[2] > 0.0, "in front of the camera");
            assert!((h[0] / h[2] - p[0]).abs() < 1e-4 && (h[1] / h[2] - p[1]).abs() < 1e-4, "{p:?} -> {h:?}");
        }
        // Edge-on: nothing of the layer can be seen.
        let edge = ResolvedLayer { position: [0.0; 3], rotation: [0.0, 90.0, 0.0], ..layer };
        assert_eq!(frame_to_layer(vp * geometry::model(&edge))[2][2], -1.0);
    }
}
