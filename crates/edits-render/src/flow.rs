//! Optical flow between two frames of a clip, and the in-between pictures it
//! makes (`frame_blend: flow`).
//!
//! After EffectCraft (https://github.com/storytold/effectcraft, MIT OR
//! Apache-2.0), `crates/raster/src/flow.rs` (hierarchical block matching on
//! luma: a box pyramid, an exhaustive search at the top, a 3x3 search around
//! the vectors of the coarser level below it, a parabola through the costs
//! for sub-pixel vectors, a 3x3 median) and `fx_time.wgsl::ftm_motion` (warp
//! both frames towards the in-between instant and cross-fade them). Written
//! from the published description of the algorithm, not from its code.
//!
//! Ours adds:
//! - flow in both directions, made per pixel: each pixel picks among the
//!   vectors of the tiles around it the one its own neighbourhood matches
//!   best, so motion edges follow the picture;
//! - for each pixel of the in-between, a choice among candidate motions of
//!   the one each frame confirms (the pixel it comes from in `a` moves by it,
//!   the pixel it goes to in `b` comes back by it): the forward-backward
//!   consistency check (Sundaram, Brox and Keutzer 2010) along the path;
//! - occlusions: a motion only one frame confirms takes that frame alone when
//!   the other has a consistently moving occluder there; with nothing
//!   consistent in either (a cut, a flash, a wrong vector) the pixel is the
//!   plain cross-fade (`mix`), as it is where the two warped frames disagree;
//! - bicubic sampling and the cross-fade in linear light.
//!
//! Everything runs on the CPU in `f32`, split by rows over a few threads; a
//! row's result does not depend on the split, so the pixels are the same on
//! every machine and adapter. Colour is warped and mixed in linear light, the
//! way [`crate::video::mix`] mixes, so a field of zero vectors gives exactly
//! the `mix` picture.

use crate::video::{linear_luts, round};
use image::RgbaImage;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tile size of the vector grid, in pixels of the level searched.
const TILE: usize = 8;
/// Side of the square compared for each tile (centred on the tile, so
/// neighbouring windows overlap). A multiple of 4.
const WINDOW: i32 = 12;
const _: () = assert!(WINDOW % 4 == 0);
/// Exhaustive search radius at the coarsest level, in pixels of that level.
const RADIUS: i32 = 4;
/// Half side of the neighbourhood compared when picking per-pixel vectors.
const PATCH: i32 = 2;
/// Tile vectors closer than this (pixels, either axis) are one candidate for a pixel.
const SAME_VECTOR: f32 = 0.75;
/// Exhaustive search radius around the zero vector at the finer levels, so a
/// small object the coarse levels blurred away is still found.
const FINE_RADIUS: i32 = 2;
/// Frames are box-halved until their longer side is at most this before the flow is estimated.
const FLOW_MAX_SIDE: usize = 1024;
/// The pyramid stops before a level's shorter side would be under this.
const MIN_LEVEL_SIDE: usize = 16;
/// Most pyramid levels (the flow level included).
const MAX_LEVELS: usize = 6;
/// Cost added per pixel of vector length, so flat areas keep short vectors.
const LENGTH_PENALTY: f32 = 1e-4;
/// Two warped samples that differ by more than this (linear light, largest
/// channel) start to fall back to the cross-fade, fully at twice it.
const DISAGREE: f32 = 0.15;

/// Threads used for one picture; 0 picks from the machine. Tests set it to
/// show that the split does not change the result.
static THREADS: AtomicUsize = AtomicUsize::new(0);

fn threads() -> usize {
    match THREADS.load(Ordering::Relaxed) {
        0 => std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(8),
        n => n,
    }
}

/// Run `f(row index, row)` over every `row_len` long row of `out`, on a few
/// threads. Each row is computed alone, so the split does not change it.
fn par_rows<T: Send>(out: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let row_len = row_len.max(1);
    let rows = out.len() / row_len;
    let n = threads().min(rows);
    if n <= 1 || rows < 4 {
        out.chunks_mut(row_len).enumerate().for_each(|(j, row)| f(j, row));
        return;
    }
    let band = rows.div_ceil(n);
    std::thread::scope(|s| {
        for (k, chunk) in out.chunks_mut(band * row_len).enumerate() {
            let f = &f;
            s.spawn(move || chunk.chunks_mut(row_len).enumerate().for_each(|(j, row)| f(k * band + j, row)));
        }
    });
}

/// Length of a vector (`hypot` without its care for overflow, which costs).
#[inline]
fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// A single-channel picture (luma, about 0..1.25).
struct Plane {
    w: usize,
    h: usize,
    d: Vec<f32>,
}

impl Plane {
    /// Luma of premultiplied sRGB bytes, with a quarter of alpha so that
    /// transparent and black differ.
    fn luma(img: &RgbaImage) -> Self {
        let d = img.as_raw().chunks_exact(4).map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32 + 0.25 * p[3] as f32) / 255.0).collect();
        Self { w: img.width() as usize, h: img.height() as usize, d }
    }

    /// Half size (rounded up), each pixel the mean of a 2x2 block (edges clamped).
    fn half(&self) -> Self {
        let (w, h) = (self.w.div_ceil(2), self.h.div_ceil(2));
        let mut d = vec![0.0; w * h];
        par_rows(&mut d, w, |y, row| {
            let (y0, y1) = (2 * y, (2 * y + 1).min(self.h - 1));
            for (x, o) in row.iter_mut().enumerate() {
                let (x0, x1) = (2 * x, (2 * x + 1).min(self.w - 1));
                *o = 0.25 * (self.d[y0 * self.w + x0] + self.d[y0 * self.w + x1] + self.d[y1 * self.w + x0] + self.d[y1 * self.w + x1]);
            }
        });
        Self { w, h, d }
    }

    #[inline]
    fn get(&self, x: i32, y: i32) -> f32 {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        self.d[y * self.w + x]
    }

    fn holds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x + WINDOW <= self.w as i32 && y + WINDOW <= self.h as i32
    }
}

