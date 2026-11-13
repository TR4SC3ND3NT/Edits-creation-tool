//! `.cube` colour lookup tables (the Adobe / Resolve text format), for the
//! `lut` effect.
//!
//! A file holds an optional 1D table (`LUT_1D_SIZE n`, n rows) followed by an
//! optional 3D table (`LUT_3D_SIZE n`, n^3 rows, red varying fastest), each
//! row three numbers. `DOMAIN_MIN` / `DOMAIN_MAX` (or the older
//! `LUT_1D_INPUT_RANGE` / `LUT_3D_INPUT_RANGE`) give the input range, 0..1 by
//! default. `TITLE`, `#` comments and unknown keywords are ignored.
//!
//! The format rules follow EffectCraft's `parse_cube`
//! (https://github.com/storytold/effectcraft, `crates/effects/src/utility.rs`),
//! Copyright (c) 2026 ArtCraft Team and the EffectCraft contributors, MIT
//! License (see THIRD_PARTY.md). Rewritten: errors name the line and what is
//! wrong, and a table can be baked into one 3D table for the GPU.

use std::path::Path;

/// The largest 3D table accepted (256^3 rows is 16.7 million).
pub const MAX_3D: usize = 256;
/// The size of the 3D table a 1D-only file (or one with a domain other than
/// 0..1) is baked into.
pub const BAKE_SIZE: usize = 33;

#[derive(Clone, Debug, PartialEq)]
pub struct Cube {
    pub title: Option<String>,
    /// The 1D table (applied first, per channel), empty when there is none.
    pub table_1d: Vec<[f32; 3]>,
    /// The 3D table, red fastest, then green, then blue; empty when there is none.
    pub table_3d: Vec<[f32; 3]>,
    /// Size of the 3D table along each axis (0 when there is none).
    pub size_3d: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
}

/// Parse the text of a `.cube` file. The error names the line and the fix.
pub fn parse(text: &str) -> Result<Cube, String> {
    let (mut n1, mut n3) = (0usize, 0usize);
    let (mut dmin, mut dmax) = ([0.0f32; 3], [1.0f32; 3]);
    let mut title = None;
    let mut rows: Vec<[f32; 3]> = Vec::new();
    let triple = |rest: &[&str], line: usize| -> Result<[f32; 3], String> {
        let v: Vec<f32> = rest.iter().map(|s| s.parse::<f32>()).collect::<Result<_, _>>().map_err(|_| format!("line {line}: expected three numbers, got \"{}\"", rest.join(" ")))?;
        match v.as_slice() {
            [r, g, b] if r.is_finite() && g.is_finite() && b.is_finite() => Ok([*r, *g, *b]),
            _ => Err(format!("line {line}: expected three finite numbers, got \"{}\"", rest.join(" "))),
        }
    };
    let size = |rest: &[&str], line: usize| -> Result<usize, String> {
        match rest {
            [n] => n.parse::<usize>().map_err(|_| format!("line {line}: a table size must be a whole number, not \"{n}\"")),
            _ => Err(format!("line {line}: a table size is one whole number")),
        }
    };
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        let l = raw.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        let words: Vec<&str> = l.split_whitespace().collect();
        let (key, rest) = (words[0], &words[1..]);
        match key {
            "TITLE" => title = Some(l["TITLE".len()..].trim().trim_matches('"').to_string()),
            "LUT_1D_SIZE" => n1 = size(rest, line)?,
            "LUT_3D_SIZE" => n3 = size(rest, line)?,
            "DOMAIN_MIN" => dmin = triple(rest, line)?,
            "DOMAIN_MAX" => dmax = triple(rest, line)?,
            "LUT_1D_INPUT_RANGE" | "LUT_3D_INPUT_RANGE" => match rest {
                [lo, hi] => {
                    let (lo, hi) = (lo.parse::<f32>(), hi.parse::<f32>());
                    let (Ok(lo), Ok(hi)) = (lo, hi) else { return Err(format!("line {line}: {key} takes two numbers")) };
                    (dmin, dmax) = ([lo; 3], [hi; 3]);
                }
                _ => return Err(format!("line {line}: {key} takes two numbers")),
            },
            k if k.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
            _ => rows.push(triple(&words, line)?),
        }
    }
    if n1 == 0 && n3 == 0 {
        return Err("no LUT_1D_SIZE or LUT_3D_SIZE line: this is not a .cube lookup table".into());
    }
    if n1 == 1 || n3 == 1 {
        return Err("a table needs at least 2 entries per axis".into());
    }
    if n3 > MAX_3D {
        return Err(format!("LUT_3D_SIZE {n3} is too large (at most {MAX_3D})"));
    }
    let want = n1 + n3 * n3 * n3;
    if rows.len() != want {
        return Err(format!("the sizes ask for {want} rows of three numbers but the file has {}", rows.len()));
    }
    if (0..3).any(|c| dmax[c] <= dmin[c]) {
        return Err(format!("DOMAIN_MAX {dmax:?} must be above DOMAIN_MIN {dmin:?} in every channel"));
    }
    let table_3d = rows.split_off(n1);
    Ok(Cube { title, table_1d: rows, table_3d, size_3d: n3, domain_min: dmin, domain_max: dmax })
}

