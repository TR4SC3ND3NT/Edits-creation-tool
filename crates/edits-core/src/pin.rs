//! Corner pin: a layer's picture mapped onto any four corners by a plane
//! homography (a projective map), as a planar tracker measures it.
//!
//! A pin is written in a layer's `transform.pin` as 8 numbers, the corners
//! `[x0, y0, x1, y1, x2, y2, x3, y3]` of the picture's top-left, top-right,
//! bottom-right and bottom-left in layer space (composition-height units,
//! origin at the centre, y down), or as 9 numbers, a row-major 3x3 matrix that
//! maps picture coordinates `(u, v, 1)`, `u` and `v` in 0..1 from the top-left,
//! to `(x, y, w)` in layer space. Keyed per frame, the corners are interpolated
//! like any vector, so motion blur sub-samples move them smoothly.
//!
//! The layer's other transform (position, scale, rotation, anchor) and the
//! camera apply after the pin, so with both at rest the corners are where the
//! picture's corners land in the frame.

/// A 3x3 matrix, row-major.
pub type Mat3 = [[f64; 3]; 3];

pub const IDENTITY: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

pub fn det(m: &Mat3) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

pub fn inverse(m: &Mat3) -> Option<Mat3> {
    let d = det(m);
    if !d.is_finite() || d.abs() < 1e-15 {
        return None;
    }
    let c = |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    Some([
        [c(1, 1, 2, 2) / d, -c(0, 1, 2, 2) / d, c(0, 1, 1, 2) / d],
        [-c(1, 0, 2, 2) / d, c(0, 0, 2, 2) / d, -c(0, 0, 1, 2) / d],
        [c(1, 0, 2, 1) / d, -c(0, 0, 2, 1) / d, c(0, 0, 1, 1) / d],
    ])
}

/// `m` applied to the point `(x, y)`; `None` at or behind the line at infinity.
pub fn apply(m: &Mat3, x: f64, y: f64) -> Option<[f64; 2]> {
    let w = m[2][0] * x + m[2][1] * y + m[2][2];
    if w.abs() < 1e-12 {
        return None;
    }
    Some([(m[0][0] * x + m[0][1] * y + m[0][2]) / w, (m[1][0] * x + m[1][1] * y + m[1][2]) / w])
}

/// The homography taking the unit square (0,0), (1,0), (1,1), (0,1) to the
/// four corners `q` in that order (Heckbert's closed form). `None` when three
/// corners are on one line.
pub fn square_to_quad(q: [[f64; 2]; 4]) -> Option<Mat3> {
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = q;
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
    let den = dx1 * dy2 - dx2 * dy1;
    let (g, h) = if sx.abs() < 1e-15 && sy.abs() < 1e-15 {
        (0.0, 0.0)
    } else {
        if den.abs() < 1e-15 {
            return None;
        }
        ((sx * dy2 - dx2 * sy) / den, (dx1 * sy - sx * dy1) / den)
    };
    let m = [[x1 - x0 + g * x1, x3 - x0 + h * x3, x0], [y1 - y0 + g * y1, y3 - y0 + h * y3, y0], [g, h, 1.0]];
    (det(&m).abs() > 1e-15).then_some(m)
}

/// The corners of the unit square under `m`, top-left first, clockwise on screen.
pub fn corners(m: &Mat3) -> Option<[[f64; 2]; 4]> {
    Some([apply(m, 0.0, 0.0)?, apply(m, 1.0, 0.0)?, apply(m, 1.0, 1.0)?, apply(m, 0.0, 1.0)?])
}

/// Whether the four corners make a convex quad (any winding): the only kind
/// a plane can project to with every corner in front of the camera.
pub fn convex(q: &[[f64; 2]; 4]) -> bool {
    let cross = |i: usize| {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    };
    let s: Vec<f64> = (0..4).map(cross).collect();
    s.iter().all(|v| *v > 1e-12) || s.iter().all(|v| *v < -1e-12)
}

/// A pin value (8 corner numbers or 9 matrix numbers) as the map from
/// picture coordinates (0..1) to layer space, scaled so that `w > 0` over the
/// picture. `Err` says what is wrong with it.
pub fn from_values(v: &[f64]) -> Result<Mat3, String> {
    if v.iter().any(|x| !x.is_finite()) {
        return Err("a pin value is not a finite number".into());
    }
    let m = match v.len() {
        8 => {
            let q = [[v[0], v[1]], [v[2], v[3]], [v[4], v[5]], [v[6], v[7]]];
            if !convex(&q) {
                return Err("the four pin corners do not make a convex quad (order: top-left, top-right, bottom-right, bottom-left)".into());
            }
            square_to_quad(q).ok_or("three pin corners are on one line")?
        }
        9 => [[v[0], v[1], v[2]], [v[3], v[4], v[5]], [v[6], v[7], v[8]]],
        n => return Err(format!("a pin is 8 numbers (four corners) or 9 (a 3x3 matrix), not {n}")),
    };
    if det(&m).abs() < 1e-15 {
        return Err("the pin matrix is singular".into());
    }
    // w at the four picture corners: all of one sign, or the picture crosses the horizon.
    let ws: Vec<f64> = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].iter().map(|(x, y)| m[2][0] * x + m[2][1] * y + m[2][2]).collect();
    let sign = if ws.iter().all(|w| *w > 1e-12) {
        1.0
    } else if ws.iter().all(|w| *w < -1e-12) {
        -1.0
    } else {
        return Err("the pin matrix sends part of the picture past the horizon (w changes sign over it)".into());
    };
    Ok(m.map(|r| r.map(|x| x * sign)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_to_quad_hits_the_corners() {
        let q = [[-0.4, -0.3], [0.5, -0.2], [0.3, 0.45], [-0.35, 0.3]];
        let m = square_to_quad(q).unwrap();
        let c = corners(&m).unwrap();
        for i in 0..4 {
            assert!((c[i][0] - q[i][0]).abs() < 1e-12 && (c[i][1] - q[i][1]).abs() < 1e-12, "{c:?}");
        }
        let inv = inverse(&m).unwrap();
        let p = apply(&inv, 0.3, 0.45).unwrap();
        assert!((p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn values_are_checked() {
        assert!(from_values(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]).is_ok());
        // Bow-tie: two corners swapped.
        assert!(from_values(&[0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0]).is_err());
        assert!(from_values(&[1.0, 2.0, 3.0]).is_err());
        // A negative overall scale is normalised to w > 0.
        let m = from_values(&[-1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0]).unwrap();
        assert!(m[2][2] > 0.0);
        // Horizon through the picture.
        assert!(from_values(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, -2.0, 0.0, 1.0]).is_err());
    }
}