/// Values on a grid of cells over one level: cell (i, j) is centred at
/// ((i + 0.5) x cell, (j + 0.5) x cell) pixels of that level. Tiles have
/// cells of `TILE` pixels, dense fields of 1.
#[derive(Clone, Debug)]
struct Grid<const N: usize> {
    gw: usize,
    gh: usize,
    /// 1 / the cell size in pixels (cells are powers of two, so this is exact).
    inv: f32,
    v: Vec<[f32; N]>,
}

impl<const N: usize> Grid<N> {
    fn new(gw: usize, gh: usize, cell: usize, value: [f32; N]) -> Self {
        Self { gw, gh, inv: 1.0 / cell as f32, v: vec![value; gw * gh] }
    }

    fn tiles(p: &Plane, value: [f32; N]) -> Self {
        Self::new(p.w.div_ceil(TILE), p.h.div_ceil(TILE), TILE, value)
    }

    /// The value at a point (pixels of the grid's level), bilinear between
    /// cell centres, held past the outer ones.
    #[inline]
    fn at(&self, x: f32, y: f32) -> [f32; N] {
        let gx = (x * self.inv - 0.5).clamp(0.0, (self.gw - 1) as f32);
        let gy = (y * self.inv - 0.5).clamp(0.0, (self.gh - 1) as f32);
        let (i0, j0) = (gx as usize, gy as usize);
        let (i1, j1) = ((i0 + 1).min(self.gw - 1), (j0 + 1).min(self.gh - 1));
        let (tx, ty) = (gx - i0 as f32, gy - j0 as f32);
        let (a, b, c, d) = (self.v[j0 * self.gw + i0], self.v[j0 * self.gw + i1], self.v[j1 * self.gw + i0], self.v[j1 * self.gw + i1]);
        let mut out = [0.0; N];
        for k in 0..N {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        out
    }
}

/// The optical flow between two frames `a` and `b`, both ways.
#[derive(Clone, Debug)]
pub(crate) struct Flow {
    /// Frame pixels per pixel of the level the flow was estimated at.
    scale: f32,
    /// Where each pixel of `a` is in `b`, and each pixel of `b` in `a` (pixels of the flow level).
    forward: Grid<2>,
    backward: Grid<2>,
}

/// Estimate the flow between two frames of the same size.
pub(crate) fn estimate(a: &RgbaImage, b: &RgbaImage) -> Flow {
    assert_eq!(a.dimensions(), b.dimensions(), "flow between frames of different sizes");
    let (mut la, mut lb) = (Plane::luma(a), Plane::luma(b));
    let mut scale = 1.0;
    while la.w.max(la.h) > FLOW_MAX_SIDE {
        (la, lb) = (la.half(), lb.half());
        scale *= 2.0;
    }
    let mut levels = vec![(la, lb)];
    while levels.len() < MAX_LEVELS {
        let (a, b) = levels.last().expect("one level");
        if a.w.min(a.h) / 2 < MIN_LEVEL_SIDE {
            break;
        }
        let next = (a.half(), b.half());
        levels.push(next);
    }
    let (a0, b0) = &levels[0];
    let forward = densify(a0, b0, &track(&levels, false));
    let backward = densify(b0, a0, &track(&levels, true));
    Flow { scale, forward, backward }
}

/// Coarse to fine: the vectors of `a` (or of `b` when `back`) in the other frame.
fn track(levels: &[(Plane, Plane)], back: bool) -> Grid<2> {
    let mut field: Option<Grid<2>> = None;
    for (k, (a, b)) in levels.iter().enumerate().rev() {
        let (a, b) = if back { (b, a) } else { (a, b) };
        let mut g = median(&search(a, b, field.as_ref()));
        if k == 0 {
            refine(a, b, &mut g);
        }
        field = Some(g);
    }
    field.expect("at least one level")
}

/// Mean absolute (or squared) difference between the window at (x0, y0) in
/// `a` and the one moved by (dx, dy) in `b`.
#[inline]
fn difference<const SQUARED: bool>(a: &Plane, b: &Plane, x0: i32, y0: i32, dx: i32, dy: i32) -> f32 {
    let mut sum = 0.0f32;
    let mut add = |p: f32, q: f32| {
        let e = p - q;
        sum += if SQUARED { e * e } else { e.abs() };
    };
    if a.holds(x0, y0) && b.holds(x0 + dx, y0 + dy) {
        // Four running sums (in a fixed order), which the compiler can keep in one vector register.
        let n = WINDOW as usize;
        let mut lanes = [0.0f32; 4];
        for y in 0..WINDOW {
            let ra = &a.d[(y0 + y) as usize * a.w + x0 as usize..][..n];
            let rb = &b.d[(y0 + dy + y) as usize * b.w + (x0 + dx) as usize..][..n];
            for (p, q) in ra.chunks_exact(4).zip(rb.chunks_exact(4)) {
                for l in 0..4 {
                    let e = p[l] - q[l];
                    lanes[l] += if SQUARED { e * e } else { e.abs() };
                }
            }
        }
        sum = (lanes[0] + lanes[1]) + (lanes[2] + lanes[3]);
    } else {
        for y in 0..WINDOW {
            for x in 0..WINDOW {
                add(a.get(x0 + x, y0 + y), b.get(x0 + dx + x, y0 + dy + y));
            }
        }
    }
    sum / (WINDOW * WINDOW) as f32
}

/// The window of tile (i, j): its top-left pixel.
fn window(i: usize, j: usize) -> (i32, i32) {
    ((i * TILE + TILE / 2) as i32 - WINDOW / 2, (j * TILE + TILE / 2) as i32 - WINDOW / 2)
}

/// Whole-pixel vectors of one level. Candidates: every vector within
/// `RADIUS` at the coarsest level; at the others every vector within
/// `FINE_RADIUS`, and 3x3 around the doubled vectors of the coarser level near
/// the tile (bilinear, its own and its 4 neighbours') and around the vector
/// just found for the tile on the left. The lowest mean absolute difference
/// wins (plus a little per pixel of length; of equals, the first in
/// (dx, dy) order).
fn search(a: &Plane, b: &Plane, coarse: Option<&Grid<2>>) -> Grid<2> {
    let mut g = Grid::tiles(a, [0.0; 2]);
    let gw = g.gw;
    par_rows(&mut g.v, gw, |j, row| {
        let mut candidates: Vec<(i32, i32)> = Vec::with_capacity(128);
        let mut seeds: Vec<(i32, i32)> = Vec::with_capacity(8);
        let mut left: Option<(i32, i32)> = None;
        for (i, out) in row.iter_mut().enumerate() {
            candidates.clear();
            seeds.clear();
            let r = if coarse.is_some() { FINE_RADIUS } else { RADIUS };
            candidates.extend((-r..=r).flat_map(|dy| (-r..=r).map(move |dx| (dx, dy))));
            if let Some(c) = coarse {
                let double = |v: [f32; 2]| ((2.0 * v[0]).round() as i32, (2.0 * v[1]).round() as i32);
                let (cx, cy) = ((i * TILE + TILE / 2) as f32 / 2.0, (j * TILE + TILE / 2) as f32 / 2.0);
                let (ci, cj) = (((cx as usize) / TILE).min(c.gw - 1), ((cy as usize) / TILE).min(c.gh - 1));
                seeds.push(double(c.at(cx, cy)));
                for (di, dj) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let ii = (ci as i64 + di).clamp(0, c.gw as i64 - 1) as usize;
                    let jj = (cj as i64 + dj).clamp(0, c.gh as i64 - 1) as usize;
                    seeds.push(double(c.v[jj * c.gw + ii]));
                }
            }
            seeds.extend(left);
            for &(sx, sy) in &seeds {
                candidates.extend((-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (sx + dx, sy + dy))));
            }
            // A fixed order (which settles exact ties), each vector once.
            candidates.sort_unstable();
            candidates.dedup();
            let (x0, y0) = window(i, j);
            let mut best = (f32::INFINITY, (0, 0));
            for &(dx, dy) in &candidates {
                let cost = difference::<false>(a, b, x0, y0, dx, dy) + LENGTH_PENALTY * (dx.abs() + dy.abs()) as f32;
                if cost < best.0 {
                    best = (cost, (dx, dy));
                }
            }
            left = Some(best.1);
            *out = [best.1 .0 as f32, best.1 .1 as f32];
        }
    });
    g
}

