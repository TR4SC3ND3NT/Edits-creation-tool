//! Precomps: a `comp` layer's picture is its comp's stack, rendered through
//! [`Renderer::render_stack_over`] at the comp time the layer shows.
//!
//! Each comp is prepared as a [`Scene`] of its own (its project is
//! `Project::comp_project`: the comp's size, duration and layers, the default
//! camera), sharing the pictures, clips, shaders and fonts of the project. To
//! render a comp the renderer swaps that scene in, so everything that reads
//! the scene (the evaluator, mattes, the camera, layer ends, the aspect)
//! sees the comp, and swaps the parent back afterwards.
//!
//! The comp's pixel size is its size in project pixels times the output
//! scale; a comp of the project's size renders at exactly the output size.
//! Pictures are cached for the output frame by (comp, comp time), so a comp
//! drawn several times at one instant (repeated panels, a matte) renders once.

use crate::assets;
use crate::renderer::{Picture, Renderer, Scene};
use anyhow::{anyhow, bail, Result};
use edits_core::{CompLayer, Evaluator, Layer, Project};
use std::collections::HashMap;
use std::sync::Arc;

/// Comp pictures kept during one output frame. More would hold one
/// frame-sized texture each for the rest of the frame.
const CACHED: usize = 4;

/// Comp pictures of the current output frame, most recent last.
#[derive(Default)]
pub(crate) struct CompCache {
    entries: Vec<((String, u64, u32, u32), Picture)>,
    /// The project's size and the output frame's size, set by each frame.
    pub root: (u32, u32, u32, u32),
}

/// The scenes of all of `root`'s comps (so that any list of layers can be
/// rendered), each with the scenes of the comps it draws in turn. A comp on
/// a cycle is left out where it would contain itself (drawing it fails;
/// `edits check` names the cycle).
pub(crate) fn comp_scenes(root: &Scene, max_side: u32) -> Result<HashMap<String, Arc<Scene>>> {
    let mut built = HashMap::new();
    let mut out = HashMap::new();
    for name in root.project.comps.keys() {
        if let Some(scene) = build(root, name, &mut built, &mut Vec::new(), max_side)? {
            out.insert(name.to_string(), scene);
        }
    }
    Ok(out)
}

fn build(root: &Scene, name: &str, built: &mut HashMap<String, Arc<Scene>>, open: &mut Vec<String>, max_side: u32) -> Result<Option<Arc<Scene>>> {
    if let Some(s) = built.get(name) {
        return Ok(Some(s.clone()));
    }
    if open.iter().any(|n| n == name) {
        return Ok(None);
    }
    let Some(project) = root.project.comp_project(name) else { return Ok(None) };
    let s = &project.settings;
    if s.width == 0 || s.height == 0 || s.width > max_side || s.height > max_side {
        bail!("comp \"{name}\": size {}x{} is not renderable (this adapter supports 1..{max_side} pixels per side)", s.width, s.height);
    }
    open.push(name.to_string());
    let children: Vec<String> = Project::comps_used(&project.layers).into_iter().map(str::to_string).collect();
    let mut comps = HashMap::new();
    for child in children {
        if let Some(scene) = build(root, &child, built, open, max_side)? {
            comps.insert(child, scene);
        }
    }
    open.pop();
    let mut scene = root.derive(project);
    scene.comps = comps;
    let scene = Arc::new(scene);
    built.insert(name.to_string(), scene.clone());
    Ok(Some(scene))
}

/// True when `mvp` puts the unit quad exactly on the frame (upright, edge
/// to edge): a comp of the frame's size is then shown pixel for pixel.
pub(crate) fn maps_onto_frame(mvp: glam::DMat4) -> bool {
    let corners = [(-0.5, -0.5, -1.0, 1.0), (0.5, -0.5, 1.0, 1.0), (0.5, 0.5, 1.0, -1.0), (-0.5, 0.5, -1.0, -1.0)];
    corners.iter().all(|&(x, y, nx, ny)| {
        let c = mvp * glam::DVec4::new(x, y, 0.0, 1.0);
        c.w > 1e-9 && (c.x / c.w - nx).abs() < 1e-6 && (c.y / c.w - ny).abs() < 1e-6 && (0.0..=c.w).contains(&c.z)
    })
}

impl Renderer {
    /// The picture of comp layer `layer` (in the stack of `scene`, evaluated
    /// by `ev`) at time `t`: its comp's stack at the mapped comp time, over
    /// the comp's background or transparent, with the comp's aspect.
    pub(crate) fn comp_picture(&mut self, ev: &Evaluator, scene: &Scene, layer: &Layer, c: &CompLayer, t: f64) -> Result<Picture> {
        let sub = scene.comps.get(&c.comp).cloned().ok_or_else(|| {
            if scene.project.comps.contains_key(&c.comp) {
                anyhow!("comp \"{}\" contains itself (run `edits check`)", c.comp)
            } else {
                anyhow!("no comp \"{}\" (run `edits check`)", c.comp)
            }
        })?;
        let local = ev.comp_time(layer, c, t, sub.project.settings.duration);
        let (w, h) = self.comp_size(&sub.project);
        let max = self.max_texture_side();
        if w > max || h > max {
            bail!("comp \"{}\" would be {w}x{h} pixels at this scale; the limit is {max} per side", c.comp);
        }
        let key = (c.comp.clone(), local.to_bits(), w, h);
        if let Some((_, pic)) = self.comp_cache.entries.iter().find(|(k, _)| *k == key) {
            return Ok(pic.clone());
        }
        let background = match sub.project.comps.get(&c.comp).and_then(|comp| comp.background) {
            Some(bg) => {
                let bg = bg.map(|v| assets::srgb_to_linear(v.clamp(0.0, 1.0)));
                [bg[0], bg[1], bg[2], 1.0]
            }
            None => [0.0; 4],
        };
        let parent = self.scene.replace(sub.clone());
        let drawn = self.render_stack_over(&sub.project.layers, local, w, h, background);
        self.scene = parent;
        let pic = Picture { tex: drawn?, aspect: w as f64 / h as f64 };
        self.comp_cache.entries.push((key, pic.clone()));
        if self.comp_cache.entries.len() > CACHED {
            // Its quads were drawn when it was made: the texture can go back to the pool.
            let (_, old) = self.comp_cache.entries.remove(0);
            self.release(old.tex);
        }
        Ok(pic)
    }

    /// The pixel size of a comp seen as `project`: its size at the scale of
    /// the output frame, which a comp of the project's size matches exactly.
    fn comp_size(&self, project: &Project) -> (u32, u32) {
        let (pw, ph, w, h) = self.comp_cache.root;
        let s = &project.settings;
        if (s.width, s.height) == (pw, ph) || ph == 0 {
            return (w.max(1), h.max(1));
        }
        let k = h as f64 / ph as f64;
        (((s.width as f64 * k).round() as u32).max(1), ((s.height as f64 * k).round() as u32).max(1))
    }

    /// Give the comp pictures of the last frame back to the texture pool.
    pub(crate) fn drop_comp_pictures(&mut self) {
        for (_, pic) in std::mem::take(&mut self.comp_cache.entries) {
            self.release(pic.tex);
        }
    }
}
