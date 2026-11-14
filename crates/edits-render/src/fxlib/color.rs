//! Built-in colour effects that need a pass of their own (most colour
//! effects are fused into other passes instead: see `compose.rs`), and the
//! tables some of them read ([`table`]).

use super::stylize::display;
use super::{num, params, rgb, vec3, Builtin, Frame, Params, Table, TABLE_WIDTH};
use edits_core::ResolvedEffect;

pub(super) const BUILTINS: &[Builtin] = &[
    Builtin { kind: "lut", source: include_str!("../shaders/fx/lut.wgsl"), pack: lut },
    Builtin { kind: "gradient_map", source: include_str!("../shaders/fx/gradient_map.wgsl"), pack: gradient_map },
    Builtin { kind: "levels", source: include_str!("../shaders/fx/levels.wgsl"), pack: levels },
    Builtin { kind: "vibrance", source: include_str!("../shaders/fx/vibrance.wgsl"), pack: vibrance },
    Builtin { kind: "white_balance", source: include_str!("../shaders/fx/white_balance.wgsl"), pack: white_balance },
    Builtin { kind: "lift_gamma_gain", source: include_str!("../shaders/fx/lift_gamma_gain.wgsl"), pack: lift_gamma_gain },
    Builtin { kind: "highlights_shadows", source: include_str!("../shaders/fx/highlights_shadows.wgsl"), pack: highlights_shadows },
    Builtin { kind: "channel_mixer", source: include_str!("../shaders/fx/channel_mixer.wgsl"), pack: channel_mixer },
    Builtin { kind: "curves", source: include_str!("../shaders/fx/curves.wgsl"), pack: curves },
    Builtin { kind: "hsl", source: include_str!("../shaders/fx/hsl.wgsl"), pack: hsl },
    Builtin { kind: "colorama", source: include_str!("../shaders/fx/colorama.wgsl"), pack: colorama },
    Builtin { kind: "replace_color", source: include_str!("../shaders/fx/replace_color.wgsl"), pack: replace_color },
    Builtin { kind: "leave_color", source: include_str!("../shaders/fx/leave_color.wgsl"), pack: leave_color },
    Builtin { kind: "solarize", source: include_str!("../shaders/fx/solarize.wgsl"), pack: solarize },
    Builtin { kind: "bleach_bypass", source: include_str!("../shaders/fx/bleach_bypass.wgsl"), pack: amount_only },
    Builtin { kind: "technicolor", source: include_str!("../shaders/fx/technicolor.wgsl"), pack: amount_only },
    Builtin { kind: "haze", source: include_str!("../shaders/fx/haze.wgsl"), pack: haze },
];

fn lut(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[amount]))
}

fn gradient_map(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let (s, m, h) = (display(rgb(e, "shadows")), display(rgb(e, "midtones")), display(rgb(e, "highlights")));
    (amount > 0.0).then(|| params(&[s[0], s[1], s[2], m[0], m[1], m[2], h[0], h[1], h[2], amount]))
}

fn levels(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (ib, mut iw, gamma, ob, ow) = (num(e, "in_black"), num(e, "in_white"), num(e, "gamma"), num(e, "out_black"), num(e, "out_white"));
    if (iw - ib).abs() < 1e-6 {
        iw = ib + 1e-6;
    }
    let identity = ib == 0.0 && iw == 1.0 && gamma == 1.0 && ob == 0.0 && ow == 1.0;
    (!identity).then(|| params(&[ib, iw, 1.0 / gamma, ob, ow]))
}

fn vibrance(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount != 0.0).then(|| params(&[amount]))
}

fn amount_only(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[amount]))
}

fn white_balance(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (t, tint) = (num(e, "temperature"), num(e, "tint"));
    // tint 1 is GPUImage's tint 200: Q moves by 2 x 0.5226 x 0.1.
    (t != 0.0 || tint != 0.0).then(|| params(&[t, tint * 0.10452]))
}

fn lift_gamma_gain(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (lift, gamma, gain, sat) = (vec3(e, "lift"), vec3(e, "gamma"), vec3(e, "gain"), num(e, "saturation"));
    let identity = lift == [0.0; 3] && gamma == [1.0; 3] && gain == [1.0; 3] && sat == 1.0;
    let g = gamma.map(|v| 1.0 / v.max(0.01));
    (!identity).then(|| params(&[lift[0], lift[1], lift[2], g[0], g[1], g[2], gain[0], gain[1], gain[2], sat]))
}

