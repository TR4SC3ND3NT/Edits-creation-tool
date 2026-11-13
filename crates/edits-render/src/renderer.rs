//! The renderer: device resources, the pass machinery and the per-frame loop.
//!
//! Working textures are RGBA16Float, linear light, premultiplied alpha. A
//! frame is a pure function of (project, frame index): nothing is carried
//! from one frame to the next except caches.

use crate::assets;
use crate::compose::{self, Input, Output, Recipe};
use crate::fx::ColorOp;
use crate::geometry::{self, FitUv};
use crate::gpu::{AdapterInfo, AdapterPolicy, Gpu};
use anyhow::{anyhow, bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use crate::stack::stack_signature;
use crate::video::{self, Clip, VideoInfo, VideoStats, Videos};
use edits_core::{Blend, ClipInfo, Evaluator, FrameBlend, Layer, LayerKind, Project, Slot, SourceFrame};
use image::RgbaImage;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::{Arc, OnceLock};

pub(crate) const WORK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const UNIFORM_SIZE: u64 = 512;
const UNIFORM_SLOTS: u64 = 2048;
const PLACEHOLDER_SIZE: u32 = 1024;
/// Largest frame: each working texture takes 8 bytes per pixel.
const MAX_PIXELS: u64 = 40_000_000;

/// How to create a [`Renderer`].
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// `None` reads `EDITS_ADAPTER` (`auto`, `hardware`, `software`), defaulting to `auto`.
    pub adapter: Option<AdapterPolicy>,
    /// Samples per pixel for the edges of cubes: 1 (off) or 4. Flat layers
    /// do not need it: their edges are faded by pixel coverage in the shader.
    pub msaa: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { adapter: None, msaa: 4 }
    }
}

/// The uniform block every pass receives (see `shaders/common.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Uni {
    pub(crate) mvp: [f32; 16],
    res: [f32; 4],
    time: [f32; 4],
    p: [[f32; 4]; 26],
}