/// Read and parse a `.cube` file.
pub fn load(path: &Path) -> Result<Cube, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

impl Cube {
    /// The table applied to one colour (input in the file's domain).
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let mut v = [0.0f32; 3];
        for i in 0..3 {
            v[i] = ((c[i] - self.domain_min[i]) / (self.domain_max[i] - self.domain_min[i])).clamp(0.0, 1.0);
        }
        if !self.table_1d.is_empty() {
            let n = self.table_1d.len();
            for (i, x) in v.iter_mut().enumerate() {
                let p = *x * (n - 1) as f32;
                let k = (p.floor() as usize).min(n - 2);
                let f = p - k as f32;
                *x = self.table_1d[k][i] * (1.0 - f) + self.table_1d[k + 1][i] * f;
            }
            if self.table_3d.is_empty() {
                return v;
            }
            v = v.map(|x| x.clamp(0.0, 1.0));
        }
        let n = self.size_3d;
        let at = |r: usize, g: usize, b: usize| self.table_3d[r + n * (g + n * b)];
        let mut k = [0usize; 3];
        let mut f = [0.0f32; 3];
        for i in 0..3 {
            let p = v[i] * (n - 1) as f32;
            k[i] = (p.floor() as usize).min(n - 2);
            f[i] = p - k[i] as f32;
        }
        let mut out = [0.0f32; 3];
        for (corner, w) in (0..8).map(|j| {
            let bit = |a: usize| (j >> a) & 1;
            let w = (0..3).map(|a| if bit(a) == 1 { f[a] } else { 1.0 - f[a] }).product::<f32>();
            ([k[0] + bit(0), k[1] + bit(1), k[2] + bit(2)], w)
        }) {
            let c = at(corner[0], corner[1], corner[2]);
            for i in 0..3 {
                out[i] += c[i] * w;
            }
        }
        out
    }

    /// One 3D table over inputs 0..1 that does what the whole file does:
    /// `(size, rows)`, red fastest. A plain 3D table over 0..1 is returned
    /// as it is; anything else is sampled on a [`BAKE_SIZE`] grid (or the
    /// file's own 3D size).
    pub fn to_3d(&self) -> (usize, Vec<[f32; 3]>) {
        let unit_domain = self.domain_min == [0.0; 3] && self.domain_max == [1.0; 3];
        if self.table_1d.is_empty() && unit_domain {
            return (self.size_3d, self.table_3d.clone());
        }
        let n = if self.size_3d > 0 { self.size_3d } else { BAKE_SIZE };
        let mut rows = Vec::with_capacity(n * n * n);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let x = [r, g, b].map(|i| i as f32 / (n - 1) as f32);
                    let input: [f32; 3] = std::array::from_fn(|i| self.domain_min[i] + x[i] * (self.domain_max[i] - self.domain_min[i]));
                    rows.push(self.apply(input));
                }
            }
        }
        (n, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3D `.cube` text of size `n` whose entries are `f` of the grid point.
    fn cube3(n: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> String {
        let mut s = format!("# generated\nTITLE \"test\"\nLUT_3D_SIZE {n}\n\n");
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let c = f([r, g, b].map(|i| i as f32 / (n - 1) as f32));
                    s.push_str(&format!("{} {} {}\n", c[0], c[1], c[2]));
                }
            }
        }
        s
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-4)
    }

    #[test]
    fn identity_and_inverting_tables() {
        for n in [2, 17] {
            let id = parse(&cube3(n, |c| c)).unwrap();
            assert_eq!((id.size_3d, id.table_3d.len(), id.title.as_deref()), (n, n * n * n, Some("test")));
            for c in [[0.0, 0.0, 0.0], [0.3, 0.6, 0.9], [1.0, 0.5, 0.25]] {
                assert!(close(id.apply(c), c), "{n}: {c:?} -> {:?}", id.apply(c));
            }
            assert_eq!(id.to_3d(), (n, id.table_3d.clone()), "a plain table is uploaded as it is");
        }
        let inv = parse(&cube3(2, |c| c.map(|v| 1.0 - v))).unwrap();
        assert!(close(inv.apply([0.2, 0.7, 1.0]), [0.8, 0.3, 0.0]));
        // Red and blue swapped: the order of the rows is red fastest.
        let swap = parse(&cube3(3, |c| [c[2], c[1], c[0]])).unwrap();
        assert!(close(swap.apply([1.0, 0.5, 0.0]), [0.0, 0.5, 1.0]));
    }

    #[test]
    fn one_d_tables_domains_and_baking() {
        let text = "LUT_1D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n0 0 0\n0.25 0.5 1\n1 1 1\n";
        let c = parse(text).unwrap();
        assert_eq!((c.table_1d.len(), c.size_3d), (3, 0));
        // Input 1 is the middle of the domain 0..2.
        assert!(close(c.apply([1.0, 1.0, 1.0]), [0.25, 0.5, 1.0]));
        assert!(close(c.apply([0.5, 0.5, 0.5]), [0.125, 0.25, 0.5]));
        let (n, rows) = c.to_3d();
        assert_eq!((n, rows.len()), (BAKE_SIZE, BAKE_SIZE.pow(3)));
        // The baked table takes inputs 0..1 across the file's domain.
        let last = BAKE_SIZE * BAKE_SIZE * BAKE_SIZE - 1;
        assert!(close(rows[last], [1.0, 1.0, 1.0]) && close(rows[0], [0.0, 0.0, 0.0]));
        // The old range keyword.
        let c = parse("LUT_3D_INPUT_RANGE 0 2\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n").unwrap();
        assert!(close(c.apply([1.0, 0.0, 2.0]), [0.5, 0.0, 1.0]));
    }

    #[test]
    fn bad_files_say_what_is_wrong() {
        let says = |text: &str, needle: &str| {
            let e = parse(text).unwrap_err();
            assert!(e.contains(needle), "{e:?} should say {needle:?}");
        };
        says("0 0 0\n", "not a .cube");
        says("LUT_3D_SIZE 2\n0 0 0\n1 0 0\n", "ask for 8 rows");
        says("LUT_3D_SIZE 2\n0 0 zero\n", "line 2: expected three numbers");
        says("LUT_3D_SIZE 2\n0 0\n", "line 2: expected three finite numbers");
        says("LUT_3D_SIZE two\n", "line 1: a table size");
        says("LUT_3D_SIZE 1\n0 0 0\n", "at least 2");
        says("LUT_3D_SIZE 300\n", "too large");
        says("LUT_1D_SIZE 2\nDOMAIN_MIN 1 0 0\nDOMAIN_MAX 1 1 1\n0 0 0\n1 1 1\n", "DOMAIN_MAX");
        assert!(load(Path::new("/nonexistent/x.cube")).unwrap_err().contains("cannot read"));
    }
}