/// Component-wise median of each vector and its (up to 8) neighbours.
fn median(g: &Grid<2>) -> Grid<2> {
    let mut out = g.clone();
    let mut buf = Vec::with_capacity(9);
    for j in 0..g.gh {
        for i in 0..g.gw {
            for c in 0..2 {
                buf.clear();
                for jj in j.saturating_sub(1)..(j + 2).min(g.gh) {
                    for ii in i.saturating_sub(1)..(i + 2).min(g.gw) {
                        buf.push(g.v[jj * g.gw + ii][c]);
                    }
                }
                buf.sort_by(f32::total_cmp);
                out.v[j * g.gw + i][c] = buf[buf.len() / 2];
            }
        }
    }
    out
}

/// Sub-pixel vectors: on each axis, the vertex of the parabola through the
/// squared differences at the vector and one pixel either side. An exact
/// match stays where it is.
fn refine(a: &Plane, b: &Plane, g: &mut Grid<2>) {
    let gw = g.gw;
    par_rows(&mut g.v, gw, |j, row| {
        for (i, v) in row.iter_mut().enumerate() {
            let (dx, dy) = (v[0] as i32, v[1] as i32);
            let (x0, y0) = window(i, j);
            let e = |ox: i32, oy: i32| difference::<true>(a, b, x0, y0, dx + ox, dy + oy);
            let e0 = e(0, 0);
            if e0 <= 1e-9 {
                continue;
            }
            let vertex = |minus: f32, plus: f32| {
                let d = minus - 2.0 * e0 + plus;
                if d > 1e-9 {
                    (0.5 * (minus - plus) / d).clamp(-0.5, 0.5)
                } else {
                    0.0
                }
            };
            *v = [dx as f32 + vertex(e(-1, 0), e(1, 0)), dy as f32 + vertex(e(0, -1), e(0, 1))];
        }
    });
}

