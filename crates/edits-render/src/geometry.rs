//! Camera, layer and cube-face matrices, and the fit of a picture into a box.
//!
//! Space: units of composition height, origin at the centre of the frame,
//! x right, y down, z away from the viewer. Rotations are degrees applied X,
//! then Y, then Z, with the standard right-handed matrices in that space:
//! +Z turns clockwise on screen, +Y brings the right-hand side toward the
//! viewer, +X brings the top toward the viewer.

use edits_core::{Fit, ResolvedCamera, ResolvedLayer};
use glam::{DMat4, DVec3};

const NEAR: f64 = 0.02;
const FAR: f64 = 200.0;

fn rotation(deg: [f64; 3]) -> DMat4 {
    DMat4::from_rotation_z(deg[2].to_radians()) * DMat4::from_rotation_y(deg[1].to_radians()) * DMat4::from_rotation_x(deg[0].to_radians())
}

/// World space -> clip space. The plane z = 0 fills the frame exactly when the
/// camera is at rest, whatever the field of view.
pub fn view_projection(cam: &ResolvedCamera, aspect: f64) -> DMat4 {
    let f = 1.0 / (cam.fov.to_radians() * 0.5).tan();
    let dist = 0.5 * f;
    let eye = DVec3::new(cam.position[0], cam.position[1], cam.position[2] - dist);
    let pose = DMat4::from_translation(eye) * rotation(cam.rotation);
    let view = pose.inverse();
    // Camera space is x right, y down, z forward; clip space has y up and depth 0..1.
    let proj = DMat4::from_cols_array(&[
        f / aspect, 0.0, 0.0, 0.0, //
        0.0, -f, 0.0, 0.0, //
        0.0, 0.0, FAR / (FAR - NEAR), 1.0, //
        0.0, 0.0, -FAR * NEAR / (FAR - NEAR), 0.0,
    ]);
    proj * view
}

/// Layer space -> world space: anchor, scale, rotate X, Y, Z, translate.
pub fn model(l: &ResolvedLayer) -> DMat4 {
    DMat4::from_translation(DVec3::from(l.position))
        * rotation(l.rotation)
        * DMat4::from_scale(DVec3::from(l.scale))
        * DMat4::from_translation(DVec3::new(-l.anchor[0], -l.anchor[1], 0.0))
}

/// The unit quad (-0.5..0.5, facing -z) stretched to a `w` x `h` box.
pub fn flat(w: f64, h: f64) -> DMat4 {
    DMat4::from_scale(DVec3::new(w, h, 1.0))
}

/// The six faces of a box in the order front, right, back, left, top, bottom.
/// Each entry places the unit quad on that face so that, seen from outside,
/// the picture is upright (for top and bottom: upright once the face has been
/// turned to the front about the X axis). The second value is the face's aspect.
pub fn cube_faces(size: [f64; 3]) -> [(DMat4, f64); 6] {
    let [w, h, d] = size;
    let face = |right: DVec3, down: DVec3, centre: DVec3| {
        DMat4::from_cols(right.extend(0.0), down.extend(0.0), right.cross(down).normalize_or_zero().extend(0.0), centre.extend(1.0))
    };
    let (x, y, z) = (DVec3::X, DVec3::Y, DVec3::Z);
    [
        (face(x * w, y * h, -z * d * 0.5), w / h),
        (face(z * d, y * h, x * w * 0.5), d / h),
        (face(-x * w, y * h, z * d * 0.5), w / h),
        (face(-z * d, y * h, -x * w * 0.5), d / h),
        (face(x * w, -z * d, -y * h * 0.5), w / d),
        (face(x * w, z * d, y * h * 0.5), w / d),
    ]
}

/// True when the unit quad drawn with `mvp` covers every pixel centre of a
/// `w` x `h` frame, so none of its edges can be seen.
pub fn covers_frame(mvp: DMat4, w: u32, h: u32) -> bool {
    let mut corners = [[0.0f64; 2]; 4];
    for (i, (x, y)) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)].into_iter().enumerate() {
        let c = mvp * DVec3::new(x, y, 0.0).extend(1.0);
        // A corner behind the near plane would be clipped: not a simple quad on screen.
        if c.w <= 1e-9 || c.z < 0.0 || c.z > c.w {
            return false;
        }
        corners[i] = [c.x / c.w, c.y / c.w];
    }
    // The outermost pixel centres are at 1 - 1/size in clip space; test a little beyond them.
    let (ex, ey) = (1.0 - 0.5 / w as f64, 1.0 - 0.5 / h as f64);
    [(-ex, -ey), (ex, -ey), (ex, ey), (-ex, ey)].into_iter().all(|(px, py)| {
        let side = |i: usize| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])
        };
        (0..4).all(|i| side(i) >= 0.0) || (0..4).all(|i| side(i) <= 0.0)
    })
}

/// How box coordinates (0..1) map into picture coordinates: `offset + uv * scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FitUv {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    /// True when the picture does not cover the box and must be cut at its edges.
    pub cut: bool,
}

