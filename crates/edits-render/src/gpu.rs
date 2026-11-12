//! Device creation. Everything is offscreen: no surface, window or display
//! server is involved.

use anyhow::{anyhow, bail, Result};
use std::fmt;
use std::str::FromStr;

/// Which kind of adapter to render on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterPolicy {
    /// A hardware GPU when one exists, otherwise a software rasteriser.
    #[default]
    Auto,
    /// A hardware GPU, or fail.
    Hardware,
    /// A software rasteriser (lavapipe, WARP, SwiftShader), or fail.
    Software,
}

impl FromStr for AdapterPolicy {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "hardware" | "gpu" => Ok(Self::Hardware),
            "software" | "cpu" => Ok(Self::Software),
            other => bail!("unknown adapter policy \"{other}\" (use auto, hardware or software)"),
        }
    }
}

impl fmt::Display for AdapterPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Hardware => "hardware",
            Self::Software => "software",
        })
    }
}

/// What the renderer ended up running on.
#[derive(Clone, Debug)]
pub struct AdapterInfo {
    pub name: String,
    /// `Vulkan`, `Metal`, `Dx12`, `Gl`.
    pub backend: String,
    /// `DiscreteGpu`, `IntegratedGpu`, `VirtualGpu`, `Cpu`, `Other`.
    pub device_type: String,
    pub driver: String,
    /// True when the adapter is a CPU rasteriser.
    pub software: bool,
}

pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub info: AdapterInfo,
}

/// The policy from `EDITS_ADAPTER`, or `Auto` when it is unset.
pub fn policy_from_env() -> Result<AdapterPolicy> {
    match std::env::var("EDITS_ADAPTER") {
        Ok(v) => v.parse().map_err(|e| anyhow!("EDITS_ADAPTER: {e}")),
        Err(_) => Ok(AdapterPolicy::Auto),
    }
}

fn is_software(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::Cpu
}

async fn pick_adapter(instance: &wgpu::Instance, policy: AdapterPolicy) -> Result<wgpu::Adapter> {
    let request = |fallback: bool| {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: None,
            ..Default::default()
        };
        async move { instance.request_adapter(&options).await }
    };
    match policy {
        AdapterPolicy::Software => request(true).await.map_err(|e| {
            anyhow!("no software adapter found ({e}); install a CPU rasteriser such as Mesa lavapipe (package mesa-vulkan-drivers)")
        }),
        AdapterPolicy::Hardware => {
            let adapter = request(false).await.map_err(|e| anyhow!("no GPU adapter found ({e})"))?;
            let info = adapter.get_info();
            if is_software(&info) {
                bail!("no hardware GPU found: the only adapter is the software rasteriser \"{}\"; use the adapter policy auto or software", info.name);
            }
            Ok(adapter)
        }
        AdapterPolicy::Auto => match request(false).await {
            Ok(a) => Ok(a),
            Err(first) => request(true).await.map_err(|second| {
                anyhow!("no graphics adapter found ({first}; software fallback: {second}); install GPU drivers or Mesa lavapipe (package mesa-vulkan-drivers)")
            }),
        },
    }
}

impl Gpu {
    pub fn new(policy: AdapterPolicy) -> Result<Self> {
        pollster::block_on(Self::new_async(policy))
    }

    async fn new_async(policy: AdapterPolicy) -> Result<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY | wgpu::Backends::GL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pick_adapter(&instance, policy).await?;
        let raw = adapter.get_info();
        let limits = wgpu::Limits::default().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("edits"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow!("cannot open the adapter \"{}\": {e}", raw.name))?;
        let info = AdapterInfo {
            name: raw.name.clone(),
            backend: format!("{:?}", raw.backend),
            device_type: format!("{:?}", raw.device_type),
            driver: if raw.driver_info.is_empty() { raw.driver.clone() } else { format!("{} {}", raw.driver, raw.driver_info) },
            software: is_software(&raw),
        };
        Ok(Self { device, queue, info })
    }
}