/// A texture with a stable id, used to cache bind groups.
#[derive(Clone)]
pub(crate) struct Tex {
    pub id: u64,
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub w: u32,
    pub h: u32,
    kind: TexKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum TexKind {
    Work,
    WorkMs,
    Depth,
    Image,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Samp {
    /// Bilinear, clamped: working textures.
    Linear,
    /// Trilinear, clamped: pictures with mip levels.
    Image,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Pipe {
    Copy,
    /// Two weighted textures, written or added to the target.
    Sum,
    SumAdd,
    Mip,
    Blur,
    Streak,
    Bright,
    RgbSplit,
    Transform2d,
    /// Any other fragment entry point of `passes.wgsl` (the multi-pass
    /// effects in `fx.rs`), compiled the first time it is used.
    Pass(&'static str),
}

struct Pipes {
    /// Flat layers: one sample per pixel, edges faded by coverage in the shader.
    geo: wgpu::RenderPipeline,
    /// Cube faces: depth tested, back faces culled, multisampled.
    geo_depth: wgpu::RenderPipeline,
    /// Generated passes (see `compose.rs`), compiled on first use.
    composed: HashMap<Recipe, wgpu::RenderPipeline>,
    full: HashMap<Pipe, wgpu::RenderPipeline>,
    /// `passes.wgsl`, for the `Pipe::Pass` pipelines.
    module: wgpu::ShaderModule,
}

#[derive(Clone)]
pub(crate) struct Picture {
    pub(crate) tex: Tex,
    pub(crate) aspect: f64,
}

/// Pictures of video frames kept on the GPU (motion blur samples and
/// neighbouring output frames often show the same source frame).
const VIDEO_TEXTURES: usize = 6;

/// A video frame on the GPU: clip key, the two source frames, the weight of the second and whether it is a flow in-between.
type VideoTexKey = (String, u64, u64, u64, bool);

/// One textured quad of a layer.
pub(crate) struct Quad {
    mvp: [f32; 16],
    tex: Tex,
    fit: FitUv,
    color: [f32; 4],
    /// The quad covers every pixel of the frame.
    pub covers: bool,
    /// Widening (fractions of the quad's size) that lets its edges be faded
    /// by pixel coverage; zero for cube faces and for quads that cover the frame.
    margin: [f32; 2],
}

impl Quad {
    /// A frame-sized texture shown pixel for pixel: the unit quad scaled to
    /// clip space, y up.
    pub(crate) fn frame(tex: Tex) -> Quad {
        let mvp = glam::DMat4::from_scale(glam::DVec3::new(2.0, -2.0, 1.0));
        let fit = geometry::fit_uv(edits_core::Fit::Stretch, [0.5, 0.5], 1.0, 1.0);
        Quad { mvp: mat(mvp), tex, fit, color: [1.0, 1.0, 1.0, 1.0], covers: true, margin: [0.0, 0.0] }
    }

    /// True for [`Quad::frame`] of a texture of `target`'s size: reading it
    /// is then a plain copy, done as one (no resampling through the quad).
    fn is_frame_of(&self, target: &Tex) -> bool {
        let frame = mat(glam::DMat4::from_scale(glam::DVec3::new(2.0, -2.0, 1.0)));
        (self.tex.w, self.tex.h) == (target.w, target.h)
            && self.mvp == frame
            && self.color == [1.0; 4]
            && (self.fit.offset, self.fit.scale, self.fit.cut) == ([0.0, 0.0], [1.0, 1.0], false)
    }
}

pub(crate) struct Scene {
    pub project: Project,
    slots: HashMap<String, Picture>,
    /// Slots that video layers play (those with a file), by slot name.
    pub clips: HashMap<String, Arc<Clip>>,
    /// Effect `source` -> key into `Renderer::custom`.
    pub custom: HashMap<String, String>,
    /// Fonts of text layers by `font` as written (`""` for the built-in one).
    pub fonts: HashMap<String, Arc<crate::text::Font>>,
    /// Lookup tables of built-in effects (see `fxlib`).
    pub loaded: crate::fxlib::Loaded,
    /// The comps this scene's layers draw, each seen as a scene of its own
    /// (see `comp.rs`). A comp that would contain itself is left out.
    pub comps: HashMap<String, Arc<Scene>>,
}

impl Scene {
    /// This scene's pictures, clips, shaders, fonts and tables for another
    /// project (a comp of this one), without comps.
    pub(crate) fn derive(&self, project: Project) -> Scene {
        Scene {
            project,
            slots: self.slots.clone(),
            clips: self.clips.clone(),
            custom: self.custom.clone(),
            fonts: self.fonts.clone(),
            loaded: self.loaded.clone(),
            comps: HashMap::new(),
        }
    }

    /// The picture of a slot the project uses (a layer's or an effect's map).
    pub(crate) fn slot_texture(&self, name: &str) -> Option<Tex> {
        self.slots.get(name).map(|p| p.tex.clone())
    }
}

impl Scene {
    /// The clip a video layer's slot plays, as the evaluator needs it.
    pub fn clip_info(&self, slot: &str) -> Option<ClipInfo> {
        self.clips.get(slot).map(|c| c.info())
    }
}

/// Values shared by every pass of one motion blur sample.
#[derive(Clone, Copy, Default)]
pub(crate) struct Ctx {
    pub w: u32,
    pub h: u32,
    /// Seconds, beats, frame index, aspect.
    pub time: [f32; 4],
}

/// What a generated pass reads.
pub(crate) enum Source<'a> {
    Tex(&'a Tex),
    /// A picture, its blurred bright pass, and the glow's colour times its intensity.
    Glow(&'a Tex, &'a Tex, [f32; 3]),
    Quad(&'a Quad),
}

/// How a generated pass writes.
pub(crate) enum Sink<'a> {
    Plain,
    /// Blended over `dst`, or over the flat colour `flat` when nothing has been drawn yet.
    Over { dst: Option<&'a Tex>, flat: [f32; 4], mode: Blend, opacity: f32 },
}

struct Readback {
    w: u32,
    h: u32,
    buffer: wgpu::Buffer,
    row_bytes: u32,
}

/// How long one render pass took, measured with [`Renderer::set_profiling`].
#[derive(Clone, Debug)]
pub struct PassTiming {
    /// What the pass does, e.g. `Blur`, `quads` or `Quad+2ops+blend0`.
    pub label: String,
    pub ms: f64,
}

/// Counters from the most recent frame, for profiling.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    /// Render passes recorded.
    pub passes: u32,
    /// Motion blur samples asked for.
    pub samples: u32,
    /// Samples actually rendered: instants at which nothing differs share one.
    pub rendered_samples: u32,
    pub width: u32,
    pub height: u32,
}

pub struct Renderer {
    pub(crate) gpu: Gpu,
    msaa: u32,
    tex_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// Effects (built-in and custom): the picture, an auxiliary picture, the
    /// two samplers and a 3D lookup table (see `shaders/fx/prelude.wgsl`).
    fx_layout: wgpu::BindGroupLayout,
    fx_pipeline_layout: wgpu::PipelineLayout,
    fx_binds: HashMap<(u64, u64, u64), wgpu::BindGroup>,
    /// Built-in effect pipelines by kind and chosen options, compiled on first use.
    fx_pipes: HashMap<(&'static str, Vec<u8>), wgpu::RenderPipeline>,
    /// Bound when an effect has no lookup table of its own.
    identity_lut: Lut,
    pipes: Pipes,
    ubuf: wgpu::Buffer,
    ubind: wgpu::BindGroup,
    staging: Vec<u8>,
    pub(crate) encoder: Option<wgpu::CommandEncoder>,
    linear: wgpu::Sampler,
    mirror: wgpu::Sampler,
    image_sampler: wgpu::Sampler,
    pub(crate) white: Tex,
    next_id: u64,
    pool: HashMap<(u32, u32, TexKind), Vec<Tex>>,
    pool_size: (u32, u32),
    binds: HashMap<(u64, u64, Samp), wgpu::BindGroup>,
    /// Pictures by file (path, size, modification time) or placeholder name.
    pictures: HashMap<String, Picture>,
    /// Compiled custom effects by file (path, size, modification time).
    pub(crate) custom: HashMap<String, wgpu::RenderPipeline>,
    /// Transition pipelines by kind, compiled on first use.
    transition_pipes: HashMap<&'static str, wgpu::RenderPipeline>,
    /// Video decoders and decoded frames.
    videos: Videos,
    /// What ffprobe said about each video file (path, size, modification time).
    probes: HashMap<String, VideoInfo>,
    /// Video frames uploaded lately, most recent last.
    video_textures: Vec<(VideoTexKey, Picture)>,
    /// Fonts, layouts, glyph masks and atlases of text layers.
    pub(crate) text: crate::text::TextCache,
    pub(crate) scene: Option<Arc<Scene>>,
    readback: Option<Readback>,
    samples_override: Option<u32>,
    pub(crate) ctx: Ctx,
    /// The output frame being rendered: grain and a custom shader's
    /// `fx.frame` read it at every instant of the frame.
    pub(crate) frame: u64,
    stats: FrameStats,
    /// Name of the pass being recorded, for profiling.
    label: String,
    profile: Option<Vec<PassTiming>>,
    /// The pipeline and buffers of particles layers (see `particles.rs`).
    pub(crate) particles: crate::particles::ParticleGpu,
    /// How many track mattes deep the layer being drawn is (a guard against cycles).
    pub(crate) matte_depth: u32,
    /// How many time effects deep the picture being drawn is (see `time.rs`).
    pub(crate) time_depth: u32,
    /// Pictures of comps rendered during this output frame (see `comp.rs`).
    pub(crate) comp_cache: crate::comp::CompCache,
    /// Pictures of shape layers (see `shape/mod.rs`).
    pub(crate) shapes: crate::shape::ShapeCache,
}

fn file_key(path: &Path) -> String {
    let meta = std::fs::metadata(path).ok();
    let stamp = meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}|{}|{stamp}", path.display(), meta.map(|m| m.len()).unwrap_or(0))
}

fn mat(m: glam::DMat4) -> [f32; 16] {
    m.as_mat4().to_cols_array()
}

impl Renderer {
    /// Open a device. Fails with a readable message when no adapter matches
    /// the policy; callers that can do without rendering should treat that as
    /// "not available" rather than a crash.
    pub fn new(options: RenderOptions) -> Result<Self> {
        let policy = match options.adapter {
            Some(p) => p,
            None => crate::gpu::policy_from_env()?,
        };
        let gpu = Gpu::new(policy)?;
        let msaa = if options.msaa >= 4 { 4 } else { 1 };
        let device = &gpu.device;

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edits uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(UNIFORM_SIZE),
                },
                count: None,
            }],
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edits textures"),
            entries: &[texture_entry(0), texture_entry(1), sampler_entry(2), sampler_entry(3)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edits"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&tex_layout)],
            immediate_size: 0,
        });
        let fx_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edits effect textures"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                sampler_entry(2),
                sampler_entry(3),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let fx_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edits effects"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&fx_layout)],
            immediate_size: 0,
        });
        let identity_lut = make_lut(device, &gpu.queue, 2, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0]], 1);

        let source = concat!(include_str!("shaders/common.wgsl"), include_str!("shaders/passes.wgsl"), include_str!("shaders/mask.wgsl"), include_str!("shaders/time.wgsl"));
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edits passes"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let mut full = HashMap::new();
        for (pipe, entry, format, blend) in [
            (Pipe::Copy, "fs_copy", WORK_FORMAT, None),
            (Pipe::Sum, "fs_copy2", WORK_FORMAT, None),
            (Pipe::SumAdd, "fs_copy2", WORK_FORMAT, Some(additive)),
            (Pipe::Mip, "fs_copy", IMAGE_FORMAT, None),
            (Pipe::Blur, "fs_blur", WORK_FORMAT, None),
            (Pipe::Streak, "fs_streak", WORK_FORMAT, None),
            (Pipe::Bright, "fs_bright", WORK_FORMAT, None),
            (Pipe::RgbSplit, "fs_rgb_split", WORK_FORMAT, None),
            (Pipe::Transform2d, "fs_transform2d", WORK_FORMAT, None),
        ] {
            full.insert(pipe, make_pipeline(device, &pipeline_layout, &module, "vs_full", entry, format, blend, 1, false));
        }
        let pipes = Pipes {
            geo: make_pipeline(device, &pipeline_layout, &module, "vs_geo", "fs_geo", WORK_FORMAT, None, 1, false),
            geo_depth: make_pipeline(device, &pipeline_layout, &module, "vs_geo", "fs_geo", WORK_FORMAT, None, msaa, true),
            composed: HashMap::new(),
            full,
            module,
        };

        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edits uniforms"),
            size: UNIFORM_SIZE * UNIFORM_SLOTS,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ubind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edits uniforms"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &ubuf, offset: 0, size: NonZeroU64::new(UNIFORM_SIZE) }),
            }],
        });
        let sampler = |address, mips: bool, aniso: u16| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: address,
                address_mode_v: address,
                address_mode_w: address,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: if mips { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest },
                anisotropy_clamp: aniso,
                ..Default::default()
            })
        };
        let linear = sampler(wgpu::AddressMode::ClampToEdge, false, 1);
        let mirror = sampler(wgpu::AddressMode::MirrorRepeat, false, 1);
        // Anisotropic filtering keeps tilted cube faces sharp; it is too slow to be worth it on a CPU rasteriser.
        let image_sampler = sampler(wgpu::AddressMode::ClampToEdge, true, if gpu.info.software { 1 } else { 8 });

        let mut r = Self {
            white: dummy(device),
            gpu,
            msaa,
            tex_layout,
            pipeline_layout,
            fx_layout,
            fx_pipeline_layout,
            fx_binds: HashMap::new(),
            fx_pipes: HashMap::new(),
            identity_lut,
            pipes,
            ubuf,
            ubind,
            staging: Vec::new(),
            encoder: None,
            linear,
            mirror,
            image_sampler,
            next_id: 1,
            pool: HashMap::new(),
            pool_size: (0, 0),
            binds: HashMap::new(),
            pictures: HashMap::new(),
            custom: HashMap::new(),
            transition_pipes: HashMap::new(),
            videos: Videos::default(),
            probes: HashMap::new(),
            video_textures: Vec::new(),
            text: Default::default(),
            scene: None,
            readback: None,
            samples_override: None,
            ctx: Ctx::default(),
            frame: 0,
            stats: FrameStats::default(),
            label: String::new(),
            profile: None,
            particles: Default::default(),
            matte_depth: 0,
            time_depth: 0,
            comp_cache: Default::default(),
            shapes: Default::default(),
        };
        r.white = r.upload_picture(&RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255])));
        r.flush();
        Ok(r)
    }

    /// The adapter in use.
    pub fn info(&self) -> &AdapterInfo {
        &self.gpu.info
    }

    /// Counters from the most recent frame.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// Time every render pass on its own. This waits for the GPU after each
    /// pass, so frames take longer than usual while it is on.
    pub fn set_profiling(&mut self, on: bool) {
        self.profile = on.then(Vec::new);
    }

    /// The pass timings collected since the last call.
    pub fn take_profile(&mut self) -> Vec<PassTiming> {
        self.profile.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Override `settings.motion_blur.samples` (`None` restores the project's value).
    pub fn set_motion_blur_samples(&mut self, samples: Option<u32>) {
        self.samples_override = samples.map(|s| s.clamp(1, 64));
    }

    /// The project given to [`Renderer::prepare`].
    pub fn project(&self) -> Option<&Project> {
        self.scene.as_deref().map(|s| &s.project)
    }

    /// Output size at `scale`, rounded to even numbers (what video encoders need).
    pub fn output_size(&self, scale: f32) -> Result<(u32, u32)> {
        let scene = self.scene.as_ref().ok_or_else(|| anyhow!("no project prepared"))?;
        Ok(scaled_size(&scene.project, scale))
    }

    // ---- preparing a project -------------------------------------------------

    /// Take a project: load the pictures of the slots it uses (empty slots get
    /// a placeholder) and compile its custom effects. `base_dir` is the folder
    /// relative paths resolve against, normally the project file's folder.
    pub fn prepare(&mut self, project: &Project, base_dir: &Path) -> Result<()> {
        let s = &project.settings;
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        if s.width == 0 || s.height == 0 || s.width > max || s.height > max {
            bail!("project size {}x{} is not renderable (this adapter supports 1..{max} pixels per side)", s.width, s.height);
        }
        if s.fps.is_nan() || s.fps <= 0.0 {
            bail!("settings.fps must be positive");
        }
        let mut used: Vec<&str> = Vec::new();
        let mut clips: HashMap<String, Arc<Clip>> = HashMap::new();
        let mut probed: Vec<String> = Vec::new();
        let mut texts: Vec<(&str, Option<&str>)> = Vec::new();
        for l in project.all_layers() {
            match &l.kind {
                LayerKind::Image { slot } => used.push(slot),
                LayerKind::Cube { faces, .. } => used.extend(faces.iter().map(String::as_str)),
                LayerKind::Solid { .. } | LayerKind::Adjustment | LayerKind::Comp(_) | LayerKind::Shape(_) => {}
                LayerKind::Text(t) => texts.push((&l.id, t.font.as_deref())),
                LayerKind::Particles(ps) => used.extend(ps.sprite_slot()),
                LayerKind::Video(v) => {
                    let Some(slot) = project.slots.get(&v.slot).filter(|s| s.path.is_some()) else {
                        // An empty slot shows its placeholder, like a picture.
                        used.push(&v.slot);
                        continue;
                    };
                    if clips.contains_key(&v.slot) {
                        continue;
                    }
                    let full = base_dir.join(slot.path.as_deref().expect("filtered above"));
                    let key = file_key(&full);
                    let info = match self.probes.get(&key) {
                        Some(info) => info.clone(),
                        None => {
                            if !full.is_file() {
                                bail!("slot \"{}\": video not found: {} (paths are relative to the project file)", v.slot, full.display());
                            }
                            let info = video::probe(&full).with_context(|| format!("slot \"{}\"", v.slot))?;
                            self.probes.insert(key.clone(), info.clone());
                            info
                        }
                    };
                    probed.push(key.clone());
                    let trim = slot.trim_frames(info.fps, info.frames);
                    let side = s.width.max(s.height).min(max);
                    clips.insert(v.slot.clone(), Arc::new(Clip::new(full, &key, info, trim, side)));
                }
            }
            used.extend(crate::fxlib::slots_used(l));
        }
        let mut slots = HashMap::new();
        let mut kept: Vec<String> = Vec::new();
        for name in used {
            if slots.contains_key(name) {
                continue;
            }
            let path = project.slots.get(name).and_then(|s: &Slot| s.path.as_deref());
            let picture = match path {
                Some(p) => {
                    let full = base_dir.join(p);
                    let key = file_key(&full);
                    kept.push(key.clone());
                    match self.pictures.get(&key) {
                        Some(pic) => pic.clone(),
                        None => {
                            let mut img = assets::load(&full, max).with_context(|| format!("slot \"{name}\""))?;
                            assets::premultiply(&mut img);
                            let pic = Picture { aspect: img.width() as f64 / img.height() as f64, tex: self.upload_picture(&img) };
                            self.pictures.insert(key, pic.clone());
                            pic
                        }
                    }
                }
                None => {
                    let key = format!("placeholder|{name}");
                    kept.push(key.clone());
                    match self.pictures.get(&key) {
                        Some(pic) => pic.clone(),
                        None => {
                            let pic = Picture { aspect: 1.0, tex: self.upload_picture(&assets::placeholder(name, PLACEHOLDER_SIZE)) };
                            self.pictures.insert(key, pic.clone());
                            pic
                        }
                    }
                }
            };
            slots.insert(name.to_string(), picture);
        }
        self.flush();
        let fonts = self.load_fonts(&texts, base_dir, file_key)?;

        let mut custom = HashMap::new();
        for l in project.all_layers() {
            for (i, e) in l.effects.iter().enumerate() {
                if e.kind != "custom" {
                    continue;
                }
                let place = format!("layer \"{}\", effect {i}", l.id);
                let src = e.source.as_deref().ok_or_else(|| anyhow!("{place}: a custom effect needs \"source\" (a WGSL file)"))?;
                if custom.contains_key(src) {
                    continue;
                }
                let full = base_dir.join(src);
                let text = std::fs::read_to_string(&full).map_err(|e| anyhow!("{place}: cannot read shader {}: {e}", full.display()))?;
                let key = file_key(&full);
                if !self.custom.contains_key(&key) {
                    let pipeline = self.compile_custom(src, &text).with_context(|| place.clone())?;
                    self.custom.insert(key.clone(), pipeline);
                }
                custom.insert(src.to_string(), key);
            }
        }
        // Keep what this project uses; pictures and shaders of earlier projects go.
        self.pictures.retain(|key, _| kept.contains(key));
        self.custom.retain(|key, _| custom.values().any(|k| k == key));
        self.probes.retain(|key, _| probed.contains(key));
        let clip_keys: Vec<&str> = clips.values().map(|c| c.key.as_str()).collect();
        self.videos.retain(&clip_keys);
        self.video_textures.retain(|((key, ..), _)| clip_keys.contains(&key.as_str()));
        self.binds.clear();
        self.fx_binds.clear();
        let loaded = self.load_fx_inputs(project, base_dir)?;
        let mut scene = Scene { project: project.clone(), slots, clips, custom, loaded, fonts, comps: HashMap::new() };
        scene.comps = crate::comp::comp_scenes(&scene, max)?;
        self.drop_comp_pictures();
        self.scene = Some(Arc::new(scene));
        Ok(())
    }

    /// Counters of the video decoders since the renderer was made.
    pub fn video_stats(&self) -> VideoStats {
        self.videos.stats
    }

    /// The picture of a video slot's source frames `at` (blended by `blend`
    /// when `at.weight` is not 0), uploaded with mip levels. The last few are
    /// kept on the GPU.
    pub(crate) fn video_picture(&mut self, clip: &Clip, at: SourceFrame, blend: FrameBlend) -> Result<Picture> {
        // Keyed by source frames: two trims of one file share their frames.
        let key: VideoTexKey = (clip.key.clone(), clip.first + at.frame, clip.first + at.next, at.weight.to_bits(), blend == FrameBlend::Flow);
        if let Some(i) = self.video_textures.iter().position(|(k, _)| *k == key) {
            let entry = self.video_textures.remove(i);
            let pic = entry.1.clone();
            self.video_textures.push(entry);
            return Ok(pic);
        }
        let img = self.videos.picture(clip, at, blend)?;
        let pic = Picture { aspect: img.width() as f64 / img.height() as f64, tex: self.upload_picture(&img) };
        self.video_textures.push((key, pic.clone()));
        if self.video_textures.len() > VIDEO_TEXTURES {
            let (_, old) = self.video_textures.remove(0);
            let id = old.tex.id;
            self.binds.retain(|k, _| k.0 != id && k.1 != id);
        }
        Ok(pic)
    }

    fn compile_custom(&mut self, name: &str, text: &str) -> Result<wgpu::RenderPipeline> {
        let device = &self.gpu.device;
        let source = format!("{text}\n{}", crate::shader::CUSTOM_CONTRACT);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(name), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline = make_pipeline(device, &self.fx_pipeline_layout, &module, "edits_vs", "edits_fs", WORK_FORMAT, None, 1, false);
        if let Some(err) = pollster::block_on(scope.pop()) {
            let message = err.to_string();
            let message = message.trim();
            bail!(
                "custom effect shader \"{name}\" does not compile:\n{message}\n\
                 (the file must define `fn effect(uv: vec2<f32>) -> vec4<f32>`; see docs/EFFECTS.md)"
            );
        }
        Ok(pipeline)
    }

    /// The pipeline of a built-in effect with the options `key` (see
    /// `fxlib::option_key`), compiled the first time it is used.
    pub(crate) fn builtin_pipeline(&mut self, b: &'static crate::fxlib::Builtin, key: &[u8]) -> Result<wgpu::RenderPipeline> {
        if let Some(p) = self.fx_pipes.get(&(b.kind, key.to_vec())) {
            return Ok(p.clone());
        }
        let spec = edits_core::effects::spec(b.kind).ok_or_else(|| anyhow!("effect \"{}\" is not in the registry", b.kind))?;
        let device = &self.gpu.device;
        let source = crate::fxlib::source(b, spec, key);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(b.kind), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline = make_pipeline(device, &self.fx_pipeline_layout, &module, "edits_vs", "edits_fs", WORK_FORMAT, None, 1, false);
        if let Some(err) = pollster::block_on(scope.pop()) {
            bail!("effect \"{}\" does not compile on this adapter: {}", b.kind, err.to_string().trim());
        }
        self.fx_pipes.insert((b.kind, key.to_vec()), pipeline.clone());
        Ok(pipeline)
    }

    /// Upload a 3D lookup table of `size`^3 linear RGB entries, red varying
    /// fastest, then green, then blue (the order of a `.cube` file).
    pub(crate) fn upload_lut(&mut self, size: u32, rgb: &[[f32; 3]]) -> Result<Lut> {
        if !(2..=256).contains(&size) || rgb.len() != (size * size * size) as usize {
            bail!("a 3D lookup table needs size^3 entries with a size of 2 to 256 (got {} entries for size {size})", rgb.len());
        }
        let id = self.new_id();
        Ok(make_lut(&self.gpu.device, &self.gpu.queue, size, rgb, id))
    }

    /// One effect pass: `pipeline` (a built-in or custom effect) reads `src`
    /// (and `aux`, and `lut`) and writes `target`, with `p` as `fx.p0` .. `fx.p15`.
    pub(crate) fn fx_pass(&mut self, pipeline: &wgpu::RenderPipeline, target: &Tex, src: &Tex, aux: Option<&Tex>, lut: Option<&Lut>, p: &[f32; 16]) {
        let aux = aux.unwrap_or(&self.white).clone();
        let lut = lut.unwrap_or(&self.identity_lut).clone();
        let key = (src.id, aux.id, lut.id);
        let bind = match self.fx_binds.get(&key) {
            Some(g) => g.clone(),
            None => {
                let g = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.fx_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src.view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&aux.view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.linear) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.mirror) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&lut.view) },
                    ],
                });
                self.fx_binds.insert(key, g.clone());
                g
            }
        };
        let packed = [[p[0], p[1], p[2], p[3]], [p[4], p[5], p[6], p[7]], [p[8], p[9], p[10], p[11]], [p[12], p[13], p[14], p[15]]];
        let uni = self.uniform(target, &packed);
        let offset = self.push_uniform(&uni);
        if self.label.is_empty() {
            self.name(|| "effect".into());
        }
        self.raw_pass(pipeline, &target.view, None, None, Some(wgpu::Color::TRANSPARENT), &[(bind, offset, 3)]);
    }

    /// The pipeline of a transition, compiled the first time it is used.
    fn transition_pipeline(&mut self, kind: &str) -> Result<wgpu::RenderPipeline> {
        let def = crate::transitions::def(kind).ok_or_else(|| anyhow!("unknown transition \"{kind}\" (see `edits transitions`)"))?;
        if let Some(p) = self.transition_pipes.get(def.kind) {
            return Ok(p.clone());
        }
        let device = &self.gpu.device;
        let source = crate::transitions::source(def);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(def.kind), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline = make_pipeline(device, &self.pipeline_layout, &module, "edits_tr_vs", "edits_tr_fs", WORK_FORMAT, None, 1, false);
        if let Some(err) = pollster::block_on(scope.pop()) {
            bail!("transition \"{kind}\" does not compile on this adapter: {}", err.to_string().trim());
        }
        self.transition_pipes.insert(def.kind, pipeline.clone());
        Ok(pipeline)
    }

    /// One transition pass: `from` and `to` into `target`.
    pub(crate) fn transition_pass(&mut self, target: &Tex, from: &Tex, to: &Tex, tr: &edits_core::ResolvedTransition) -> Result<()> {
        let pipeline = self.transition_pipeline(tr.kind)?;
        let def = crate::transitions::def(tr.kind).expect("checked by transition_pipeline");
        let mut uni = Uni::zeroed();
        crate::transitions::uniforms(def, tr, target.w as f32 / target.h as f32, bytemuck::bytes_of_mut(&mut uni));
        let bind = self.bind(from, Some(to), Samp::Linear);
        let offset = self.push_uniform(&uni);
        self.name(|| format!("transition {}", tr.kind));
        self.raw_pass(&pipeline, &target.view, None, None, Some(wgpu::Color::TRANSPARENT), &[(bind, offset, 3)]);
        Ok(())
    }

    // ---- textures ------------------------------------------------------------

    pub(crate) fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Upload a premultiplied sRGB picture with a full mip chain.
    fn upload_picture(&mut self, img: &RgbaImage) -> Tex {
        let (w, h) = img.dimensions();
        let mips = 32 - w.max(h).leading_zeros();
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edits picture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            img.as_raw(),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let level = |i: u32| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: i, mip_level_count: Some(1), ..Default::default() });
        for i in 1..mips {
            let (src, dst) = (level(i - 1), level(i));
            let bind = self.make_bind(&src, &self.white.view.clone(), Samp::Linear);
            let mut uni = Uni::zeroed();
            uni.p[0][0] = 1.0;
            let offset = self.push_uniform(&uni);
            let pipeline = self.pipes.full[&Pipe::Mip].clone();
            self.name(|| "Mip".into());
            self.raw_pass(&pipeline, &dst, None, None, Some(wgpu::Color::TRANSPARENT), &[(bind, offset, 3)]);
        }
        let id = self.new_id();
        Tex { id, view: texture.create_view(&wgpu::TextureViewDescriptor::default()), texture, w, h, kind: TexKind::Image }
    }

    /// Upload a glyph atlas (two coverage bytes per pixel, no mip levels).
    pub(crate) fn upload_mask(&mut self, w: u32, h: u32, data: &[u8]) -> Tex {
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edits glyphs"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::text::ATLAS_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(2 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let id = self.new_id();
        Tex { id, view: texture.create_view(&wgpu::TextureViewDescriptor::default()), texture, w, h, kind: TexKind::Image }
    }

    /// Upload a frame-sized picture made on the CPU (RGBA16Float,
    /// premultiplied linear, like a working texture; no mip levels).
    pub(crate) fn upload_work(&mut self, w: u32, h: u32, data: &[u8]) -> Tex {
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edits shapes"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORK_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            data,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * w), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let id = self.new_id();
        Tex { id, view: texture.create_view(&wgpu::TextureViewDescriptor::default()), texture, w, h, kind: TexKind::Image }
    }

    /// Forget the bind groups of a texture that is no longer used.
    pub(crate) fn forget_texture(&mut self, id: u64) {
        self.binds.retain(|k, _| k.0 != id && k.1 != id);
    }

    /// The largest side of a texture on this adapter.
    pub(crate) fn max_texture_side(&self) -> u32 {
        self.gpu.device.limits().max_texture_dimension_2d
    }

    /// A pipeline drawing layer quads (`vs_geo`) with the fragment `entry` of
    /// `source`, blended premultiplied "over" what the target holds.
    pub(crate) fn make_blended_pipeline(&self, label: &str, source: &str, entry: &str) -> wgpu::RenderPipeline {
        let device = &self.gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let over = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
        make_pipeline(device, &self.pipeline_layout, &module, "vs_geo", entry, WORK_FORMAT, Some(wgpu::BlendState { color: over, alpha: over }), 1, false)
    }

    /// A working texture from the pool. Give it back with [`Self::release`].
    pub(crate) fn acquire(&mut self, w: u32, h: u32) -> Tex {
        self.acquire_kind(w, h, TexKind::Work)
    }

    fn acquire_kind(&mut self, w: u32, h: u32, kind: TexKind) -> Tex {
        if let Some(t) = self.pool.get_mut(&(w, h, kind)).and_then(Vec::pop) {
            return t;
        }
        let (format, samples, usage) = match kind {
            TexKind::Work => (WORK_FORMAT, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC),
            TexKind::WorkMs => (WORK_FORMAT, self.msaa, wgpu::TextureUsages::RENDER_ATTACHMENT),
            TexKind::Depth => (DEPTH_FORMAT, self.msaa, wgpu::TextureUsages::RENDER_ATTACHMENT),
            TexKind::Image => unreachable!("pictures are not pooled"),
        };
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edits work"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let id = self.new_id();
        Tex { id, view: texture.create_view(&wgpu::TextureViewDescriptor::default()), texture, w, h, kind }
    }

    pub(crate) fn release(&mut self, t: Tex) {
        if t.kind != TexKind::Image {
            self.pool.entry((t.w, t.h, t.kind)).or_default().push(t);
        }
    }

    fn make_bind(&self, a: &wgpu::TextureView, b: &wgpu::TextureView, samp: Samp) -> wgpu::BindGroup {
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(a) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(b) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(match samp {
                        Samp::Linear => &self.linear,
                        Samp::Image => &self.image_sampler,
                    }),
                },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.mirror) },
            ],
        })
    }

    pub(crate) fn bind(&mut self, a: &Tex, b: Option<&Tex>, samp: Samp) -> wgpu::BindGroup {
        let b = b.unwrap_or(&self.white).clone();
        let key = (a.id, b.id, samp);
        if let Some(g) = self.binds.get(&key) {
            return g.clone();
        }
        let g = self.make_bind(&a.view, &b.view, samp);
        self.binds.insert(key, g.clone());
        g
    }

    // ---- passes --------------------------------------------------------------

    /// Make room for `n` uniform blocks in the current submission.
    pub(crate) fn reserve_uniforms(&mut self, n: u64) {
        if self.staging.len() as u64 + n * UNIFORM_SIZE > UNIFORM_SIZE * UNIFORM_SLOTS {
            self.flush();
        }
    }

    pub(crate) fn push_uniform(&mut self, uni: &Uni) -> u32 {
        self.reserve_uniforms(1);
        let offset = self.staging.len() as u32;
        self.staging.extend_from_slice(bytemuck::bytes_of(uni));
        offset
    }

    /// Send everything recorded so far to the GPU.
    pub(crate) fn flush(&mut self) {
        if !self.staging.is_empty() {
            self.gpu.queue.write_buffer(&self.ubuf, 0, &self.staging);
            self.staging.clear();
        }
        if let Some(encoder) = self.encoder.take() {
            self.gpu.queue.submit([encoder.finish()]);
        }
    }

    /// One render pass: `draws` are (textures, uniform offset, vertex count).
    pub(crate) fn raw_pass(
        &mut self,
        pipeline: &wgpu::RenderPipeline,
        target: &wgpu::TextureView,
        resolve: Option<&wgpu::TextureView>,
        depth: Option<&wgpu::TextureView>,
        clear: Option<wgpu::Color>,
        draws: &[(wgpu::BindGroup, u32, u32)],
    ) {
        self.stats.passes += 1;
        let device = &self.gpu.device;
        let encoder = self.encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("edits frame") }));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations {
                    load: clear.map(wgpu::LoadOp::Clear).unwrap_or(wgpu::LoadOp::Load),
                    store: if resolve.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                },
            })],
            depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        for (bind, offset, vertices) in draws {
            pass.set_bind_group(0, &self.ubind, &[*offset]);
            pass.set_bind_group(1, bind, &[]);
            pass.draw(0..*vertices, 0..1);
        }
        drop(pass);
        if self.profile.is_some() {
            let started = std::time::Instant::now();
            self.flush();
            let _ = self.gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let timing = PassTiming { label: std::mem::take(&mut self.label), ms: started.elapsed().as_secs_f64() * 1000.0 };
            self.profile.as_mut().expect("checked above").push(timing);
        }
    }

    pub(crate) fn name(&mut self, label: impl FnOnce() -> String) {
        if self.profile.is_some() {
            self.label = label();
        }
    }

    pub(crate) fn uniform(&self, target: &Tex, p: &[[f32; 4]]) -> Uni {
        let mut uni = Uni::zeroed();
        uni.res = [target.w as f32, target.h as f32, 1.0 / target.w as f32, 1.0 / target.h as f32];
        uni.time = self.ctx.time;
        uni.p[..p.len()].copy_from_slice(p);
        uni
    }

    /// A full-target pass of a built-in pipeline reading `t0` (and `t1`).
    pub(crate) fn pass(&mut self, pipe: Pipe, target: &Tex, load: bool, t0: &Tex, t1: Option<&Tex>, p: &[[f32; 4]]) {
        let pipeline = match (self.pipes.full.get(&pipe), pipe) {
            (Some(p), _) => p.clone(),
            (None, Pipe::Pass(entry)) => {
                let p = make_pipeline(&self.gpu.device, &self.pipeline_layout, &self.pipes.module, "vs_full", entry, WORK_FORMAT, None, 1, false);
                self.pipes.full.insert(pipe, p.clone());
                p
            }
            (None, _) => unreachable!("{pipe:?} is made with the renderer"),
        };
        self.name(|| format!("{pipe:?}"));
        self.pass_with(&pipeline, target, load, t0, t1, p);
    }

    pub(crate) fn pass_with(&mut self, pipeline: &wgpu::RenderPipeline, target: &Tex, load: bool, t0: &Tex, t1: Option<&Tex>, p: &[[f32; 4]]) {
        let bind = self.bind(t0, t1, Samp::Linear);
        let uni = self.uniform(target, p);
        let offset = self.push_uniform(&uni);
        let clear = if load { None } else { Some(wgpu::Color::TRANSPARENT) };
        if self.label.is_empty() {
            self.name(|| "custom".into());
        }
        self.raw_pass(pipeline, &target.view, None, None, clear, &[(bind, offset, 3)]);
    }

    /// The pipeline of a generated pass, compiled the first time it is needed.
    fn composed(&mut self, recipe: &Recipe) -> wgpu::RenderPipeline {
        if let Some(p) = self.pipes.composed.get(recipe) {
            return p.clone();
        }
        let source = format!("{}{}", include_str!("shaders/common.wgsl"), recipe.fragment());
        let device = &self.gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("edits composed"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline = make_pipeline(device, &self.pipeline_layout, &module, recipe.vertex(), "fs_main", WORK_FORMAT, None, 1, false);
        self.pipes.composed.insert(recipe.clone(), pipeline.clone());
        pipeline
    }

    /// A generated pass into `target`: read `source`, apply the colour
    /// operations `ops` (at most `compose::MAX_OPS`), write through `sink`.
    pub(crate) fn compose(&mut self, target: &Tex, source: Source, ops: &[ColorOp], sink: Sink) {
        debug_assert!(ops.len() <= compose::MAX_OPS);
        let mut uni = self.uniform(target, &[]);
        let (input, t0, mut t1, samp) = match source {
            // A frame-sized picture shown pixel for pixel is copied, not resampled.
            Source::Quad(q) if q.is_frame_of(target) => (Input::Pixels, &q.tex, None, Samp::Linear),
            Source::Tex(t) => (Input::Tex, t, None, Samp::Linear),
            Source::Glow(t, glow, color) => {
                uni.p[1] = [color[0], color[1], color[2], 0.0];
                (Input::Glow, t, Some(glow), Samp::Linear)
            }
            Source::Quad(q) => {
                uni.mvp = q.mvp;
                uni.p[0] = [q.fit.offset[0], q.fit.offset[1], q.fit.scale[0], q.fit.scale[1]];
                uni.p[1] = q.color;
                (Input::Quad { cut: q.fit.cut }, &q.tex, None, Samp::Image)
            }
        };
        let output = match sink {
            Sink::Plain => Output::Plain,
            Sink::Over { dst, flat, mode, opacity } => {
                uni.p[2] = [0.0, 0.0, opacity.min(1.0), 0.0];
                uni.p[3] = flat;
                if dst.is_some() {
                    debug_assert!(t1.is_none(), "a glow cannot be blended in the same pass");
                    t1 = dst;
                }
                Output::Blend { mode: compose::blend_code(mode), flat: dst.is_none() }
            }
        };
        for (i, op) in ops.iter().enumerate() {
            uni.p[compose::OPS_AT + 2 * i] = op[0];
            uni.p[compose::OPS_AT + 2 * i + 1] = op[1];
        }
        let recipe = Recipe { input, ops: ops.iter().map(|op| op[0][0] as u8).collect(), output };
        let pipeline = self.composed(&recipe);
        let bind = self.bind(t0, t1, samp);
        let offset = self.push_uniform(&uni);
        self.name(|| recipe.label());
        self.raw_pass(&pipeline, &target.view, None, None, Some(wgpu::Color::TRANSPARENT), &[(bind, offset, recipe.vertices())]);
    }

    /// Draw a layer's quads into a fresh transparent texture. Flat layers
    /// fade their own edges; a cube (`depth`) is multisampled instead.
    pub(crate) fn draw_quads(&mut self, quads: &[Quad], depth: bool) -> Tex {
        let (w, h) = (self.ctx.w, self.ctx.h);
        let target = self.acquire(w, h);
        self.reserve_uniforms(quads.len() as u64);
        let mut draws = Vec::with_capacity(quads.len());
        for q in quads {
            let cut = if q.fit.cut { 1.0 } else { 0.0 };
            let fade = if q.margin != [0.0, 0.0] { 1.0 } else { 0.0 };
            let mut uni = self.uniform(&target, &[[q.fit.offset[0], q.fit.offset[1], q.fit.scale[0], q.fit.scale[1]], q.color, [cut, fade, 0.0, 0.0]]);
            uni.mvp = q.mvp;
            uni.p[25] = [q.margin[0], q.margin[1], 0.0, 0.0];
            let bind = self.bind(&q.tex, None, Samp::Image);
            draws.push((bind, self.push_uniform(&uni), 6));
        }
        let clear = Some(wgpu::Color::TRANSPARENT);
        self.name(|| if depth { "cube".into() } else { "quads".into() });
        if !depth {
            self.raw_pass(&self.pipes.geo.clone(), &target.view, None, None, clear, &draws);
            return target;
        }
        let pipeline = self.pipes.geo_depth.clone();
        let depth_tex = self.acquire_kind(w, h, TexKind::Depth);
        if self.msaa > 1 {
            let ms = self.acquire_kind(w, h, TexKind::WorkMs);
            self.raw_pass(&pipeline, &ms.view, Some(&target.view), Some(&depth_tex.view), clear, &draws);
            self.release(ms);
        } else {
            self.raw_pass(&pipeline, &target.view, None, Some(&depth_tex.view), clear, &draws);
        }
        self.release(depth_tex);
        target
    }

    /// Add `a` (and `b`) times their weights to the motion blur sum.
    fn accumulate(&mut self, sum: &mut Option<Tex>, a: &Tex, b: Option<&Tex>, weights: [f32; 2]) {
        let p = [[weights[0], weights[1], 0.0, 0.0]];
        match sum {
            Some(target) => {
                let target = target.clone();
                self.pass(Pipe::SumAdd, &target, true, a, b, &p);
            }
            None => {
                let target = self.acquire(a.w, a.h);
                self.pass(Pipe::Sum, &target, false, a, b, &p);
                *sum = Some(target);
            }
        }
    }

    /// The image so far as a texture: the background when nothing has been drawn yet.
    pub(crate) fn materialise(&mut self, acc: &mut Option<Tex>, background: wgpu::Color) -> Tex {
        if let Some(t) = acc {
            return t.clone();
        }
        let t = self.acquire(self.ctx.w, self.ctx.h);
        self.name(|| "clear".into());
        self.raw_pass(&self.pipes.full[&Pipe::Copy].clone(), &t.view, None, None, Some(background), &[]);
        *acc = Some(t.clone());
        t
    }

    /// Blend `source` (after the colour operations `ops`) over the image so far.
    pub(crate) fn blend_over(&mut self, acc: &mut Option<Tex>, flat: [f32; 4], source: Source, ops: &[ColorOp], mode: Blend, opacity: f32) {
        let out = self.acquire(self.ctx.w, self.ctx.h);
        self.compose(&out, source, ops, Sink::Over { dst: acc.as_ref(), flat, mode, opacity });
        if let Some(old) = acc.replace(out) {
            self.release(old);
        }
    }

    // ---- frames --------------------------------------------------------------

    /// Render output frame `frame_index` (0-based) at `scale` times the project
    /// resolution. Returns straight sRGB RGBA8, top row first.
    pub fn render_frame(&mut self, frame_index: u64, scale: f32) -> Result<RgbaImage> {
        let t = self.scene.as_ref().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?.project.frame_time(frame_index);
        self.render(t, frame_index, scale)
    }

    /// Render the frame that starts at project time `time` (in the project's
    /// time base: beats or seconds).
    pub fn render_at(&mut self, time: f64, scale: f32) -> Result<RgbaImage> {
        let scene = self.scene.as_ref().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let p = &scene.project;
        let frame = (p.to_seconds(time) * p.settings.fps + 1e-6).floor().max(0.0) as u64;
        self.render(time, frame, scale)
    }

    /// Check an output size and start a frame of that size: a different size
    /// than the last frame's empties the texture pool.
    fn begin_frame(&mut self, w: u32, h: u32, scale: f32, samples: u32) -> Result<()> {
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        if w > max || h > max || w as u64 * h as u64 > MAX_PIXELS {
            bail!("a {w}x{h} frame is too large to render (scale {scale}); the limit is {max} pixels per side and {} megapixels", MAX_PIXELS / 1_000_000);
        }
        // Comp pictures belong to one output frame.
        self.drop_comp_pictures();
        if let Some(scene) = &self.scene {
            self.comp_cache.root = (scene.project.settings.width, scene.project.settings.height, w, h);
        }
        if self.pool_size != (w, h) {
            // A different output size: the pooled textures are of no use any more.
            self.pool.clear();
            self.binds.clear();
            self.fx_binds.clear();
            self.pool_size = (w, h);
        }
        self.stats = FrameStats { passes: 0, samples, rendered_samples: samples, width: w, height: h };
        Ok(())
    }

    fn render(&mut self, time: f64, frame: u64, scale: f32) -> Result<RgbaImage> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let project = &scene.project;
        let s = &project.settings;
        let (w, h) = scaled_size(project, scale);
        let samples = self.samples_override.unwrap_or(s.motion_blur.samples).clamp(1, 64);
        self.begin_frame(w, h, scale, samples)?;
        self.frame = frame;
        let ev = Evaluator::new(project);
        let shutter = s.motion_blur.shutter.clamp(0.0, 1.0) * project.frame_len();

        // Sub-frame instants at which the project evaluates to exactly the
        // same values give the same picture: render each distinct one once.
        let mut instants: Vec<(f64, Vec<u64>, u32)> = Vec::new();
        for i in 0..samples {
            let t = time + shutter * i as f64 / samples as f64;
            let state = stack_signature(&ev, &scene, &project.layers, t);
            match instants.iter_mut().find(|(_, s, _)| *s == state) {
                Some(same) => same.2 += 1,
                None => instants.push((t, state, 1)),
            }
        }
        self.stats.rendered_samples = instants.len() as u32;

        let mut sum: Option<Tex> = None;
        let mut waiting: Option<(Tex, f32)> = None;
        let single = instants.len() == 1;
        for (t, _, count) in instants {
            let acc = self.render_stack(&project.layers, t, w, h)?;
            if single {
                sum = Some(acc);
                break;
            }
            // Average in linear light, two samples per pass.
            let weight = count as f32 / samples as f32;
            match waiting.take() {
                None => waiting = Some((acc, weight)),
                Some((first, first_weight)) => {
                    self.accumulate(&mut sum, &first, Some(&acc), [first_weight, weight]);
                    self.release(first);
                    self.release(acc);
                }
            }
            // One submission per sample keeps the driver's queue short.
            self.flush();
        }
        if let Some((last, weight)) = waiting.take() {
            self.accumulate(&mut sum, &last, None, [weight, 0.0]);
            self.release(last);
        }
        let sum = sum.expect("at least one sample");
        let image = self.read_back(&sum);
        self.release(sum);
        image
    }

    /// Composite some layers of the prepared project at project time `time`
    /// (one instant: no motion blur) over the background, at `scale` times
    /// the project resolution. `render_layers(&project.layers, t, s)` is the
    /// frame at `t` without motion blur; a subset shows what those layers
    /// alone look like. The layers need not be the project's own, but the
    /// slots and custom shaders they use must have been prepared.
    pub fn render_layers(&mut self, layers: &[Layer], time: f64, scale: f32) -> Result<RgbaImage> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let p = &scene.project;
        let (w, h) = scaled_size(p, scale);
        self.begin_frame(w, h, scale, 1)?;
        self.frame = (p.to_seconds(time) * p.settings.fps + 1e-6).floor().max(0.0) as u64;
        let out = self.render_stack(layers, time, w, h)?;
        let image = self.read_back(&out);
        self.release(out);
        image
    }

    /// One layer's own picture at project time `time` (see
    /// `Renderer::layer_content`): drawn through the camera with all of its
    /// effects, without its opacity, blend mode or transition, over
    /// transparent black. Straight sRGB RGBA8 like a frame, but with alpha
    /// kept. `None` for an adjustment layer or when the layer is not visible
    /// at `time`.
    pub fn render_layer(&mut self, layer: &Layer, time: f64, scale: f32) -> Result<Option<RgbaImage>> {
        let scene = self.scene.clone().ok_or_else(|| anyhow!("no project prepared: call prepare() first"))?;
        let p = &scene.project;
        let (w, h) = scaled_size(p, scale);
        self.begin_frame(w, h, scale, 1)?;
        self.frame = (p.to_seconds(time) * p.settings.fps + 1e-6).floor().max(0.0) as u64;
        self.set_time(p, time, w, h);
        let Some(content) = self.layer_content(layer, time)? else { return Ok(None) };
        let image = self.read_back_alpha(&content, true);
        self.release(content);
        image.map(Some)
    }

    /// The quads of an image, solid, cube or video layer, and whether they
    /// need a depth buffer. `frame` is a video layer's current frame (`None`
    /// shows its slot's placeholder).
    pub(crate) fn layer_quads(&self, scene: &Scene, layer: &Layer, mvp: glam::DMat4, aspect: f64, frame: Option<&Picture>) -> (Vec<Quad>, bool) {
        let white = [1.0, 1.0, 1.0, 1.0];
        let (w, h) = (self.ctx.w, self.ctx.h);
        let flat = !matches!(layer.kind, LayerKind::Cube { .. });
        // Flat layers fade their edges unless the edges are out of sight.
        let edges = |m: glam::DMat4| {
            let covers = geometry::covers_frame(m, w, h);
            (covers, if covers || !flat { [0.0, 0.0] } else { geometry::edge_margin(m, w, h) })
        };
        let picture_quad = |name: &str, m: glam::DMat4, box_aspect: f64, picture: Option<&Picture>| {
            let (covers, margin) = edges(m);
            let slot = scene.project.slots.get(name);
            let (fit, focus) = slot.map(|s| (s.fit, s.focus)).unwrap_or((edits_core::Fit::Cover, [0.5, 0.5]));
            match picture.or_else(|| scene.slots.get(name)) {
                Some(pic) => Quad { mvp: mat(m), tex: pic.tex.clone(), fit: geometry::fit_uv(fit, focus, pic.aspect, box_aspect), color: white, covers, margin },
                // Not reachable after `prepare`, which gives every used slot a picture.
                None => Quad { mvp: mat(m), tex: self.white.clone(), fit: geometry::fit_uv(edits_core::Fit::Stretch, focus, 1.0, 1.0), color: [1.0, 0.0, 1.0, 1.0], covers, margin },
            }
        };
        let slot_quad = |name: &str, m: glam::DMat4, box_aspect: f64| picture_quad(name, m, box_aspect, None);
        match &layer.kind {
            LayerKind::Image { slot } => (vec![slot_quad(slot, mvp * geometry::flat(aspect, 1.0), aspect)], false),
            LayerKind::Video(v) => (vec![picture_quad(&v.slot, mvp * geometry::flat(aspect, 1.0), aspect, frame)], false),
            LayerKind::Solid { color } => {
                let a = color[3].clamp(0.0, 1.0);
                let c = [0, 1, 2].map(|i| assets::srgb_to_linear(color[i].clamp(0.0, 1.0)) * a);
                let fit = geometry::fit_uv(edits_core::Fit::Stretch, [0.5, 0.5], 1.0, 1.0);
                let m = mvp * geometry::flat(aspect, 1.0);
                let (covers, margin) = edges(m);
                (vec![Quad { mvp: mat(m), tex: self.white.clone(), fit, color: [c[0], c[1], c[2], a], covers, margin }], false)
            }
            LayerKind::Cube { faces, size } => {
                let quads = geometry::cube_faces(*size)
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (m, face_aspect))| {
                        let name = faces.get(i).or(faces.last())?;
                        Some(slot_quad(name, mvp * *m, *face_aspect))
                    })
                    .collect();
                (quads, true)
            }
            // Text is drawn glyph by glyph (`text.rs`); adjustment layers have no picture.
            LayerKind::Adjustment | LayerKind::Text(_) => (Vec::new(), false),
            // Drawn by `particle_quads` and `shape_quads` instead.
            LayerKind::Particles(_) | LayerKind::Shape(_) => (Vec::new(), false),
            // `frame` is the comp's picture at this instant, in a box of the comp's size.
            LayerKind::Comp(c) => match (frame, scene.comps.get(&c.comp)) {
                (Some(pic), Some(sub)) => {
                    let (sw, sh) = (sub.project.settings.width as f64, sub.project.settings.height as f64);
                    let unit = scene.project.settings.height.max(1) as f64;
                    let m = mvp * geometry::flat(sw / unit, sh / unit);
                    // Shown pixel for pixel, it is copied exactly (no resampling).
                    if (pic.tex.w, pic.tex.h) == (w, h) && crate::comp::maps_onto_frame(m) {
                        return (vec![Quad::frame(pic.tex.clone())], false);
                    }
                    let (covers, margin) = edges(m);
                    let fit = geometry::fit_uv(edits_core::Fit::Stretch, [0.5, 0.5], 1.0, 1.0);
                    (vec![Quad { mvp: mat(m), tex: pic.tex.clone(), fit, color: white, covers, margin }], false)
                }
                _ => (Vec::new(), false),
            },
        }
    }

    /// Copy a finished frame out and encode it: linear RGBA16Float in,
    /// straight sRGB RGBA8 out. The background is opaque, so alpha is 255.
    fn read_back(&mut self, src: &Tex) -> Result<RgbaImage> {
        self.read_back_alpha(src, false)
    }

    /// [`Self::read_back`], keeping alpha when `alpha` is true (colour is then
    /// un-premultiplied before it is encoded).
    fn read_back_alpha(&mut self, src: &Tex, alpha: bool) -> Result<RgbaImage> {
        let (w, h) = (src.w, src.h);
        if self.readback.as_ref().is_none_or(|r| (r.w, r.h) != (w, h)) {
            let row_bytes = (8 * w).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("edits readback"),
                size: row_bytes as u64 * h as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            self.readback = Some(Readback { w, h, buffer, row_bytes });
        }
        let (buffer, row_bytes) = self.readback.as_ref().map(|r| (r.buffer.clone(), r.row_bytes)).expect("just created");
        let device = &self.gpu.device;
        let encoder = self.encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("edits frame") }));
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &src.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row_bytes), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.flush();

        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.gpu.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| anyhow!("the GPU stopped responding: {e}"))?;
        rx.recv().map_err(|_| anyhow!("frame readback was cancelled"))?.map_err(|e| anyhow!("frame readback failed: {e}"))?;
        let lut = srgb_lut();
        let mut pixels = vec![255u8; (4 * w * h) as usize];
        {
            let data = buffer.get_mapped_range(..).map_err(|e| anyhow!("frame readback failed: {e}"))?;
            for (row, out) in data.chunks(row_bytes as usize).zip(pixels.chunks_exact_mut((4 * w) as usize)) {
                for (texel, px) in row.chunks_exact(8).zip(out.chunks_exact_mut(4)) {
                    let half = |c: usize| u16::from_le_bytes([texel[2 * c], texel[2 * c + 1]]);
                    if alpha {
                        let a = half_to_f32(half(3)).clamp(0.0, 1.0);
                        px[3] = (a * 255.0).round() as u8;
                        for (c, out) in px[..3].iter_mut().enumerate() {
                            let v = if a > 0.0 { half_to_f32(half(c)) / a } else { 0.0 };
                            *out = if v.is_nan() || v <= 0.0 { 0 } else { (assets::linear_to_srgb(v.min(1.0)) * 255.0).round() as u8 };
                        }
                    } else {
                        for c in 0..3 {
                            px[c] = lut[half(c) as usize];
                        }
                    }
                }
            }
        }
        buffer.unmap();
        RgbaImage::from_raw(w, h, pixels).ok_or_else(|| anyhow!("frame readback returned the wrong size"))
    }
}

