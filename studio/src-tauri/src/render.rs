//! The render thread: it owns the `edits_render::Renderer` and answers frame
//! requests with JPEG bytes for the frame viewer.
//!
//! Requests come in two kinds. The **current** frame (what the viewer shows
//! now) has one place: a newer request replaces one that has not started,
//! which is answered as superseded, so scrubbing never queues up work.
//! **Prefetch** requests (frames ahead of the playhead while playing) wait in
//! a queue behind it and are dropped on a seek. Finished frames are cached
//! by revision, frame, scale and motion blur, least recently used out.

use edits_core::Project;
use edits_render::{AdapterInfo, RenderOptions, Renderer};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// Which frame, of which version of the project, how.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameKey {
    pub revision: u64,
    pub frame: u64,
    /// Preview scale in thousandths (500 = half the project's size).
    pub scale_milli: u32,
    pub blur: bool,
}

impl FrameKey {
    pub fn new(revision: u64, frame: u64, scale: f32, blur: bool) -> Self {
        FrameKey {
            revision,
            frame,
            scale_milli: (scale.clamp(0.05, 1.0) * 1000.0).round() as u32,
            blur,
        }
    }
    pub fn scale(&self) -> f32 {
        self.scale_milli as f32 / 1000.0
    }
}

pub type Reply = mpsc::Sender<Result<Arc<Vec<u8>>, String>>;

struct Job {
    key: FrameKey,
    reply: Reply,
}

#[derive(Clone)]
pub struct ProjectMsg {
    pub revision: u64,
    pub project: Arc<Project>,
    pub base_dir: PathBuf,
}

#[derive(Default)]
struct Inbox {
    project: Option<ProjectMsg>,
    current: Option<Job>,
    prefetch: VecDeque<Job>,
    quit: bool,
}

/// What the renderer reports, for the status line and `doctor`-like info.
#[derive(Serialize, Clone, Debug, Default)]
pub struct RenderStatus {
    pub adapter: Option<String>,
    pub backend: Option<String>,
    pub software: bool,
    pub error: Option<String>,
    /// Milliseconds the last frame took.
    pub last_ms: Option<f64>,
    pub prepared: Option<u64>,
    pub prepare_error: Option<String>,
    pub cached_frames: usize,
    pub cached_bytes: usize,
}

