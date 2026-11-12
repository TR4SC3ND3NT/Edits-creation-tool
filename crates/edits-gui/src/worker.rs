//! The render thread. It owns the `Renderer`; the window never calls it.
//!
//! The window posts what it wants (a project to prepare, a frame to render)
//! into a mailbox that holds only the newest of each. The thread takes
//! whatever is there when it becomes free, so a request that was overtaken
//! while a frame was rendering is never rendered at all.

use crate::cache::FrameKey;
use eframe::egui::ColorImage;
use edits_core::Project;
use edits_render::{AdapterInfo, RenderOptions, Renderer};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

/// A slot that keeps only the newest value posted to it.
#[derive(Debug)]
pub struct Latest<T> {
    value: Option<T>,
}

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self { value: None }
    }
}

impl<T> Latest<T> {
    /// Post a value. Returns the one it replaced, which will never be handled.
    pub fn post(&mut self, value: T) -> Option<T> {
        self.value.replace(value)
    }
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
    pub fn is_empty(&self) -> bool {
        self.value.is_none()
    }
}

/// A version of the project for the renderer to load.
#[derive(Clone, Debug)]
pub struct ProjectMsg {
    pub revision: u64,
    pub project: Arc<Project>,
    pub base_dir: PathBuf,
}

#[derive(Default)]
struct Inbox {
    project: Latest<ProjectMsg>,
    request: Latest<FrameKey>,
    quit: bool,
}

/// What the render thread tells the window.
pub enum WorkerEvent {
    /// The adapter is open.
    Adapter(AdapterInfo),
    /// There is no usable graphics adapter; nothing will be rendered.
    NoAdapter(String),
    /// A revision's pictures and shaders are loaded.
    Prepared { revision: u64, ms: f64 },
    /// A revision could not be loaded (a picture that does not decode, a shader that does not compile).
    PrepareFailed { revision: u64, error: String },
    Frame { key: FrameKey, image: Arc<ColorImage>, ms: f64 },
    RenderFailed { key: FrameKey, error: String },
    /// The request was for a revision the renderer does not hold.
    Skipped { key: FrameKey },
}