/// The value of a half-float bit pattern.
fn half_to_f32(bits: u16) -> f32 {
    let (exponent, mantissa) = ((bits >> 10) & 0x1F, (bits & 0x3FF) as f32);
    let value = match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => {
            if mantissa == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + mantissa / 1024.0) * 2f32.powi(e as i32 - 15),
    };
    if bits & 0x8000 != 0 {
        -value
    } else {
        value
    }
}

/// Every half-float bit pattern -> its 8-bit sRGB value. Negative values and
/// NaN become 0, values above 1 become 255.
fn srgb_lut() -> &'static [u8] {
    static LUT: OnceLock<Vec<u8>> = OnceLock::new();
    LUT.get_or_init(|| {
        (0..=u16::MAX)
            .map(|bits| {
                let value = half_to_f32(bits);
                if value.is_nan() || value <= 0.0 {
                    0
                } else {
                    (assets::linear_to_srgb(value.min(1.0)) * 255.0).round() as u8
                }
            })
            .collect()
    })
}

/// Project size at `scale`, each side rounded to an even number of pixels.
pub(crate) fn scaled_size(project: &Project, scale: f32) -> (u32, u32) {
    let scale = if scale.is_finite() && scale > 0.0 { scale as f64 } else { 1.0 };
    let side = |v: u32| (((v as f64 * scale / 2.0).round() as u32) * 2).max(2);
    (side(project.settings.width), side(project.settings.height))
}