/// Per-pixel vectors from the tile vectors: at each pixel, of the bilinear
/// vector there and the vectors of the 3x3 tiles around it, the one whose
/// 5x5 neighbourhood matches best (compared at whole pixels, the first of
/// equals kept; a pixel with a single candidate keeps the bilinear vector;
/// vectors within `SAME_VECTOR` of an
/// earlier one are not candidates). Motion edges follow the picture
/// instead of the tile grid.
fn densify(a: &Plane, b: &Plane, tiles: &Grid<2>) -> Grid<2> {
    // Tiles whose 3x3 neighbourhood is within half of `SAME_VECTOR` of their
    // own vector: every pixel there has one candidate.
    let smooth: Vec<bool> = (0..tiles.v.len())
        .map(|k| {
            let (i, j) = (k % tiles.gw, k / tiles.gw);
            let own = tiles.v[k];
            (j.saturating_sub(1)..(j + 2).min(tiles.gh)).all(|jj| {
                (i.saturating_sub(1)..(i + 2).min(tiles.gw)).all(|ii| {
                    let v = tiles.v[jj * tiles.gw + ii];
                    (v[0] - own[0]).abs().max((v[1] - own[1]).abs()) <= 0.5 * SAME_VECTOR
                })
            })
        })
        .collect();
    let mut g = Grid::new(a.w, a.h, 1, [0.0; 2]);
    par_rows(&mut g.v, a.w, |y, row| {
        let mut candidates: Vec<([f32; 2], (i32, i32))> = Vec::with_capacity(10);
        let tj = (y / TILE).min(tiles.gh - 1);
        for (x, out) in row.iter_mut().enumerate() {
            if smooth[tj * tiles.gw + (x / TILE).min(tiles.gw - 1)] {
                *out = tiles.at(x as f32 + 0.5, y as f32 + 0.5);
                continue;
            }
            candidates.clear();
            let mut add = |v: [f32; 2]| {
                if !candidates.iter().any(|c| (c.0[0] - v[0]).abs().max((c.0[1] - v[1]).abs()) <= SAME_VECTOR) {
                    candidates.push((v, (v[0].round() as i32, v[1].round() as i32)));
                }
            };
            add(tiles.at(x as f32 + 0.5, y as f32 + 0.5));
            let ti = (x / TILE).min(tiles.gw - 1);
            for jj in tj.saturating_sub(1)..(tj + 2).min(tiles.gh) {
                for ii in ti.saturating_sub(1)..(ti + 2).min(tiles.gw) {
                    add(tiles.v[jj * tiles.gw + ii]);
                }
            }
            if candidates.len() == 1 {
                *out = candidates[0].0;
                continue;
            }
            let (x, y) = (x as i32, y as i32);
            let mut best = (f32::INFINITY, candidates[0].0);
            for &(v, (dx, dy)) in &candidates {
                let mut cost = 0.0;
                for oy in -PATCH..=PATCH {
                    for ox in -PATCH..=PATCH {
                        cost += (a.get(x + ox, y + oy) - b.get(x + dx + ox, y + dy + oy)).abs();
                    }
                }
                if cost < best.0 {
                    best = (cost, v);
                }
            }
            *out = best.1;
        }
    });
    g
}

/// How far each vector of `there` is trusted: 1 when following it and then
/// `back` returns within a pixel (plus 5 % of its length) of the start, 0 at
/// twice that or when it leaves the frame (`w` x `h` pixels of the level).
#[cfg(test)]
fn consistency(there: &Grid<2>, back: &Grid<2>, w: usize, h: usize) -> Grid<1> {
    let mut out = Grid::new(there.gw, there.gh, 1, [0.0]);
    for j in 0..there.gh {
        for i in 0..there.gw {
            let v = there.v[j * there.gw + i];
            let c = 1.0 / there.inv;
            let (px, py) = (((i as f32 + 0.5) * c).min(w as f32 - 0.5), ((j as f32 + 0.5) * c).min(h as f32 - 0.5));
            let (qx, qy) = (px + v[0], py + v[1]);
            if qx < -0.5 || qy < -0.5 || qx > w as f32 + 0.5 || qy > h as f32 + 0.5 {
                continue;
            }
            let u = back.at(qx, qy);
            let error = length(v[0] + u[0], v[1] + u[1]);
            out.v[j * there.gw + i] = [(2.0 - error / (1.0 + 0.05 * length(v[0], v[1]))).clamp(0.0, 1.0)];
        }
    }
    out
}

impl Flow {
    /// How far a motion `v` (pixels of the flow level, from `a` to `b`)
    /// through point `q` of the in-between at `t` is confirmed by each
    /// frame: by `a` when the pixel it comes from moves by `v`, by `b` when
    /// the pixel it goes to comes back by `v` (both together are the
    /// forward-backward consistency check along the path). 1 within a pixel
    /// (plus 5 % of the length), 0 at twice that or off the frame.
    #[inline]
    fn confirm(&self, q: (f32, f32), t: f32, v: [f32; 2]) -> (f32, f32) {
        let tolerance = 1.0 + 0.05 * length(v[0], v[1]);
        let (w, h) = (self.forward.gw as f32, self.forward.gh as f32);
        let score = |x: f32, y: f32, field: &Grid<2>, sign: f32| {
            if x < -0.5 || y < -0.5 || x > w + 0.5 || y > h + 0.5 {
                return 0.0;
            }
            let u = field.at(x, y);
            let error = length(u[0] - sign * v[0], u[1] - sign * v[1]);
            (2.0 - error / tolerance).clamp(0.0, 1.0)
        };
        (score(q.0 - t * v[0], q.1 - t * v[1], &self.forward, 1.0), score(q.0 + (1.0 - t) * v[0], q.1 + (1.0 - t) * v[1], &self.backward, -1.0))
    }

