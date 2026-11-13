//! Layers as functions of time.
//!
//! [`Renderer::layer_content`] produces one layer's own picture at any
//! instant; [`Renderer::render_stack`] composites a list of layers into a
//! texture. A frame is `render_stack(project.layers, t)` at each motion blur
//! instant, averaged. Everything that renders a layer somewhere else (an echo
//! at an earlier time, a matte, a precomp, a video frame) goes through these
//! two functions, so a layer looks the same wherever it is used.
//!
//! [`layer_signature`] says what a layer's picture at `t` depends on: two
//! instants with equal signatures give the same picture, which is how still
//! moments cost one motion blur sample, and what a cache of layer pictures
//! can be keyed by.

use crate::assets;
use crate::fx::Step;
use crate::geometry;
use crate::renderer::{Quad, Renderer, Scene, Sink, Source, Tex};
use anyhow::{anyhow, bail, Context, Result};
use edits_core::{Blend, Evaluator, Layer, LayerKind, Project, ResolvedLayer, ResolvedMask, ResolvedTransition};

/// Deepest chain of track mattes (a matte whose source has a matte, ...).
/// `check` reports cycles; this keeps an unchecked project from recursing forever.
const MAX_MATTE_DEPTH: u32 = 16;

/// One layer evaluated at an instant: everything needed to draw it.
pub(crate) struct LayerAt {
    pub resolved: ResolvedLayer,
    pub opacity: f32,
    pub steps: Vec<Step>,
    pub transition: Option<ResolvedTransition>,
    /// Its masks at the layer's time, in order.
    pub masks: Vec<ResolvedMask>,
}

impl Renderer {
    /// Set the values every pass reads (size, time) for project time `t` of
    /// the output frame being rendered.
    pub(crate) fn set_time(&mut self, project: &Project, t: f64, w: u32, h: u32) {
        let s = &project.settings;
        let seconds = project.to_seconds(t);
        let beats = s.bpm.filter(|b| *b > 0.0).map(|bpm| (seconds - s.beat_offset) * bpm / 60.0).unwrap_or(0.0);
        let aspect = s.width as f64 / s.height as f64;
        self.ctx = crate::renderer::Ctx { w, h, time: [seconds as f32, beats as f32, self.frame as f32, aspect as f32] };
    }

    /// Evaluate `layer` at `t` for a `w` x `h` target (the context must already be
    /// set for the layer's time, see [`Evaluator::layer_time`]). `None` when the
    /// layer is not visible then. Transform and effects are evaluated at the
    /// layer's time (posterized by its `fps`), the transition at `t`.
    fn layer_at(&mut self, ev: &Evaluator, layer: &Layer, t: f64) -> Result<Option<LayerAt>> {
        if !ev.active(layer, t) {
            return Ok(None);
        }
        let lt = ev.layer_time(layer, t);
        let resolved = ev.layer(layer, lt);
        let opacity = resolved.opacity as f32;
        if opacity <= 0.0 {
            return Ok(None);
        }
        let effects: Vec<_> = layer.effects.iter().filter(|e| e.enabled).map(|e| ev.effect(e, lt)).collect();
        let steps = self.plan_effects(&layer.id, &effects, self.ctx.w as f32, self.ctx.h as f32)?;
        Ok(Some(LayerAt { resolved, opacity, steps, transition: ev.transition(layer, t), masks: ev.masks(layer, lt) }))
    }