/// A 3D lookup table on the GPU.
#[derive(Clone)]
pub(crate) struct Lut {
    pub id: u64,
    pub view: wgpu::TextureView,
}

fn make_lut(device: &wgpu::Device, queue: &wgpu::Queue, size: u32, rgb: &[[f32; 3]], id: u64) -> Lut {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("edits lut"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: size },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut bytes = Vec::with_capacity(rgb.len() * 8);
    for c in rgb {
        for v in [c[0], c[1], c[2], 1.0] {
            bytes.extend_from_slice(&f32_to_half(v).to_le_bytes());
        }
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8 * size), rows_per_image: Some(size) },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: size },
    );
    Lut { id, view: texture.create_view(&wgpu::TextureViewDescriptor::default()) }
}

/// The nearest half-float to `v` (round to nearest even).
fn f32_to_half(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xff) as i32;
    let mant = x & 0x7f_ffff;
    if exp == 255 {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let round = |value: u32, rest: u32, halfway: u32| if rest > halfway || (rest == halfway && value & 1 == 1) { value + 1 } else { value };
    let e = exp - 127 + 15;
    if e >= 31 {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mant | 0x80_0000;
        let shift = (14 - e) as u32;
        return sign | round(m >> shift, m & ((1 << shift) - 1), 1 << (shift - 1)) as u16;
    }
    // A carry out of the mantissa correctly moves to the next exponent (or to infinity).
    sign | round(((e as u32) << 10) | (mant >> 13), mant & 0x1fff, 0x1000) as u16
}