    /// The motion (from `a` to `b`) of what is at point `q` of the
    /// in-between picture at `t` (0 is `a`, 1 is `b`), all in pixels of the flow level,
    /// and how far `a` and `b` confirm it. Candidates: the forward vector at
    /// `p`, the backward one reversed, and each followed once into the two
    /// frames; the best confirmed wins (the first of equals, the first fully
    /// confirmed at once). Where something is hidden in one frame only the
    /// other confirms, which is how occlusions are told from wrong vectors.
    fn motion_at(&self, q: (f32, f32), t: f32) -> ([f32; 2], f32, f32) {
        let f = self.forward.at(q.0, q.1);
        let mut best = ([0.0; 2], -1.0, 0.0, 0.0);
        let mut consider = |v: [f32; 2]| {
            let (ca, cb) = self.confirm(q, t, v);
            if ca + cb > best.1 {
                best = (v, ca + cb, ca, cb);
            }
            ca + cb >= 2.0
        };
        'search: for first in [true, false] {
            let u = if first {
                f
            } else {
                let g = self.backward.at(q.0, q.1);
                [-g[0], -g[1]]
            };
            if consider(u) {
                break;
            }
            let from_a = self.forward.at(q.0 - t * u[0], q.1 - t * u[1]);
            let to_b = self.backward.at(q.0 + (1.0 - t) * u[0], q.1 + (1.0 - t) * u[1]);
            for v in [from_a, [-to_b[0], -to_b[1]]] {
                if consider(v) {
                    break 'search;
                }
            }
        }
        let (v, _, mut ca, mut cb) = best;
        if ca < 1.0 || cb < 1.0 {
            // Seen from one frame only. That is an occlusion when the other
            // frame has something else there that moves consistently (the
            // occluder); when it has no consistent motion either (a cut, a
            // flash, a wrong vector), neither side is trusted.
            let ka = self.consistent(&self.forward, &self.backward, (q.0 - t * v[0], q.1 - t * v[1]));
            let kb = self.consistent(&self.backward, &self.forward, (q.0 + (1.0 - t) * v[0], q.1 + (1.0 - t) * v[1]));
            (ca, cb) = (ca * (cb + (1.0 - cb) * kb), cb * (ca + (1.0 - ca) * ka));
        }
        (v, ca, cb)
    }

    /// The motion field of the in-between at `t`, at the flow level: per
    /// pixel the vector of [`Self::motion_at`] and how far `a` and `b`
    /// confirm it.
    fn motions(&self, t: f32) -> Grid<4> {
        let (w, h) = (self.forward.gw, self.forward.gh);
        let mut m = Grid::new(w, h, 1, [0.0; 4]);
        par_rows(&mut m.v, w, |y, row| {
            for (x, o) in row.iter_mut().enumerate() {
                let (v, ca, cb) = self.motion_at((x as f32 + 0.5, y as f32 + 0.5), t);
                *o = [v[0], v[1], ca, cb];
            }
        });
        m
    }

    /// From a motion field of [`Self::motions`], at frame position `p`: the
    /// motion in frame pixels and how far `a` and `b` confirm it (bilinear
    /// between flow-level pixels; exact at them).
    #[inline]
    fn motion(&self, m: &Grid<4>, p: (f32, f32)) -> ([f32; 2], f32, f32) {
        let k = m.at(p.0 / self.scale, p.1 / self.scale);
        ([k[0] * self.scale, k[1] * self.scale], k[2], k[3])
    }

    /// The forward-backward consistency of `field` at flow-level point `q`:
    /// 1 when its vector there and `back`'s vector where it lands cancel
    /// within a pixel (plus 5 % of the length), 0 at twice that or off the frame.
    #[inline]
    fn consistent(&self, field: &Grid<2>, back: &Grid<2>, q: (f32, f32)) -> f32 {
        let (w, h) = (field.gw as f32, field.gh as f32);
        if q.0 < -0.5 || q.1 < -0.5 || q.0 > w + 0.5 || q.1 > h + 0.5 {
            return 0.0;
        }
        let v = field.at(q.0, q.1);
        let (x, y) = (q.0 + v[0], q.1 + v[1]);
        if x < -0.5 || y < -0.5 || x > w + 0.5 || y > h + 0.5 {
            return 0.0;
        }
        let u = back.at(x, y);
        let error = length(v[0] + u[0], v[1] + u[1]);
        (2.0 - error / (1.0 + 0.05 * length(v[0], v[1]))).clamp(0.0, 1.0)
    }

    /// The mean forward-backward consistency of each field (for tests).
    #[cfg(test)]
    fn trust(&self) -> (f32, f32) {
        let (w, h) = (self.forward.gw, self.forward.gh);
        let mean = |g: &Grid<1>| g.v.iter().map(|v| v[0]).sum::<f32>() / g.v.len() as f32;
        (mean(&consistency(&self.forward, &self.backward, w, h)), mean(&consistency(&self.backward, &self.forward, w, h)))
    }
}

/// Catmull-Rom weights of the 4 taps around a fraction `t` (exactly 0, 1, 0, 0 at `t` = 0).
#[inline]
fn cubic(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [(-t3 + 2.0 * t2 - t) * 0.5, (3.0 * t3 - 5.0 * t2 + 2.0) * 0.5, (-3.0 * t3 + 4.0 * t2 + t) * 0.5, (t3 - t2) * 0.5]
}