pub fn fit_uv(fit: Fit, focus: [f64; 2], image_aspect: f64, box_aspect: f64) -> FitUv {
    let (mut off, mut scale, mut cut) = ([0.0, 0.0], [1.0, 1.0], false);
    let wider = image_aspect > box_aspect;
    match fit {
        Fit::Stretch => {}
        Fit::Cover => {
            // The visible part of the picture, kept as close to `focus` as its edges allow.
            let axis = if wider { 0 } else { 1 };
            let part = if wider { box_aspect / image_aspect } else { image_aspect / box_aspect };
            scale[axis] = part;
            off[axis] = (focus[axis] - part * 0.5).clamp(0.0, 1.0 - part);
        }
        Fit::Contain => {
            let axis = if wider { 1 } else { 0 };
            let span = if wider { image_aspect / box_aspect } else { box_aspect / image_aspect };
            scale[axis] = span;
            off[axis] = 0.5 - span * 0.5;
            cut = true;
        }
    }
    FitUv { offset: [off[0] as f32, off[1] as f32], scale: [scale[0] as f32, scale[1] as f32], cut }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(m: DMat4, p: [f64; 3]) -> [f64; 3] {
        let c = m * DVec3::from(p).extend(1.0);
        [c.x / c.w, c.y / c.w, c.z / c.w]
    }

    #[test]
    fn plane_z0_fills_the_frame_at_any_fov() {
        for fov in [20.0, 39.6, 90.0] {
            let cam = ResolvedCamera { fov, position: [0.0; 3], rotation: [0.0; 3] };
            let vp = view_projection(&cam, 16.0 / 9.0);
            let tl = project(vp, [-8.0 / 9.0, -0.5, 0.0]);
            let br = project(vp, [8.0 / 9.0, 0.5, 0.0]);
            assert!((tl[0] + 1.0).abs() < 1e-9 && (tl[1] - 1.0).abs() < 1e-9, "{tl:?}");
            assert!((br[0] - 1.0).abs() < 1e-9 && (br[1] + 1.0).abs() < 1e-9, "{br:?}");
            assert!(tl[2] > 0.0 && tl[2] < 1.0);
        }
    }

    #[test]
    fn farther_layers_are_smaller_and_deeper() {
        let cam = ResolvedCamera { fov: 39.6, position: [0.0; 3], rotation: [0.0; 3] };
        let vp = view_projection(&cam, 1.0);
        let near = project(vp, [0.5, 0.0, 0.0]);
        let far = project(vp, [0.5, 0.0, 1.0]);
        assert!(far[0] < near[0] && far[0] > 0.0);
        assert!(far[2] > near[2]);
    }

    #[test]
    fn rotation_conventions() {
        let l = |rotation| ResolvedLayer { position: [0.0; 3], scale: [1.0; 3], rotation, anchor: [0.0; 2], opacity: 1.0 };
        // +Z: the right-hand point moves down the screen (clockwise).
        let p = model(&l([0.0, 0.0, 90.0])) * DVec3::X.extend(1.0);
        assert!((p.y - 1.0).abs() < 1e-9);
        // +Y: the right-hand point comes toward the viewer.
        let p = model(&l([0.0, 90.0, 0.0])) * DVec3::X.extend(1.0);
        assert!((p.z + 1.0).abs() < 1e-9);
        // +X: the top comes toward the viewer.
        let p = model(&l([90.0, 0.0, 0.0])) * (-DVec3::Y).extend(1.0);
        assert!((p.z + 1.0).abs() < 1e-9);
    }

    #[test]
    fn full_frame_quads_are_recognised() {
        let cam = ResolvedCamera { fov: 39.6, position: [0.0; 3], rotation: [0.0; 3] };
        let vp = view_projection(&cam, 1.0);
        let l = |scale: f64, rotation: [f64; 3], position: [f64; 3]| {
            vp * model(&ResolvedLayer { position, scale: [scale; 3], rotation, anchor: [0.0; 2], opacity: 1.0 }) * flat(1.0, 1.0)
        };
        assert!(covers_frame(l(1.0, [0.0; 3], [0.0; 3]), 1080, 1080));
        assert!(covers_frame(l(1.2, [0.0, 0.0, 5.0], [0.0; 3]), 1080, 1080));
        assert!(covers_frame(l(1.0, [0.0, 0.0, 180.0], [0.0; 3]), 1080, 1080));
        assert!(!covers_frame(l(0.99, [0.0; 3], [0.0; 3]), 1080, 1080));
        assert!(!covers_frame(l(1.0, [0.0, 0.0, 5.0], [0.0; 3]), 1080, 1080));
        assert!(!covers_frame(l(1.0, [0.0; 3], [0.01, 0.0, 0.0]), 1080, 1080));
        assert!(!covers_frame(l(1.0, [0.0; 3], [0.0, 0.0, 0.1]), 1080, 1080));
        assert!(!covers_frame(l(1.0, [0.0; 3], [0.0, 0.0, -5.0]), 1080, 1080)); // behind the camera
    }

    #[test]
    fn cover_keeps_focus_and_contain_adds_bars() {
        // A 2:1 picture in a square box.
        let c = fit_uv(Fit::Cover, [0.5, 0.5], 2.0, 1.0);
        assert_eq!((c.offset, c.scale, c.cut), ([0.25, 0.0], [0.5, 1.0], false));
        let left = fit_uv(Fit::Cover, [0.0, 0.5], 2.0, 1.0);
        assert_eq!(left.offset, [0.0, 0.0]);
        let k = fit_uv(Fit::Contain, [0.5, 0.5], 2.0, 1.0);
        assert_eq!((k.offset, k.scale, k.cut), ([0.0, -0.5], [1.0, 2.0], true));
        let s = fit_uv(Fit::Stretch, [0.5, 0.5], 2.0, 1.0);
        assert_eq!((s.offset, s.scale), ([0.0, 0.0], [1.0, 1.0]));
    }
}
