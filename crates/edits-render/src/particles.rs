//! Particle layers on the GPU.
//!
//! The particles alive at an instant come from `edits_core::particles`
//! (closed form: no state is carried between frames). Here they become one
//! instance buffer and one instanced draw of `shaders/particles.wgsl` into
//! a frame-sized transparent texture, through the camera and the layer's
//! transform like a flat layer. That texture is handed back as a quad that
//! covers the frame, so the rest of the stack (effects, opacity, blend mode,
//! transitions, motion blur samples) treats a particles layer like any other.

use crate::renderer::{Quad, Renderer, Scene, Tex, WORK_FORMAT};
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use edits_core::particles::{self, Particles, Sprite};
use edits_core::{Evaluator, Layer};
use glam::{DMat4, DVec4};
use std::collections::HashMap;

/// One particle as the vertex shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    center: [f32; 2],
    axis: [f32; 2],
    half: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    mvp: [f32; 16],
    res: [f32; 4],
    info: [f32; 4],
}

/// The particle pipeline and its buffers, made the first time a particles
/// layer is drawn.
#[derive(Default)]
pub(crate) struct ParticleGpu {
    state: Option<State>,
    /// The layer's picture: reused by every particles layer, since each one
    /// is composited before the next is drawn.
    target: Option<Tex>,
}

struct State {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    capacity: u64,
    sampler: wgpu::Sampler,
    /// Shown by the procedural sprites, which read no picture.
    blank: wgpu::TextureView,
    /// Bind groups by sprite picture (texture id; 0 for none).
    binds: HashMap<u64, wgpu::BindGroup>,
}