/// A frame in linear light: premultiplied colour, alpha still 0..255.
struct Linear {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Linear {
    fn new(img: &RgbaImage, decode: &[f32]) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let raw = img.as_raw();
        let mut px = vec![[0.0; 4]; w * h];
        par_rows(&mut px, w, |y, row| {
            for (o, p) in row.iter_mut().zip(raw[y * w * 4..(y + 1) * w * 4].chunks_exact(4)) {
                *o = [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize], p[3] as f32];
            }
        });
        Self { w, h, px }
    }

    /// Bicubic (Catmull-Rom) sample at a frame position (pixel centres at
    /// +0.5, edges held). At a pixel centre it is that pixel exactly.
    #[inline]
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (w, h) = (self.w, self.h);
        let fx = (x - 0.5).clamp(0.0, (w - 1) as f32);
        let fy = (y - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        if tx == 0.0 && ty == 0.0 {
            return self.px[y0 * w + x0];
        }
        let (kx, ky) = (cubic(tx), cubic(ty));
        let xs = [x0.saturating_sub(1), x0, (x0 + 1).min(w - 1), (x0 + 2).min(w - 1)];
        let ys = [y0.saturating_sub(1), y0, (y0 + 1).min(h - 1), (y0 + 2).min(h - 1)];
        let mut out = [0.0f32; 4];
        for (&yy, &ky) in ys.iter().zip(&ky) {
            let row = &self.px[yy * w..(yy + 1) * w];
            let mut line = [0.0f32; 4];
            for (&xx, &kx) in xs.iter().zip(&kx) {
                let p = row[xx];
                for c in 0..4 {
                    line[c] += kx * p[c];
                }
            }
            for c in 0..4 {
                out[c] += ky * line[c];
            }
        }
        out
    }
}