fn highlights_shadows(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (s, h) = (num(e, "shadows"), num(e, "highlights"));
    (s > 0.0 || h > 0.0).then(|| params(&[s, 1.0 - h]))
}

fn channel_mixer(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let row = |name: &str| -> [f32; 4] {
        let v = e.vec(name, 4);
        std::array::from_fn(|i| if v[i].is_finite() { v[i] as f32 } else { 0.0 })
    };
    let (r, g, b, amount) = (row("red"), row("green"), row("blue"), num(e, "amount"));
    let identity = r == [1.0, 0.0, 0.0, 0.0] && g == [0.0, 1.0, 0.0, 0.0] && b == [0.0, 0.0, 1.0, 0.0];
    (amount > 0.0 && !identity).then(|| params(&[r[0], r[1], r[2], r[3], g[0], g[1], g[2], g[3], b[0], b[1], b[2], b[3], amount]))
}

fn curves(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let identity = ["master", "red", "green", "blue"].iter().all(|n| Curve::of(e, n).is_none());
    (amount > 0.0 && !identity).then(|| params(&[amount]))
}

fn hsl(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    HSL_BANDS.iter().any(|(name, _)| vec3(e, name) != [0.0; 3]).then(|| params(&[1.0]))
}

fn colorama(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    let phase = super::fract(e.num("phase") / 360.0);
    (amount > 0.0).then(|| params(&[phase, num(e, "cycles"), amount]))
}

fn replace_color(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (from, to, tol, amount) = (display(rgb(e, "from")), display(rgb(e, "to")), vec3(e, "tolerance"), num(e, "amount"));
    (amount > 0.0).then(|| params(&[from[0], from[1], from[2], to[0], to[1], to[2], tol[0], tol[1], tol[2], num(e, "softness"), amount]))
}

fn leave_color(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (k, tol, amount) = (display(rgb(e, "color")), num(e, "tolerance"), num(e, "amount"));
    (amount > 0.0 && tol < 1.0).then(|| params(&[k[0], k[1], k[2], tol, num(e, "softness"), amount]))
}

fn solarize(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let amount = num(e, "amount");
    (amount > 0.0).then(|| params(&[num(e, "threshold"), num(e, "softness"), amount]))
}

fn haze(e: &ResolvedEffect, _: &Frame) -> Option<Params> {
    let (d, slope, c) = (num(e, "distance"), num(e, "slope"), display(rgb(e, "color")));
    (d != 0.0 || slope != 0.0).then(|| params(&[d, slope, c[0], c[1], c[2]]))
}

// ---- tables -----------------------------------------------------------------

/// The table effect `kind` reads for `e`, when it reads one.
pub(super) fn table(kind: &str, e: &ResolvedEffect) -> Option<Table> {
    match kind {
        "curves" => Some(curves_table(e)),
        "hsl" => Some(hsl_table(e)),
        "colorama" => Some(colorama_table(e)),
        "gradient_map" if e.option("mode") == "stops" => Some(stops_table(e)),
        _ => None,
    }
}

/// The value of entry `i` of a table row: 0 to 1.
fn at(i: usize) -> f64 {
    i as f64 / (TABLE_WIDTH - 1) as f64
}

/// A tone curve: a natural cubic spline (second derivative 0 at both ends)
/// through control points, flat beyond the first and last point and clamped
/// to 0..1, as glfx.js's curves filter and EffectCraft's Curves draw it.
struct Curve {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// Second derivative at each point.
    m: Vec<f64>,
}

impl Curve {
    /// The curve of parameter `name` ([x0, y0, x1, y1, ...]), or `None` when it
    /// is the identity or has fewer than two distinct points.
    fn of(e: &ResolvedEffect, name: &str) -> Option<Curve> {
        let v = e.params.get(name)?;
        let mut pts: Vec<(f64, f64)> = v.chunks_exact(2).filter(|p| p[0].is_finite() && p[1].is_finite()).map(|p| (p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0))).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        // A repeated x keeps its last point.
        let mut dedup: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
        for p in pts {
            match dedup.last_mut() {
                Some(q) if (p.0 - q.0).abs() < 1e-9 => *q = p,
                _ => dedup.push(p),
            }
        }
        if dedup.len() < 2 || dedup.iter().all(|p| (p.0 - p.1).abs() < 1e-9) && dedup.first().map(|p| p.0) == Some(0.0) && dedup.last().map(|p| p.0) == Some(1.0) {
            return None;
        }
        let (xs, ys): (Vec<f64>, Vec<f64>) = dedup.into_iter().unzip();
        let m = natural_second_derivatives(&xs, &ys);
        Some(Curve { xs, ys, m })
    }

    fn eval(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        let k = self.xs.partition_point(|&v| v <= x).clamp(1, n - 1) - 1;
        let h = self.xs[k + 1] - self.xs[k];
        let (a, b) = ((self.xs[k + 1] - x) / h, (x - self.xs[k]) / h);
        let y = a * self.ys[k] + b * self.ys[k + 1] + ((a * a * a - a) * self.m[k] + (b * b * b - b) * self.m[k + 1]) * h * h / 6.0;
        y.clamp(0.0, 1.0)
    }
}