    /// The quads of a content layer at `t`, through the camera, and whether
    /// they need a depth buffer. A video layer decodes its frame at `t` here.
    fn quads_at(&mut self, ev: &Evaluator, layer: &Layer, at: &LayerAt, t: f64) -> Result<(Vec<Quad>, bool)> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let aspect = self.ctx.time[3] as f64;
        let vp = geometry::view_projection(&ev.camera(t), aspect);
        if let LayerKind::Particles(ps) = &layer.kind {
            return self.particle_quads(&scene, ev, layer, ps, vp * geometry::model(&at.resolved), t).with_context(|| format!("layer \"{}\"", layer.id));
        }
        let mut frame = None;
        if let LayerKind::Comp(c) = &layer.kind {
            frame = Some(self.comp_picture(ev, &scene, layer, c, t).with_context(|| format!("layer \"{}\"", layer.id))?);
        }
        if let LayerKind::Video(v) = &layer.kind {
            if let Some(clip) = scene.clips.get(&v.slot) {
                if let Some(sf) = ev.source_frame(layer, v, t, clip.info()) {
                    frame = Some(self.video_picture(clip, sf).with_context(|| format!("layer \"{}\"", layer.id))?);
                }
            }
        }
        Ok(self.layer_quads(&scene, layer, self.layer_mvp(ev, at, t), aspect, frame.as_ref()))
    }

    /// Layer space through the camera at `t` into clip space.
    fn layer_mvp(&self, ev: &Evaluator, at: &LayerAt, t: f64) -> glam::DMat4 {
        let aspect = self.ctx.time[3] as f64;
        geometry::view_projection(&ev.camera(t), aspect) * geometry::model(&at.resolved)
    }

    /// A content layer's picture at `t` in a fresh frame-sized texture: its
    /// quads or the glyphs of a text layer, then [`Self::finish_content`].
    /// `layers` is the stack the layer is in (where its matte is looked up).
    fn content_at(&mut self, ev: &Evaluator, layers: &[Layer], layer: &Layer, at: &LayerAt, t: f64, steps: &[Step]) -> Result<Tex> {
        let drawn = match &layer.kind {
            LayerKind::Text(text) => {
                let mvp = self.layer_mvp(ev, at, t);
                self.draw_text(&layer.id, text, mvp, ev.layer_time(layer, t))?
            }
            _ => {
                let (quads, depth) = self.quads_at(ev, layer, at, t)?;
                self.draw_quads(&quads, depth)
            }
        };
        self.finish_content(ev, layers, layer, at, t, steps, drawn)
    }

    /// What follows drawing a layer, in the order of After Effects: its
    /// masks cut the drawn picture, `steps` (effects) run on the result, and
    /// the track matte multiplies what comes out.
    #[allow(clippy::too_many_arguments)]
    fn finish_content(&mut self, ev: &Evaluator, layers: &[Layer], layer: &Layer, at: &LayerAt, t: f64, steps: &[Step], drawn: Tex) -> Result<Tex> {
        let mut content = if at.masks.is_empty() { drawn } else { self.apply_masks(drawn, &at.masks, self.layer_mvp(ev, at, t)) };
        if let Some(out) = self.run_effects(&content, steps) {
            self.release(std::mem::replace(&mut content, out));
        }
        self.matte(layers, layer, t, content)
    }

    /// `picture` (this layer's, or what its adjustment effects made) times
    /// the layer's track matte, if it has one: the matte source's own
    /// picture at `t`, rendered here with its transform, masks, effects and
    /// matte. `picture` is released when a new texture is returned.
    fn matte(&mut self, layers: &[Layer], layer: &Layer, t: f64, picture: Tex) -> Result<Tex> {
        let Some(m) = &layer.matte else { return Ok(picture) };
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let Some(source) = edits_core::mask::find_layer(layers, &scene.project, &m.layer) else {
            bail!("layer \"{}\": no layer \"{}\" to use as a matte (run `edits check`)", layer.id, m.layer);
        };
        if self.matte_depth >= MAX_MATTE_DEPTH {
            bail!("layer \"{}\": track mattes are nested more than {MAX_MATTE_DEPTH} deep; they probably form a cycle (run `edits check`)", layer.id);
        }
        self.matte_depth += 1;
        let drawn = self.content_in(layers, source, t);
        self.matte_depth -= 1;
        let (matte, opacity) = match drawn.with_context(|| format!("matte of layer \"{}\"", layer.id))? {
            Some((tex, opacity)) => (Some(tex), opacity),
            None => (None, 0.0),
        };
        Ok(self.apply_matte(picture, matte, m.mode, opacity))
    }

    /// A layer's own picture at project time `t`: its quads in the frame
    /// (through the camera), with its whole effect chain applied, in a
    /// frame-sized working texture (linear, premultiplied). Opacity, blend
    /// mode and transition are not applied: they belong to compositing.
    ///
    /// `None` when the layer is not visible at `t` and for adjustment layers,
    /// which have no picture of their own. The caller owns the texture and
    /// gives it back with [`Renderer::release`]. The pass context is restored
    /// afterwards, so this can be called in the middle of another stack (an
    /// echo renders the same layer at earlier times).
    pub(crate) fn layer_content(&mut self, layer: &Layer, t: f64) -> Result<Option<Tex>> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        Ok(self.content_in(&scene.project.layers, layer, t)?.map(|(tex, _)| tex))
    }

    /// [`Self::layer_content`] for a layer of the stack `layers`, with the
    /// layer's opacity at `t`.
    fn content_in(&mut self, layers: &[Layer], layer: &Layer, t: f64) -> Result<Option<(Tex, f32)>> {
        if matches!(layer.kind, LayerKind::Adjustment) {
            return Ok(None);
        }
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let saved = self.ctx;
        let ev = Evaluator::new(&scene.project);
        self.set_time(&scene.project, ev.layer_time(layer, t), saved.w.max(1), saved.h.max(1));
        let result = self.layer_at(&ev, layer, t).and_then(|at| match at {
            Some(at) => Ok(Some((self.content_at(&ev, layers, layer, &at, t, &at.steps)?, at.opacity))),
            None => Ok(None),
        });
        self.ctx = saved;
        result
    }

    /// Composite `layers` (bottom to top) at project time `t` over the
    /// project's background into a fresh `w` x `h` working texture.
    pub(crate) fn render_stack(&mut self, layers: &[Layer], t: f64, w: u32, h: u32) -> Result<Tex> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let bg = scene.project.settings.background.map(|c| assets::srgb_to_linear(c.clamp(0.0, 1.0)));
        self.render_stack_over(layers, t, w, h, [bg[0], bg[1], bg[2], 1.0])
    }

    /// [`Self::render_stack`] over a premultiplied linear `background`
    /// (transparent for a precomp).
    pub(crate) fn render_stack_over(&mut self, layers: &[Layer], t: f64, w: u32, h: u32, background: [f32; 4]) -> Result<Tex> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let project = &scene.project;
        let saved = self.ctx;
        self.set_time(project, t, w, h);
        let ev = Evaluator::new(project);
        let clear = wgpu::Color { r: background[0] as f64, g: background[1] as f64, b: background[2] as f64, a: background[3] as f64 };
        let result = self.stack(&ev, layers, t, clear, background);
        self.ctx = saved;
        result
    }

    fn stack(&mut self, ev: &Evaluator, layers: &[Layer], t: f64, background: wgpu::Color, flat: [f32; 4]) -> Result<Tex> {
        let (w, h) = (self.ctx.w, self.ctx.h);
        // `None` until something is drawn: the first layer blends over the
        // flat background colour instead of over a cleared texture.
        let mut acc: Option<Tex> = None;
        for layer in layers {
            // A hidden layer is only drawn as another layer's matte.
            if !layer.visible {
                continue;
            }
            // A layer with its own `fps` runs its passes at its own time.
            let lt = ev.layer_time(layer, t);
            let saved = self.ctx;
            if lt != t {
                self.set_time(ev.project, lt, w, h);
            }
            let drawn = self.stack_layer(ev, layers, layer, t, &mut acc, background, flat);
            self.ctx = saved;
            drawn?;
        }
        Ok(self.materialise(&mut acc, background))
    }

    /// What an adjustment layer's effects made of the picture beneath, cut
    /// to its masks and matte. `out` is released when a new texture is returned.
    fn limit_adjustment(&mut self, ev: &Evaluator, layers: &[Layer], layer: &Layer, at: &LayerAt, t: f64, out: Tex) -> Result<Tex> {
        let out = if at.masks.is_empty() { out } else { self.apply_masks(out, &at.masks, self.layer_mvp(ev, at, t)) };
        self.matte(layers, layer, t, out)
    }

    /// Draw one layer of the stack `layers` over `acc`, the picture so far.
    #[allow(clippy::too_many_arguments)]
    fn stack_layer(&mut self, ev: &Evaluator, layers: &[Layer], layer: &Layer, t: f64, acc: &mut Option<Tex>, background: wgpu::Color, flat: [f32; 4]) -> Result<()> {
        let (w, h) = (self.ctx.w, self.ctx.h);
        let Some(at) = self.layer_at(ev, layer, t)? else { return Ok(()) };
        let opacity = at.opacity;
        let adjustment = matches!(layer.kind, LayerKind::Adjustment);
        let limited = !at.masks.is_empty() || layer.matte.is_some();
        if let Some(tr) = &at.transition {
            // "from" is the frame so far; "to" is the frame with this
            // layer on it, so the end of the transition is the plain cut.
            let from = self.materialise(acc, background);
            let to = self.acquire(w, h);
            if adjustment {
                match self.run_effects(&from, &at.steps) {
                    Some(out) => {
                        let out = self.limit_adjustment(ev, layers, layer, &at, t, out)?;
                        self.compose(&to, Source::Tex(&out), &[], Sink::Over { dst: Some(&from), flat, mode: layer.blend, opacity });
                        self.release(out);
                    }
                    None => self.compose(&to, Source::Tex(&from), &[], Sink::Plain),
                }
            } else {
                let (head, tail) = crate::fx::split_tail(&at.steps);
                let content = self.content_at(ev, layers, layer, &at, t, head)?;
                self.compose(&to, Source::Tex(&content), &tail, Sink::Over { dst: Some(&from), flat, mode: layer.blend, opacity });
                self.release(content);
            }
            let out = self.acquire(w, h);
            self.transition_pass(&out, &from, &to, tr).with_context(|| format!("layer \"{}\"", layer.id))?;
            self.release(to);
            if let Some(old) = acc.replace(out) {
                self.release(old);
            }
            return Ok(());
        }
        if adjustment {
            if at.steps.is_empty() {
                return Ok(());
            }
            let base = self.materialise(acc, background);
            let Some(out) = self.run_effects(&base, &at.steps) else { return Ok(()) };
            if opacity >= 1.0 && layer.blend == Blend::Normal && !limited {
                self.release(base);
                *acc = Some(out);
            } else {
                // Masked or matted, the effects show only where the masks and matte let them.
                let out = self.limit_adjustment(ev, layers, layer, &at, t, out)?;
                self.blend_over(acc, flat, Source::Tex(&out), &[], layer.blend, opacity);
                self.release(out);
            }
            return Ok(());
        }
        // Colour effects at the end of the chain ride along with the blend
        // (they act on straight colour, so a mask or matte in between, which
        // only scales a pixel, changes nothing for them).
        let (head, tail) = crate::fx::split_tail(&at.steps);
        let content = if let LayerKind::Text(_) = &layer.kind {
            self.content_at(ev, layers, layer, &at, t, head)?
        } else {
            let (quads, depth) = self.quads_at(ev, layer, &at, t)?;
            // A flat layer that hides the whole frame has no visible edges; if
            // nothing has to look at neighbouring pixels it is drawn, graded
            // and blended in a single pass.
            if head.is_empty() && !depth && !limited && quads.len() == 1 && quads[0].covers {
                self.blend_over(acc, flat, Source::Quad(&quads[0]), &tail, layer.blend, opacity);
                return Ok(());
            }
            let drawn = self.draw_quads(&quads, depth);
            self.finish_content(ev, layers, layer, &at, t, head, drawn)?
        };
        self.blend_over(acc, flat, Source::Tex(&content), &tail, layer.blend, opacity);
        self.release(content);
        Ok(())
    }
}