/// A frame cache that holds at most `budget` bytes, least recently used out.
pub struct Cache {
    map: HashMap<FrameKey, (Arc<Vec<u8>>, u64)>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

impl Cache {
    pub fn new(budget: usize) -> Self {
        Cache {
            map: HashMap::new(),
            tick: 0,
            bytes: 0,
            budget,
        }
    }
    pub fn get(&mut self, key: &FrameKey) -> Option<Arc<Vec<u8>>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|(v, t)| {
            *t = tick;
            v.clone()
        })
    }
    pub fn put(&mut self, key: FrameKey, value: Arc<Vec<u8>>) {
        self.tick += 1;
        self.bytes += value.len();
        if let Some((old, _)) = self.map.insert(key, (value, self.tick)) {
            self.bytes -= old.len();
        }
        while self.bytes > self.budget && self.map.len() > 1 {
            let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| *k)
            else {
                break;
            };
            if let Some((v, _)) = self.map.remove(&oldest) {
                self.bytes -= v.len();
            }
        }
    }
    /// Forget every frame of other revisions.
    pub fn keep_revision(&mut self, revision: u64) {
        self.map.retain(|k, _| k.revision == revision);
        self.bytes = self.map.values().map(|(v, _)| v.len()).sum();
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

/// The studio's end of the render thread.
pub struct RenderService {
    shared: Arc<(Mutex<Inbox>, Condvar)>,
    pub cache: Arc<Mutex<Cache>>,
    pub status: Arc<Mutex<RenderStatus>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// JPEG quality of preview frames: no visible blocking at preview size, about 60 KB at 540 x 540.
const QUALITY: u8 = 90;

pub fn encode_jpeg(img: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Vec::with_capacity((img.width() * img.height() / 4) as usize);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, QUALITY)
        .encode_image(&rgb)
        .map_err(|e| format!("cannot encode the frame: {e}"))?;
    Ok(out)
}

impl RenderService {
    pub fn spawn(options: RenderOptions, cache_bytes: usize) -> Self {
        let shared: Arc<(Mutex<Inbox>, Condvar)> = Arc::default();
        let cache = Arc::new(Mutex::new(Cache::new(cache_bytes)));
        let status: Arc<Mutex<RenderStatus>> = Arc::default();
        let (inbox, c, s) = (shared.clone(), cache.clone(), status.clone());
        let thread = std::thread::Builder::new()
            .name("edits-render".into())
            .spawn(move || run(options, &inbox, &c, &s))
            .ok();
        RenderService {
            shared,
            cache,
            status,
            thread: Mutex::new(thread),
        }
    }

    /// Give the renderer a new version of the project. Requests for older
    /// versions waiting in line are answered as stale.
    pub fn set_project(&self, msg: ProjectMsg) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            if let Some(job) = inbox.current.take() {
                let _ = job.reply.send(Err(
                    "stale: a newer version of the project was loaded".into()
                ));
            }
            for job in inbox.prefetch.drain(..) {
                let _ = job.reply.send(Err(
                    "stale: a newer version of the project was loaded".into()
                ));
            }
            inbox.project = Some(msg);
        }
        cv.notify_one();
    }

    /// Ask for a frame. Its answer arrives on the returned channel.
    pub fn request(
        &self,
        key: FrameKey,
        prefetch: bool,
    ) -> mpsc::Receiver<Result<Arc<Vec<u8>>, String>> {
        let (tx, rx) = mpsc::channel();
        if let Some(hit) = self.cache.lock().ok().and_then(|mut c| c.get(&key)) {
            let _ = tx.send(Ok(hit));
            return rx;
        }
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            let job = Job { key, reply: tx };
            if prefetch {
                // A frame asked twice is rendered once: the second copy finds it in the cache.
                inbox.prefetch.push_back(job);
                // Prefetching runs a little ahead, never far.
                while inbox.prefetch.len() > 48 {
                    if let Some(old) = inbox.prefetch.pop_front() {
                        let _ = old.reply.send(Err("superseded".into()));
                    }
                }
            } else if let Some(old) = inbox.current.replace(job) {
                let _ = old.reply.send(Err("superseded".into()));
            }
        }
        cv.notify_one();
        rx
    }

    /// Drop the frames waiting to be prefetched (after a seek, or on pause).
    pub fn clear_prefetch(&self) {
        let (lock, _) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            for job in inbox.prefetch.drain(..) {
                let _ = job.reply.send(Err("superseded".into()));
            }
        }
    }

    pub fn status(&self) -> RenderStatus {
        let mut s = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        if let Ok(c) = self.cache.lock() {
            s.cached_frames = c.len();
            s.cached_bytes = c.bytes();
        }
        s
    }
}

impl Drop for RenderService {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            inbox.quit = true;
        }
        cv.notify_one();
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
}

fn adapter_text(i: &AdapterInfo) -> String {
    format!("{} ({})", i.name, i.backend)
}