/// Second derivatives of the natural cubic spline through (xs, ys) (zero at
/// both ends): the tridiagonal system solved by the Thomas algorithm.
fn natural_second_derivatives(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let n = xs.len();
    let mut m = vec![0.0; n];
    if n < 3 {
        return m;
    }
    // Rows i = 1 .. n-2: h[i-1] m[i-1] + 2 (h[i-1] + h[i]) m[i] + h[i] m[i+1] = 6 (d[i] - d[i-1]).
    let h: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let d: Vec<f64> = (0..n - 1).map(|i| (ys[i + 1] - ys[i]) / h[i]).collect();
    let (mut diag, mut rhs) = (vec![0.0; n], vec![0.0; n]);
    for i in 1..n - 1 {
        diag[i] = 2.0 * (h[i - 1] + h[i]);
        rhs[i] = 6.0 * (d[i] - d[i - 1]);
        if i > 1 {
            let w = h[i - 1] / diag[i - 1];
            diag[i] -= w * h[i - 1];
            rhs[i] -= w * rhs[i - 1];
        }
    }
    for i in (1..n - 1).rev() {
        m[i] = (rhs[i] - h[i] * m[i + 1]) / diag[i];
    }
    m
}

/// For each x, every channel's curve after the master curve.
fn curves_table(e: &ResolvedEffect) -> Table {
    let master = Curve::of(e, "master");
    let chans = ["red", "green", "blue"].map(|n| Curve::of(e, n));
    let ev = |c: &Option<Curve>, x: f64| c.as_ref().map_or(x, |c| c.eval(x));
    (0..TABLE_WIDTH)
        .map(|i| {
            let x = ev(&master, at(i));
            std::array::from_fn(|k| ev(&chans[k], x) as f32)
        })
        .collect()
}

/// The bands of `hsl`: parameter and centre hue in degrees.
const HSL_BANDS: [(&str, f64); 8] = [("reds", 0.0), ("oranges", 30.0), ("yellows", 60.0), ("greens", 120.0), ("aquas", 180.0), ("blues", 240.0), ("purples", 270.0), ("magentas", 300.0)];

/// For each hue (0..1 turns): the hue shift in turns, the saturation and the
/// lightness step, each band's at its centre and blended smoothly between
/// neighbouring centres.
fn hsl_table(e: &ResolvedEffect) -> Table {
    let v: Vec<[f32; 3]> = HSL_BANDS.iter().map(|(name, _)| vec3(e, name)).collect();
    (0..TABLE_WIDTH)
        .map(|i| {
            let deg = at(i) * 360.0;
            let k = HSL_BANDS.iter().rposition(|(_, c)| *c <= deg).unwrap_or(0);
            let (c0, c1) = (HSL_BANDS[k].1, HSL_BANDS.get(k + 1).map_or(360.0, |b| b.1));
            let t = ((deg - c0) / (c1 - c0)).clamp(0.0, 1.0) as f32;
            let w = t * t * (3.0 - 2.0 * t);
            let (a, b) = (v[k], v[(k + 1) % HSL_BANDS.len()]);
            let mix = |j: usize| a[j] + (b[j] - a[j]) * w;
            [mix(0) / 360.0, mix(1).clamp(-1.0, 1.0), mix(2).clamp(-1.0, 1.0)]
        })
        .collect()
}

/// Colours of a list parameter [r, g, b, ...] (linear) as display values,
/// at most 16, at least one.
fn colour_list(e: &ResolvedEffect, name: &str) -> Vec<[f32; 3]> {
    let v = e.params.get(name).cloned().unwrap_or_default();
    let mut out: Vec<[f32; 3]> = v.chunks_exact(3).take(16).map(|c| display(std::array::from_fn(|k| if c[k].is_finite() { c[k] as f32 } else { 0.0 }))).collect();
    if out.is_empty() {
        out.push([0.0; 3]);
    }
    out
}