/// Everything `layer`'s picture at project time `t` depends on, as raw bits,
/// or `None` when the layer is not visible then. Two instants with equal
/// signatures give the same picture of this layer (given the same pictures
/// beneath it, for adjustment layers and transitions).
///
/// A layer's own values are read at its time (posterized by its `fps`). A
/// layer whose content reads the time itself (a custom shader) includes that
/// time, so its signature changes at every instant; a video layer includes
/// the source frames it shows.
pub(crate) fn layer_signature(ev: &Evaluator, scene: &Scene, layer: &Layer, t: f64) -> Option<Vec<u64>> {
    signature(ev, scene, layer, t, 0)
}

/// [`layer_signature`] of a layer that is `depth` mattes deep.
fn signature(ev: &Evaluator, scene: &Scene, layer: &Layer, t: f64, depth: u32) -> Option<Vec<u64>> {
    if !ev.active(layer, t) {
        return None;
    }
    let mut out = Vec::with_capacity(32);
    let lt = ev.layer_time(layer, t);
    let l = ev.layer(layer, lt);
    out.extend(l.position.iter().chain(&l.scale).chain(&l.rotation).chain(&l.anchor).map(|v| v.to_bits()));
    out.push(l.opacity.to_bits());
    if let Some(tr) = ev.transition(layer, t) {
        out.push(tr.progress.to_bits());
        out.extend(tr.params.values().flatten().map(|v| v.to_bits()));
    }
    if let LayerKind::Video(v) = &layer.kind {
        if let Some(sf) = scene.clip_info(&v.slot).and_then(|clip| ev.source_frame(layer, v, t, clip)) {
            out.extend([sf.frame, sf.next, sf.weight.to_bits()]);
        }
    }
    if let LayerKind::Particles(_) = &layer.kind {
        // Particles move at every instant.
        out.push(lt.to_bits());
    }
    if let LayerKind::Text(text) = &layer.kind {
        // Animated letters change at every instant.
        if text.animators.iter().any(|a| a.enabled) {
            out.push(lt.to_bits());
        }
    }
    if let LayerKind::Comp(c) = &layer.kind {
        // A comp's picture is what its own stack shows at the comp time.
        match scene.comps.get(&c.comp) {
            Some(sub) => {
                let local = ev.comp_time(layer, c, t, sub.project.settings.duration);
                let sig = stack_signature(&Evaluator::new(&sub.project), sub, &sub.project.layers, local);
                out.push(sig.len() as u64);
                out.extend(sig);
            }
            None => out.push(u64::MAX),
        }
    }
    for e in layer.effects.iter().filter(|e| e.enabled) {
        if effect_reads_time(&e.kind) {
            out.push(lt.to_bits());
        }
        for values in ev.effect(e, lt).params.values() {
            out.push(values.len() as u64);
            out.extend(values.iter().map(|v| v.to_bits()));
        }
    }
    for m in &layer.masks {
        out.extend(ev.mask(m, lt).bits());
    }
    // A matte changes with its source; a source that is not there (or a
    // cycle, which `matte` refuses to draw) counts as a constant.
    if let Some(m) = &layer.matte {
        let source = scene.project.layers.iter().find(|l| l.id == m.layer).filter(|_| depth < MAX_MATTE_DEPTH);
        match source.and_then(|s| signature(ev, scene, s, t, depth + 1)) {
            Some(sig) => {
                out.push(sig.len() as u64);
                out.extend(sig);
            }
            None => out.push(u64::MAX),
        }
    }
    Some(out)
}

/// Effects whose picture changes with the time even when their parameters
/// do not.
fn effect_reads_time(kind: &str) -> bool {
    edits_core::effects::spec(kind).is_some_and(|s| s.reads_time)
}

/// Everything the composite of `layers` at `t` depends on: the camera and
/// the signature of every visible layer. Two instants with equal signatures
/// render identically.
pub(crate) fn stack_signature(ev: &Evaluator, scene: &Scene, layers: &[Layer], t: f64) -> Vec<u64> {
    let mut out = Vec::with_capacity(64);
    let cam = ev.camera(t);
    out.push(cam.fov.to_bits());
    out.extend(cam.position.iter().chain(&cam.rotation).map(|v| v.to_bits()));
    for (i, layer) in layers.iter().enumerate().filter(|(_, l)| l.visible) {
        if let Some(sig) = layer_signature(ev, scene, layer, t) {
            out.push(u64::MAX - i as u64);
            out.extend(sig);
        }
    }
    out
}