/// The window's end of the render thread.
pub struct Worker {
    shared: Arc<(Mutex<Inbox>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    /// Start the thread. Events are passed to `send`, and `wake` is called
    /// after each so the window repaints.
    pub fn spawn(options: RenderOptions, events: Sender<WorkerEvent>, wake: impl Fn() + Send + 'static) -> Self {
        let shared: Arc<(Mutex<Inbox>, Condvar)> = Arc::default();
        let inbox = shared.clone();
        let thread = std::thread::Builder::new()
            .name("edits-render".into())
            .spawn(move || {
                let emit = |e: WorkerEvent| {
                    let _ = events.send(e);
                    wake();
                };
                run(options, &inbox, &emit);
            })
            .ok();
        Self { shared, thread }
    }

    /// Give the renderer a new version of the project.
    pub fn set_project(&self, msg: ProjectMsg) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            inbox.project.post(msg);
            // A frame asked of the old version is of no use any more.
            inbox.request.take();
        }
        cv.notify_one();
    }

    /// Ask for a frame. Replaces any request not yet started.
    pub fn request(&self, key: FrameKey) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            inbox.request.post(key);
        }
        cv.notify_one();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        if let Ok(mut inbox) = lock.lock() {
            inbox.quit = true;
        }
        cv.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(options: RenderOptions, shared: &(Mutex<Inbox>, Condvar), emit: &dyn Fn(WorkerEvent)) {
    let mut renderer = match Renderer::new(options) {
        Ok(r) => {
            emit(WorkerEvent::Adapter(r.info().clone()));
            Some(r)
        }
        Err(e) => {
            emit(WorkerEvent::NoAdapter(format!("{e:#}")));
            None
        }
    };
    let mut prepared: Option<u64> = None;
    let (lock, cv) = shared;
    loop {
        let (project, request) = {
            let Ok(mut inbox) = lock.lock() else { return };
            while !inbox.quit && inbox.project.is_empty() && inbox.request.is_empty() {
                inbox = match cv.wait(inbox) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if inbox.quit {
                return;
            }
            (inbox.project.take(), inbox.request.take())
        };
        let Some(renderer) = renderer.as_mut() else {
            if let Some(key) = request {
                emit(WorkerEvent::Skipped { key });
            }
            continue;
        };
        if let Some(msg) = project {
            let t = Instant::now();
            match renderer.prepare(&msg.project, &msg.base_dir) {
                Ok(()) => {
                    prepared = Some(msg.revision);
                    emit(WorkerEvent::Prepared { revision: msg.revision, ms: t.elapsed().as_secs_f64() * 1000.0 });
                }
                Err(e) => {
                    prepared = None;
                    emit(WorkerEvent::PrepareFailed { revision: msg.revision, error: format!("{e:#}") });
                }
            }
        }
        if let Some(key) = request {
            if prepared != Some(key.revision) {
                emit(WorkerEvent::Skipped { key });
                continue;
            }
            renderer.set_motion_blur_samples(if key.blur { None } else { Some(1) });
            let t = Instant::now();
            match renderer.render_frame(key.frame, key.scale.factor()) {
                Ok(img) => {
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    let size = [img.width() as usize, img.height() as usize];
                    // The renderer's output is opaque, so straight and premultiplied alpha are the same bytes.
                    let image = Arc::new(ColorImage::from_rgba_premultiplied(size, img.as_raw()));
                    emit(WorkerEvent::Frame { key, image, ms });
                }
                Err(e) => emit(WorkerEvent::RenderFailed { key, error: format!("{e:#}") }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Scale;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn latest_keeps_only_the_newest() {
        let mut slot = Latest::default();
        assert!(slot.is_empty());
        assert_eq!(slot.post(1), None);
        assert_eq!(slot.post(2), Some(1), "the stale request is dropped");
        assert_eq!(slot.post(3), Some(2));
        assert_eq!(slot.take(), Some(3));
        assert_eq!(slot.take(), None);
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

    fn key(revision: u64, frame: u64) -> FrameKey {
        FrameKey { revision, frame, scale: Scale::Half, blur: false }
    }

    /// Wait for the next frame-related event.
    fn next(rx: &mpsc::Receiver<WorkerEvent>) -> WorkerEvent {
        rx.recv_timeout(Duration::from_secs(60)).expect("the render thread answers")
    }

    #[test]
    fn renders_the_newest_request_off_the_calling_thread() {
        let (tx, rx) = mpsc::channel();
        let worker = Worker::spawn(RenderOptions::default(), tx, || {});
        match next(&rx) {
            WorkerEvent::Adapter(_) => {}
            // A machine without any graphics adapter cannot run this test.
            WorkerEvent::NoAdapter(e) => {
                eprintln!("skipped: {e}");
                return;
            }
            _ => panic!("the first event names the adapter"),
        }

        // A request before any project is answered, not left hanging.
        worker.request(key(1, 0));
        assert!(matches!(next(&rx), WorkerEvent::Skipped { .. }));

        worker.set_project(ProjectMsg { revision: 1, project: project("[1,0,0,1]"), base_dir: ".".into() });
        // Three requests in a row, faster than anything can render: the
        // first may or may not have started, the middle one is dropped.
        worker.request(key(1, 1));
        worker.request(key(1, 2));
        worker.request(key(1, 3));
        let mut frames = Vec::new();
        loop {
            match next(&rx) {
                WorkerEvent::Prepared { revision, .. } => assert_eq!(revision, 1),
                WorkerEvent::Frame { key, image, .. } => {
                    assert_eq!(image.size, [32, 32], "half of 64x64");
                    let px = image.pixels[0];
                    assert!(px.r() > 200 && px.g() < 50, "red, got {px:?}");
                    frames.push(key.frame);
                    if key.frame == 3 {
                        break;
                    }
                }
                _ => panic!("unexpected event"),
            }
        }
        assert!(frames.len() <= 2 && frames.last() == Some(&3), "at least one request was dropped: {frames:?}");

        // A new version replaces the old one; requests for the old are refused.
        worker.set_project(ProjectMsg { revision: 2, project: project("[0,0,1,1]"), base_dir: ".".into() });
        worker.request(key(2, 0));
        loop {
            match next(&rx) {
                WorkerEvent::Prepared { revision, .. } => assert_eq!(revision, 2),
                WorkerEvent::Frame { key, image, .. } => {
                    assert_eq!(key.revision, 2);
                    assert!(image.pixels[0].b() > 200);
                    break;
                }
                _ => panic!("unexpected event"),
            }
        }
        worker.request(key(1, 0));
        assert!(matches!(next(&rx), WorkerEvent::Skipped { key } if key.revision == 1));
    }
}