impl State {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edits particles"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/particles.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edits particles"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("edits particles"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4];
        // Premultiplied "over".
        let over = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edits particles"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_particle"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Instance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &attributes })],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_particle"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: WORK_FORMAT, blend: Some(wgpu::BlendState { color: over, alpha: over }), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("edits particle uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 1024;
        let instances = instance_buffer(device, capacity);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("edits particle sprites"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("edits particle blank"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &blank, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &[255, 255, 255, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let blank = blank.create_view(&wgpu::TextureViewDescriptor::default());
        Self { pipeline, layout, uniforms, instances, capacity, sampler, blank, binds: HashMap::new() }
    }

    fn bind(&mut self, device: &wgpu::Device, picture: Option<&Tex>) -> wgpu::BindGroup {
        let key = picture.map(|t| t.id).unwrap_or(0);
        if let Some(b) = self.binds.get(&key) {
            return b.clone();
        }
        let view = picture.map(|t| &t.view).unwrap_or(&self.blank);
        let b = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edits particles"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        // Pictures change with the project: keep only a few.
        if self.binds.len() > 16 {
            self.binds.clear();
        }
        self.binds.insert(key, b.clone());
        b
    }
}

fn instance_buffer(device: &wgpu::Device, capacity: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("edits particle instances"),
        size: capacity * std::mem::size_of::<Instance>() as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The instances of `list`: quads in layer space.
fn instances(ps: &Particles, list: &[particles::Particle], picture_aspect: f64) -> Vec<Instance> {
    let sprite = ps.sprite();
    list.iter()
        .filter(|p| p.size > 0.0 && p.opacity > 0.0)
        .map(|p| {
            let (s, c) = p.rotation.to_radians().sin_cos();
            let mut axis = [c, s];
            let mut center = p.position;
            let mut half = [p.size * 0.5, p.size * 0.5];
            let mut color = p.color;
            match sprite {
                Some(Sprite::Spark) => {
                    let speed = p.velocity[0].hypot(p.velocity[1]);
                    if speed > 1e-9 {
                        axis = [p.velocity[0] / speed, p.velocity[1] / speed];
                    }
                    // The head is at the particle; the streak trails behind it.
                    let length = p.size + speed * ps.streak.max(0.0);
                    half = [length * 0.5, p.size * 0.5];
                    let back = (length - p.size) * 0.5;
                    center = [center[0] - axis[0] * back, center[1] - axis[1] * back];
                }
                Some(Sprite::Petal) => {
                    half = [p.size * 0.5, p.size * 0.32 * p.flip.abs()];
                    // The back of a petal is a little darker.
                    let light = if p.flip < 0.0 { 0.82 } else { 1.0 };
                    color = color.map(|v| v * light);
                }
                Some(Sprite::Slot(_)) => half = [p.size * 0.5 * picture_aspect, p.size * 0.5],
                _ => {}
            }
            let f = |v: f64| v as f32;
            Instance {
                center: center.map(f),
                axis: axis.map(f),
                half: half.map(f),
                color: [f(color[0]), f(color[1]), f(color[2]), f(p.opacity)],
            }
        })
        .collect()
}

fn sprite_code(sprite: Option<Sprite>) -> f32 {
    match sprite {
        Some(Sprite::Dot) | None => 0.0,
        Some(Sprite::Spark) => 1.0,
        Some(Sprite::Star) => 2.0,
        Some(Sprite::Petal) => 3.0,
        Some(Sprite::Slot(_)) => 4.0,
    }
}

impl Renderer {
    /// A particles layer at project time `t`: its particles drawn with
    /// `mvp` (camera times the layer's model) into a frame-sized texture,
    /// returned as one quad that covers the frame.
    pub(crate) fn particle_quads(&mut self, scene: &Scene, ev: &Evaluator, layer: &Layer, ps: &Particles, mvp: DMat4, t: f64) -> Result<(Vec<Quad>, bool)> {
        let (w, h) = (self.ctx.w, self.ctx.h);
        let list = particles::evaluate(ev, layer, ps, ev.layer_time(layer, t));
        let picture = ps.sprite_slot().and_then(|name| scene.slot_texture(name));
        let aspect = picture.as_ref().map(|p| p.w as f64 / p.h.max(1) as f64).unwrap_or(1.0);
        let data = instances(ps, &list, aspect);

        let target = match self.particles.target.take() {
            Some(t) if t.w == w && t.h == h => t,
            // Another size: the old texture is dropped with the pool's.
            _ => self.acquire(w, h),
        };
        self.particles.target = Some(target.clone());

        // Earlier particle draws read the same buffers: send them first.
        self.flush();
        let device = self.gpu.device.clone();
        let queue = self.gpu.queue.clone();
        let state = self.particles.state.get_or_insert_with(|| State::new(&device, &queue));
        if data.len() as u64 > state.capacity {
            state.capacity = (data.len() as u64).next_power_of_two();
            state.instances = instance_buffer(&device, state.capacity);
        }
        let uniforms = Uniforms {
            mvp: mvp.as_mat4().to_cols_array(),
            res: [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32],
            info: [sprite_code(ps.sprite()), units_per_pixel(mvp, w, h) as f32, 0.0, 0.0],
        };
        queue.write_buffer(&state.uniforms, 0, bytemuck::bytes_of(&uniforms));
        if !data.is_empty() {
            queue.write_buffer(&state.instances, 0, bytemuck::cast_slice(&data));
        }
        let bind = state.bind(&device, picture.as_ref());
        let pipeline = state.pipeline.clone();
        let buffer = state.instances.clone();

        let encoder = self.encoder.get_or_insert_with(|| device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("edits frame") }));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("particles"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if !data.is_empty() {
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..6, 0..data.len() as u32);
        }
        drop(pass);
        Ok((vec![Quad::frame(target)], false))
    }
}

/// About how many layer units one pixel spans at the layer's origin, for
/// the smallest size a particle is drawn at.
fn units_per_pixel(mvp: DMat4, w: u32, h: u32) -> f64 {
    // A point of the layer in pixels.
    let at = |x: f64, y: f64| {
        let c = mvp * DVec4::new(x, y, 0.0, 1.0);
        (c.w.abs() > 1e-9).then(|| [c.x / c.w * 0.5 * w as f64, c.y / c.w * 0.5 * h as f64])
    };
    let step = 0.01;
    let fallback = 1.0 / h as f64;
    match (at(0.0, 0.0), at(0.0, step), at(step, 0.0)) {
        (Some(a), Some(b), Some(c)) => {
            let px = (b[0] - a[0]).hypot(b[1] - a[1]).max((c[0] - a[0]).hypot(c[1] - a[1]));
            if px > 1e-9 {
                step / px
            } else {
                fallback
            }
        }
        _ => fallback,
    }
}