/// A stand-in bound to texture slots a pass does not use (replaced by a white pixel once uploads work).
fn dummy(device: &wgpu::Device) -> Tex {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: IMAGE_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    Tex { id: 0, view: texture.create_view(&wgpu::TextureViewDescriptor::default()), texture, w: 1, h: 1, kind: TexKind::Image }
}

#[allow(clippy::too_many_arguments)]
fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    vertex: &str,
    fragment: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    samples: u32,
    depth: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some(vertex), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // Quads are wound top-left, top-right, bottom-right as seen from
            // outside, which is clockwise on screen.
            front_face: wgpu::FrontFace::Cw,
            cull_mode: depth.then_some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: depth.then(|| wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_trip() {
        for v in [0.0f32, 1.0, -2.5, 0.5, 65504.0, 1e-5, 6.1e-5, 0.333_333, 1e9, -1e-9] {
            let back = half_to_f32(f32_to_half(v));
            let tolerance = (v.abs() * 1e-3).max(6e-8);
            if v.abs() > 65520.0 {
                assert!(back.is_infinite(), "{v}");
            } else {
                assert!((back - v).abs() <= tolerance, "{v} -> {back}");
            }
        }
        assert_eq!(f32_to_half(1.0), 0x3c00);
        assert_eq!(f32_to_half(-0.0), 0x8000);
        assert!(half_to_f32(f32_to_half(f32::NAN)).is_nan());
    }

    #[test]
    fn effects_read_a_3d_lookup_table() {
        let Ok(mut r) = Renderer::new(RenderOptions::default()) else {
            eprintln!("skipping: no adapter");
            return;
        };
        let p = Project::from_json(r#"{"format":"edits.project/1","settings":{"width":8,"height":8,"fps":10,"duration":1},"layers":[{"id":"s","kind":"solid","color":[1,0.5,0,1]}]}"#).unwrap();
        r.prepare(&p, Path::new(".")).unwrap();
        r.begin_frame(8, 8, 1.0, 1).unwrap();
        let base = r.render_stack(&p.layers, 0.0, 8, 8).unwrap();
        let pipeline = r.compile_custom("lut", "fn effect(uv: vec2<f32>) -> vec4<f32> { return vec4<f32>(fx_lut(src(uv).rgb), 1.0); }").unwrap();
        // Without a table of its own an effect sees the identity.
        let out = r.acquire(8, 8);
        r.fx_pass(&pipeline, &out, &base, None, None, &[0.0; 16]);
        // (Filtering weights are 8-bit on some adapters: allow one step.)
        let got = r.read_back(&out).unwrap().get_pixel(4, 4).0;
        assert!(got[0] == 255 && got[2] == 0 && (got[1] as i32 - 128).abs() <= 1, "{got:?}");
        // An inverting table (affine, so trilinear filtering is exact).
        let corners: Vec<[f32; 3]> = (0..8).map(|i| [1.0 - (i & 1) as f32, 1.0 - ((i >> 1) & 1) as f32, 1.0 - ((i >> 2) & 1) as f32]).collect();
        let lut = r.upload_lut(2, &corners).unwrap();
        r.fx_pass(&pipeline, &out, &base, None, Some(&lut), &[0.0; 16]);
        let green = (assets::linear_to_srgb(1.0 - assets::srgb_to_linear(0.5)) * 255.0).round() as u8;
        let got = r.read_back(&out).unwrap().get_pixel(4, 4).0;
        assert!(got[0] == 0 && got[2] == 255 && (got[1] as i32 - green as i32).abs() <= 1, "{got:?}");
        assert!(r.upload_lut(3, &corners).is_err(), "a table of the wrong size is refused");
    }
}