/// The picture at `weight` (0 is `a`, 1 is `b`) between two premultiplied
/// sRGB frames: both warped along `flow` towards that instant and
/// cross-faded in linear light, each by how far its frame confirms the
/// motion; where neither does, or the two warped frames disagree, the plain
/// cross-fade of the two frames.
pub(crate) fn interpolate(a: &RgbaImage, b: &RgbaImage, flow: &Flow, weight: f64) -> RgbaImage {
    let (decode, encode) = linear_luts();
    let steps = (encode.len() - 1) as f32;
    let t = weight.clamp(0.0, 1.0) as f32;
    let (w, h) = (a.width() as usize, a.height() as usize);
    let (la, lb) = (Linear::new(a, decode), Linear::new(b, decode));
    let motions = flow.motions(t);
    let mut out = vec![0u8; w * h * 4];
    par_rows(&mut out, w * 4, |y, row| {
        for (x, o) in row.chunks_exact_mut(4).enumerate() {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let (v, ca, cb) = flow.motion(&motions, p);
            let mut trust = ca.max(cb);
            // The cross-fade, exactly as `video::mix` computes it.
            let (qa, qb) = (la.px[y * w + x], lb.px[y * w + x]);
            let mixed = |c: usize| qa[c] * (1.0 - t) + qb[c] * t;
            let mut warped = [0.0f32; 4];
            if trust > 0.0 {
                let sa = la.sample(p.0 - t * v[0], p.1 - t * v[1]);
                let sb = lb.sample(p.0 + (1.0 - t) * v[0], p.1 + (1.0 - t) * v[1]);
                if ca.min(cb) > 0.0 {
                    // Both frames confirm the motion but show different
                    // things (a flash, a wrong vector both ways): lean on
                    // the cross-fade.
                    let differ = (0..3).map(|c| (sa[c] - sb[c]).abs()).fold((sa[3] - sb[3]).abs() / 255.0, f32::max);
                    let agree = (2.0 - differ / DISAGREE).clamp(0.0, 1.0);
                    trust *= 1.0 - ca.min(cb) * (1.0 - agree);
                }
                let (wa, wb) = if ca == cb {
                    (1.0 - t, t)
                } else if (1.0 - t) * ca + t * cb > 1e-6 {
                    let (wa, wb) = ((1.0 - t) * ca, t * cb);
                    (wa / (wa + wb), wb / (wa + wb))
                } else if ca > cb {
                    // Confirmed only by the frame the instant is at the far end from.
                    (1.0, 0.0)
                } else {
                    (0.0, 1.0)
                };
                for c in 0..4 {
                    warped[c] = sa[c] * wa + sb[c] * wb;
                }
            }
            for (c, o) in o.iter_mut().enumerate() {
                let v = if trust <= 0.0 {
                    mixed(c)
                } else if trust >= 1.0 {
                    warped[c]
                } else {
                    warped[c] * trust + mixed(c) * (1.0 - trust)
                };
                *o = if c < 3 { encode[round(v.clamp(0.0, 1.0) * steps)] } else { round(v.clamp(0.0, 255.0)) as u8 };
            }
        }
    });
    RgbaImage::from_raw(w as u32, h as u32, out).expect("size matches")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::mix;

    fn hash(x: i32, y: i32, seed: u32) -> f32 {
        let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    }

    /// Smooth value noise with features about `cell` pixels across.
    fn noise(x: f32, y: f32, cell: f32, seed: u32) -> f32 {
        let (gx, gy) = (x / cell, y / cell);
        let (i, j) = (gx.floor() as i32, gy.floor() as i32);
        let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
        let (tx, ty) = (smooth(gx - i as f32), smooth(gy - j as f32));
        let top = hash(i, j, seed) * (1.0 - tx) + hash(i + 1, j, seed) * tx;
        let bottom = hash(i, j + 1, seed) * (1.0 - tx) + hash(i + 1, j + 1, seed) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// A colourful texture defined everywhere in the plane.
    fn texture(x: f32, y: f32) -> [u8; 4] {
        let c = |seed: u32| ((0.7 * noise(x, y, 6.0, seed) + 0.3 * noise(x, y, 2.5, seed + 7)) * 255.0).round() as u8;
        [c(1), c(2), c(3), 255]
    }

    /// The texture moved by `(dx, dy)` pixels.
    fn shifted(w: u32, h: u32, dx: f32, dy: f32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| image::Rgba(texture(x as f32 + 0.5 - dx, y as f32 + 0.5 - dy)))
    }

    /// Root mean square difference of the colour bytes inside a margin.
    fn rms(a: &RgbaImage, b: &RgbaImage, margin: u32) -> f64 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in margin..a.height() - margin {
            for x in margin..a.width() - margin {
                for c in 0..3 {
                    let e = a.get_pixel(x, y)[c] as f64 - b.get_pixel(x, y)[c] as f64;
                    sum += e * e;
                    n += 1.0;
                }
            }
        }
        (sum / n).sqrt()
    }

    /// Mean squared horizontal and vertical difference of neighbouring bytes inside a margin.
    fn gradient_energy(img: &RgbaImage, margin: u32) -> f64 {
        let (mut sum, mut n) = (0.0, 0.0);
        for y in margin..img.height() - margin {
            for x in margin..img.width() - margin {
                for c in 0..3 {
                    let v = img.get_pixel(x, y)[c] as f64;
                    let (dx, dy) = (img.get_pixel(x + 1, y)[c] as f64 - v, img.get_pixel(x, y + 1)[c] as f64 - v);
                    sum += dx * dx + dy * dy;
                    n += 1.0;
                }
            }
        }
        sum / n
    }

    const W: u32 = 128;
    const H: u32 = 96;
    const MARGIN: u32 = 12;

    #[test]
    fn a_translation_is_recovered_both_ways() {
        let (d0, d1) = (3.0, -2.0);
        let flow = estimate(&shifted(W, H, 0.0, 0.0), &shifted(W, H, d0, d1));
        let (mut sum, mut n) = (0.0, 0.0);
        for (grid, sign) in [(&flow.forward, 1.0), (&flow.backward, -1.0)] {
            for j in 1..grid.gh - 1 {
                for i in 1..grid.gw - 1 {
                    let v = grid.v[j * grid.gw + i];
                    sum += (v[0] - sign * d0).powi(2) + (v[1] - sign * d1).powi(2);
                    n += 1.0;
                }
            }
        }
        let error = (sum / n).sqrt();
        assert!(error < 0.25, "vector RMS error {error} px");
        let (fa, fb) = flow.trust();
        assert!(fa > 0.85 && fb > 0.85, "trust {fa} {fb}");
    }

    #[test]
    fn halfway_matches_the_half_shift_within_a_pixel() {
        for (d0, d1) in [(4.0, 0.0), (3.0, 1.0), (-5.0, 2.5)] {
            let (a, b) = (shifted(W, H, 0.0, 0.0), shifted(W, H, d0, d1));
            let flow = estimate(&a, &b);
            // The displacement the warp uses, against the true half shift.
            let motions = flow.motions(0.5);
            let (mut sum, mut n) = (0.0, 0.0);
            for y in MARGIN..H - MARGIN {
                for x in MARGIN..W - MARGIN {
                    let (v, ..) = flow.motion(&motions, (x as f32 + 0.5, y as f32 + 0.5));
                    let (to_a, to_b) = ([-0.5 * v[0], -0.5 * v[1]], [0.5 * v[0], 0.5 * v[1]]);
                    sum += (to_a[0] + d0 / 2.0).powi(2) + (to_a[1] + d1 / 2.0).powi(2) + (to_b[0] - d0 / 2.0).powi(2) + (to_b[1] - d1 / 2.0).powi(2);
                    n += 2.0;
                }
            }
            let error = (sum / n).sqrt();
            assert!(error < 1.0, "shift ({d0}, {d1}): displacement RMS error {error} px");
            // The picture is the texture moved half way, far closer than the cross-fade.
            let truth = shifted(W, H, d0 / 2.0, d1 / 2.0);
            let out = interpolate(&a, &b, &flow, 0.5);
            let mixed = mix(&a, &b, 0.5);
            let (e_flow, e_mix) = (rms(&out, &truth, MARGIN), rms(&mixed, &truth, MARGIN));
            assert!(e_flow < e_mix / 3.0, "shift ({d0}, {d1}): flow error {e_flow}, mix error {e_mix}");
            // Sharper than the cross-fade, about as sharp as the truth.
            let (g_flow, g_mix, g_truth) = (gradient_energy(&out, MARGIN), gradient_energy(&mixed, MARGIN), gradient_energy(&truth, MARGIN));
            assert!(g_flow > 1.3 * g_mix && g_flow > 0.8 * g_truth, "gradient energy flow {g_flow}, mix {g_mix}, truth {g_truth}");
        }
    }

    #[test]
    fn identical_frames_give_the_cross_fade() {
        let a = shifted(W, H, 0.0, 0.0);
        let flow = estimate(&a, &a);
        assert!(flow.forward.v.iter().chain(&flow.backward.v).all(|v| *v == [0.0, 0.0]));
        for w in [0.25, 0.5, 0.9] {
            assert_eq!(interpolate(&a, &a, &flow, w), mix(&a, &a, w));
        }
    }

    #[test]
    fn unrelated_frames_are_not_trusted() {
        // A cut: the second frame is another texture.
        let a = shifted(W, H, 0.0, 0.0);
        let b = RgbaImage::from_fn(W, H, |x, y| {
            let v = (noise(x as f32 + 1000.0, y as f32, 5.0, 11) * 255.0) as u8;
            image::Rgba([v, 255 - v, v / 2, 255])
        });
        let flow = estimate(&a, &b);
        let (fa, fb) = flow.trust();
        assert!(fa < 0.5 && fb < 0.5, "trust {fa} {fb}");
        let out = interpolate(&a, &b, &flow, 0.5);
        let off = rms(&out, &mix(&a, &b, 0.5), 0);
        assert!(off < 20.0);
    }

    #[test]
    fn a_moving_square_over_a_still_background() {
        // Occlusion: a textured square moves 6 px right over a still texture.
        let scene = |dx: f32| {
            RgbaImage::from_fn(W, H, |x, y| {
                let (fx, fy) = (x as f32 + 0.5 - dx, y as f32 + 0.5);
                if (40.0..80.0).contains(&fx) && (28.0..68.0).contains(&fy) {
                    let v = texture(fx + 300.0, fy);
                    image::Rgba([v[2], v[0], v[1], 255])
                } else {
                    image::Rgba(texture(x as f32 + 0.5, y as f32 + 0.5))
                }
            })
        };
        let (a, b, truth) = (scene(0.0), scene(6.0), scene(3.0));
        let flow = estimate(&a, &b);
        let out = interpolate(&a, &b, &flow, 0.5);
        let (e_flow, e_mix) = (rms(&out, &truth, 4), rms(&mix(&a, &b, 0.5), &truth, 4));
        assert!(e_flow < e_mix / 2.0, "flow error {e_flow}, mix error {e_mix}");
    }

    #[test]
    fn the_same_pixels_on_any_number_of_threads() {
        let (a, b) = (shifted(W, H, 0.0, 0.0), shifted(W, H, 2.5, -1.5));
        let run = |n: usize| {
            THREADS.store(n, Ordering::Relaxed);
            let flow = estimate(&a, &b);
            let out = interpolate(&a, &b, &flow, 0.3);
            THREADS.store(0, Ordering::Relaxed);
            (format!("{:?}", flow.forward.v), out)
        };
        let one = run(1);
        assert_eq!(one, run(3));
        assert_eq!(one, run(8));
        assert_eq!(one, run(1));
    }

    #[test]
    fn large_frames_are_estimated_at_a_reduced_size() {
        // 2000 px wide: estimated at half size, vectors in frame pixels.
        let (w, h) = (2000, 80);
        let flow = estimate(&shifted(w, h, 0.0, 0.0), &shifted(w, h, 8.0, 0.0));
        assert_eq!(flow.scale, 2.0);
        let (v, ..) = flow.motion(&flow.motions(0.5), (1000.0, 40.0));
        let (to_a, to_b) = ([-0.5 * v[0], -0.5 * v[1]], [0.5 * v[0], 0.5 * v[1]]);
        assert!((to_a[0] + 4.0).abs() < 0.5 && (to_b[0] - 4.0).abs() < 0.5 && to_a[1].abs() < 0.5, "{to_a:?} {to_b:?}");
    }

    /// Cost at 1080p: `cargo test --release -p edits-render --lib flow_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn flow_cost_at_1080p() {
        let (w, h) = (1920, 1080);
        // A pan, and a textured square (600 px) moving over a still background.
        let square = |dx: f32| {
            RgbaImage::from_fn(w, h, |x, y| {
                let (fx, fy) = (x as f32 + 0.5 - dx, y as f32 + 0.5);
                let v = if (600.0..1200.0).contains(&fx) && (200.0..800.0).contains(&fy) { texture(fx * 0.5 + 300.0, fy * 0.5) } else { texture(x as f32 * 0.5, y as f32 * 0.5) };
                image::Rgba(v)
            })
        };
        let scenes = [("pan", shifted(w, h, 0.0, 0.0), shifted(w, h, 7.0, -3.0)), ("square", square(0.0), square(14.0))];
        for threads_wanted in [1, 0] {
            THREADS.store(threads_wanted, Ordering::Relaxed);
            for (name, a, b) in &scenes {
                let mut times = vec![];
                for _ in 0..3 {
                    let start = std::time::Instant::now();
                    let flow = estimate(a, b);
                    let estimated = start.elapsed();
                    let _ = interpolate(a, b, &flow, 0.5);
                    let warped = start.elapsed() - estimated;
                    let start = std::time::Instant::now();
                    let _ = mix(a, b, 0.5);
                    times.push((estimated, warped, start.elapsed()));
                }
                let best = |k: usize| times.iter().map(|t| [t.0, t.1, t.2][k]).min().unwrap();
                eprintln!("1080p {name} on {} threads: flow {:?}, warp {:?}, mix {:?}", threads(), best(0), best(1), best(2));
            }
        }
        THREADS.store(0, Ordering::Relaxed);
    }
}