/// A palette of colours (display values) at positions in 0..1, sorted;
/// cyclic: past the last stop it blends back into the first.
fn palette_eval(stops: &[(f64, [f32; 3])], ph: f64) -> [f32; 3] {
    let n = stops.len();
    let k = stops.iter().rposition(|s| s.0 <= ph).unwrap_or(n - 1);
    let (p0, c0) = stops[k];
    let (p1, c1) = stops[(k + 1) % n];
    let span = (p1 - p0).rem_euclid(1.0).max(1e-6);
    let t = ((ph - p0).rem_euclid(1.0) / span).clamp(0.0, 1.0) as f32;
    std::array::from_fn(|i| c0[i] + (c1[i] - c0[i]) * t)
}

/// One cycle of `colorama`'s palette: the palettes are EffectCraft's
/// Colorama presets (display values). Entry x is the palette at 0.999 x, so
/// the end of a cycle is the palette's last stop (white for `fire`), not its
/// wrap back to the first.
fn colorama_table(e: &ResolvedEffect) -> Table {
    let s = |v: &[(f64, [f32; 3])]| v.to_vec();
    let stops: Vec<(f64, [f32; 3])> = match e.option("palette") {
        "fire" => s(&[(0.0, [0.0, 0.0, 0.0]), (0.3, [0.7, 0.05, 0.0]), (0.6, [1.0, 0.55, 0.0]), (0.85, [1.0, 0.95, 0.4]), (0.999, [1.0, 1.0, 1.0])]),
        "ice" => s(&[(0.0, [0.0, 0.0, 0.1]), (0.4, [0.1, 0.3, 0.8]), (0.75, [0.5, 0.85, 1.0]), (0.999, [1.0, 1.0, 1.0])]),
        "grey" => s(&[(0.0, [0.0; 3]), (0.999, [1.0; 3])]),
        "pastel" => s(&[(0.0, [1.0, 0.7, 0.7]), (1.0 / 3.0, [0.7, 1.0, 0.7]), (2.0 / 3.0, [0.7, 0.7, 1.0])]),
        "custom" => {
            let c = colour_list(e, "colors");
            let n = c.len() as f64;
            c.into_iter().enumerate().map(|(i, c)| (i as f64 / n, c)).collect()
        }
        _ => s(&[(0.0, [1.0, 0.0, 0.0]), (1.0 / 6.0, [1.0, 1.0, 0.0]), (2.0 / 6.0, [0.0, 1.0, 0.0]), (3.0 / 6.0, [0.0, 1.0, 1.0]), (4.0 / 6.0, [0.0, 0.0, 1.0]), (5.0 / 6.0, [1.0, 0.0, 1.0])]),
    };
    (0..TABLE_WIDTH).map(|i| palette_eval(&stops, at(i) * 0.999)).collect()
}

/// `gradient_map`'s stops: the colours evenly spaced from 0 to 1, blended
/// as display values.
fn stops_table(e: &ResolvedEffect) -> Table {
    let c = colour_list(e, "stops");
    let n = c.len();
    (0..TABLE_WIDTH)
        .map(|i| {
            if n == 1 {
                return c[0];
            }
            let x = at(i) * (n - 1) as f64;
            let k = (x.floor() as usize).min(n - 2);
            let t = (x - k as f64) as f32;
            std::array::from_fn(|j| c[k][j] + (c[k + 1][j] - c[k][j]) * t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_splines_pass_through_their_points_and_are_straight_through_two() {
        let xs = [0.0, 0.25, 0.75, 1.0];
        let ys = [0.0, 0.4, 0.6, 1.0];
        let m = natural_second_derivatives(&xs, &ys);
        assert_eq!((m[0], m[3]), (0.0, 0.0));
        let c = Curve { xs: xs.to_vec(), ys: ys.to_vec(), m };
        for (x, y) in xs.iter().zip(ys) {
            assert!((c.eval(*x) - y).abs() < 1e-9);
        }
        // Symmetric points give a curve symmetric about the centre.
        assert!((c.eval(0.4) + c.eval(0.6) - 1.0).abs() < 1e-9);
        let line = Curve { xs: vec![0.0, 1.0], ys: vec![0.2, 0.8], m: vec![0.0, 0.0] };
        assert!((line.eval(0.5) - 0.5).abs() < 1e-12);
        // A spline through collinear points is that line.
        let xs = [0.0, 0.3, 1.0];
        let ys = [0.0, 0.3, 1.0];
        let m = natural_second_derivatives(&xs, &ys);
        assert!(m.iter().all(|v| v.abs() < 1e-12));
    }
}