fn run(
    options: RenderOptions,
    shared: &(Mutex<Inbox>, Condvar),
    cache: &Mutex<Cache>,
    status: &Mutex<RenderStatus>,
) {
    let set = |f: &dyn Fn(&mut RenderStatus)| {
        if let Ok(mut s) = status.lock() {
            f(&mut s)
        }
    };
    let mut renderer = match Renderer::new(options) {
        Ok(r) => {
            let info = r.info().clone();
            set(&|s| {
                s.adapter = Some(adapter_text(&info));
                s.backend = Some(info.backend.clone());
                s.software = info.software;
            });
            Some(r)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            set(&|s| s.error = Some(msg.clone()));
            None
        }
    };
    let mut prepared: Option<u64> = None;
    let (lock, cv) = shared;
    loop {
        let (project, job) = {
            let Ok(mut inbox) = lock.lock() else { return };
            while !inbox.quit
                && inbox.project.is_none()
                && inbox.current.is_none()
                && inbox.prefetch.is_empty()
            {
                inbox = match cv.wait(inbox) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if inbox.quit {
                return;
            }
            let project = inbox.project.take();
            let job = if project.is_some() {
                None
            } else {
                inbox.current.take().or_else(|| inbox.prefetch.pop_front())
            };
            (project, job)
        };
        if let Some(msg) = project {
            let Some(r) = renderer.as_mut() else { continue };
            match r.prepare(&msg.project, &msg.base_dir) {
                Ok(()) => {
                    prepared = Some(msg.revision);
                    if let Ok(mut c) = cache.lock() {
                        c.keep_revision(msg.revision);
                    }
                    set(&|s| {
                        s.prepared = Some(msg.revision);
                        s.prepare_error = None;
                    });
                }
                Err(e) => {
                    prepared = None;
                    let text = format!("{e:#}");
                    set(&|s| s.prepare_error = Some(text.clone()));
                }
            }
            continue;
        }
        let Some(job) = job else { continue };
        let Some(r) = renderer.as_mut() else {
            let why = status
                .lock()
                .ok()
                .and_then(|s| s.error.clone())
                .unwrap_or_else(|| "no graphics adapter".into());
            let _ = job.reply.send(Err(format!("no renderer: {why}")));
            continue;
        };
        if prepared != Some(job.key.revision) {
            let _ = job.reply.send(Err(format!(
                "stale: revision {} is not loaded",
                job.key.revision
            )));
            continue;
        }
        if let Some(hit) = cache.lock().ok().and_then(|mut c| c.get(&job.key)) {
            let _ = job.reply.send(Ok(hit));
            continue;
        }
        r.set_motion_blur_samples(if job.key.blur { None } else { Some(1) });
        let t = Instant::now();
        let result = r
            .render_frame(job.key.frame, job.key.scale())
            .map_err(|e| format!("{e:#}"))
            .and_then(|img| encode_jpeg(&img));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(bytes) => {
                let bytes = Arc::new(bytes);
                if let Ok(mut c) = cache.lock() {
                    c.put(job.key, bytes.clone());
                }
                set(&|s| s.last_ms = Some(ms));
                let _ = job.reply.send(Ok(bytes));
            }
            Err(e) => {
                let _ = job.reply.send(Err(e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key(frame: u64) -> FrameKey {
        FrameKey::new(1, frame, 0.5, false)
    }

    #[test]
    fn the_cache_drops_the_least_recently_used_frame() {
        let mut c = Cache::new(250);
        c.put(key(0), Arc::new(vec![0; 100]));
        c.put(key(1), Arc::new(vec![0; 100]));
        assert!(
            c.get(&key(0)).is_some(),
            "frame 0 is now the most recently used"
        );
        c.put(key(2), Arc::new(vec![0; 100]));
        assert!(c.get(&key(1)).is_none(), "frame 1 was the oldest");
        assert!(c.get(&key(0)).is_some() && c.get(&key(2)).is_some());
        assert_eq!(c.bytes(), 200);
        c.keep_revision(2);
        assert_eq!((c.len(), c.bytes()), (0, 0));
    }

    #[test]
    fn scale_is_kept_in_thousandths() {
        assert_eq!(FrameKey::new(1, 0, 0.5, false).scale_milli, 500);
        assert_eq!(FrameKey::new(1, 0, 4.0, false).scale(), 1.0);
    }

    fn project(color: &str) -> Arc<Project> {
        Arc::new(
            Project::from_json(&format!(
                r#"{{"format":"edits.project/1","settings":{{"width":64,"height":64,"fps":25,"duration":4}},
                    "layers":[{{"id":"bg","kind":"solid","color":{color}}}]}}"#
            ))
            .unwrap(),
        )
    }

    #[test]
    fn renders_jpeg_frames_off_the_calling_thread() {
        let service = RenderService::spawn(RenderOptions::default(), 64 << 20);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let s = service.status();
            if let Some(e) = s.error {
                eprintln!("skipped: no graphics adapter ({e})");
                return;
            }
            if s.adapter.is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "the renderer did not start");
            std::thread::sleep(Duration::from_millis(20));
        }
        service.set_project(ProjectMsg {
            revision: 1,
            project: project("[1,0,0,1]"),
            base_dir: ".".into(),
        });
        let rx = service.request(FrameKey::new(1, 3, 0.5, false), false);
        let bytes = rx.recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (32, 32), "half of 64x64");
        let px = img.get_pixel(16, 16);
        assert!(px[0] > 200 && px[1] < 60, "red, got {px:?}");
        // Asked again: from the cache.
        let again = service
            .request(FrameKey::new(1, 3, 0.5, false), true)
            .recv()
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&bytes, &again));
        // An older revision is refused once a newer one is loaded.
        service.set_project(ProjectMsg {
            revision: 2,
            project: project("[0,0,1,1]"),
            base_dir: ".".into(),
        });
        let stale = service
            .request(FrameKey::new(1, 0, 0.5, false), false)
            .recv_timeout(Duration::from_secs(60))
            .unwrap();
        assert!(stale.unwrap_err().contains("stale"));
    }
}
