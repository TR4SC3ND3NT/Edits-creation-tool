//! Edits Studio: the desktop app of the edits engine.
//!
//! A Tauri v2 shell around a React + shadcn/ui frontend (`studio/src`). The
//! Rust side owns everything that touches the project: it reads and watches
//! the file (`engine::reload`), renders preview frames on a worker thread
//! (`render`, through `edits-render`), makes every edit through
//! `edits_core::edit` with actor `window` (`studio`), writes `ui.json` for
//! the agent (`uictx`), runs `edits render` for the Renders page (`renders`)
//! and compares renders with a reference analysis (`reference`). The agent
//! panel's ACP client and the agent's activity log are in `agent`. The
//! commands the frontend calls are in `commands` (and `agent::commands`).

mod agent;
mod commands;
mod engine;
mod inspect;
mod project;
mod reference;
mod render;
mod renders;
mod studio;
mod uictx;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::webview::PageLoadEvent;
use tauri::{Emitter, Manager};
use tauri_plugin_log::{Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

pub const USAGE: &str = "\
edits-studio: the desktop app of the edits engine

usage: edits-studio [project.json] [options]

  --page edit|overview|reference|library|activity|renders
                       page to open on (default: edit)
  --select <layer id>  layer to select in the inspector
  --at <frame>         frame to start on
  -h, --help           this text

The renderer's adapter is chosen by EDITS_ADAPTER (auto, hardware, software).
`edits gui [project.json]` starts this app when it is built (docs/GUI.md).
";

/// What the command line asked for, handed to the frontend at start.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Startup {
    pub project: Option<String>,
    pub page: Option<String>,
    pub select: Option<String>,
    pub at: Option<u64>,
}

const PAGES: [&str; 6] = [
    "edit",
    "overview",
    "reference",
    "library",
    "activity",
    "renders",
];

impl Startup {
    /// `Ok(None)` for `--help`.
    pub fn parse(args: &[String]) -> Result<Option<Startup>, String> {
        let mut s = Startup::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = |name: &str| {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match a.as_str() {
                "-h" | "--help" => return Ok(None),
                "--page" => {
                    let p = value("--page")?;
                    if !PAGES.contains(&p.as_str()) {
                        return Err(format!(
                            "--page takes one of {}, got \"{p}\"",
                            PAGES.join(", ")
                        ));
                    }
                    s.page = Some(p);
                }
                "--select" => s.select = Some(value("--select")?),
                "--at" => {
                    let v = value("--at")?;
                    s.at = Some(
                        v.parse()
                            .map_err(|_| format!("--at takes a frame number, got \"{v}\""))?,
                    );
                }
                other if other.starts_with('-') && other.len() > 1 => {
                    return Err(format!("unknown option {other}"))
                }
                _ if s.project.is_none() => s.project = Some(a.clone()),
                _ => {
                    return Err(format!(
                        "only one project file can be opened, got a second: {a}"
                    ))
                }
            }
        }
        if let Some(p) = &s.project {
            let path = PathBuf::from(p);
            if !path.is_file() {
                return Err(format!("project file not found: {p}"));
            }
            s.project = Some(path.canonicalize().unwrap_or(path).display().to_string());
        }
        Ok(Some(s))
    }
}

fn external_navigation_plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("external-navigation")
        .on_navigation(|webview, url| {
            let is_internal_host = matches!(
                url.host_str(),
                Some("localhost") | Some("127.0.0.1") | Some("tauri.localhost") | Some("::1")
            );
            let is_internal = url.scheme() == "tauri" || is_internal_host;
            if is_internal {
                return true;
            }
            let is_external_link = matches!(url.scheme(), "http" | "https" | "mailto" | "tel");
            if is_external_link {
                log::info!("opening external link in system browser: {}", url);
                let _ = webview.opener().open_url(url.as_str(), None::<&str>);
                return false;
            }
            true
        })
        .build()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(startup: Startup) {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                    Target::new(TargetKind::Webview),
                ])
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(external_navigation_plugin())
        .manage(startup)
        .setup(|app| {
            let handle = app.handle().clone();
            let emit: studio::Emit = Arc::new(move |name, payload| {
                let _ = handle.emit(name, payload);
            });
            let render =
                render::RenderService::spawn(edits_render::RenderOptions::default(), 384 << 20);
            let studio = studio::Studio::new(render, emit.clone());
            app.manage(studio.clone());
            // The agent panel: an ACP client, and the agent's activity log.
            let (for_path, for_doc) = (Arc::downgrade(&studio), Arc::downgrade(&studio));
            let host = agent::AgentHost::new(
                emit,
                Arc::new(move || for_path.upgrade().and_then(|s| s.path())),
                Arc::new(move || {
                    for_doc
                        .upgrade()
                        .and_then(|s| s.snapshot())
                        .and_then(|s| s.doc)
                }),
            );
            host.watch_activity();
            app.manage(host);
            // `ui.json` is rewritten on a heartbeat while the window is open.
            let weak = Arc::downgrade(&studio);
            std::thread::Builder::new()
                .name("edits-ui-heartbeat".into())
                .spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let Some(s) = weak.upgrade() else { break };
                    if let Ok(mut ui) = s.ui.lock() {
                        if let Some(f) = ui.as_mut() {
                            f.tick(std::time::Instant::now());
                        }
                    };
                })?;
            let startup = app.state::<Startup>().inner().clone();
            if let Some(p) = startup.project {
                if let Err(e) = studio.open_project(std::path::Path::new(&p)) {
                    log::error!("{e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::startup,
            commands::open_project,
            commands::snapshot,
            commands::render_status,
            commands::frame,
            commands::prefetch_clear,
            commands::inspect,
            commands::edit_ops,
            commands::prop_set,
            commands::prop_key_here,
            commands::prop_remove_key,
            commands::prop_ease,
            commands::layer_add,
            commands::layer_duplicate,
            commands::layer_remove,
            commands::layer_toggle,
            commands::layer_move,
            commands::layer_reorder,
            commands::layer_drag,
            commands::effect_add,
            commands::effect_remove,
            commands::effect_toggle,
            commands::effect_move,
            commands::effect_option,
            commands::undo,
            commands::redo,
            commands::restore,
            commands::effects_list,
            commands::presets_list,
            commands::preset_apply,
            commands::ui_report,
            commands::renders_list,
            commands::render_start,
            commands::render_cancel,
            commands::analyses_find,
            commands::analysis_load,
            commands::reports_list,
            commands::report_load,
            commands::compare_status,
            commands::compare_start,
            agent::commands::agent_adapters,
            agent::commands::agent_setup,
            agent::commands::agent_connect,
            agent::commands::agent_disconnect,
            agent::commands::agent_session_new,
            agent::commands::agent_session_load,
            agent::commands::agent_session_list,
            agent::commands::agent_prompt,
            agent::commands::agent_cancel,
            agent::commands::agent_permission,
            agent::commands::agent_set_mode,
            agent::commands::agent_set_model,
            agent::commands::agent_set_config,
            agent::commands::agent_authenticate,
            agent::commands::agent_login,
            agent::commands::agent_files,
        ])
        .on_page_load(|webview, payload| {
            if webview.label() == "main" && matches!(payload.event(), PageLoadEvent::Finished) {
                log::info!("main webview finished loading");
                let _ = webview.window().show();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the studio");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(s) = handle.try_state::<Arc<studio::Studio>>() {
                s.close();
            }
            if let Some(a) = handle.try_state::<Arc<agent::AgentHost>>() {
                a.disconnect();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Startup>, String> {
        Startup::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_command_line_names_a_project_and_where_to_start() {
        assert_eq!(parse(&[]).unwrap(), Some(Startup::default()));
        assert_eq!(parse(&["--help"]).unwrap(), None);
        let s = parse(&["--page", "renders", "--select", "d", "--at", "37"])
            .unwrap()
            .unwrap();
        assert_eq!(
            (s.page.as_deref(), s.select.as_deref(), s.at),
            (Some("renders"), Some("d"), Some(37))
        );
        let path =
            std::env::temp_dir().join(format!("edits-studio-args-{}.json", std::process::id()));
        std::fs::write(&path, "{}").unwrap();
        let s = parse(&[path.to_str().unwrap()]).unwrap().unwrap();
        assert!(s.project.unwrap().ends_with(".json"));
        for bad in [
            &["--page", "nope"][..],
            &["--at", "soon"],
            &["/nonexistent/p.json"],
            &["--frobnicate"],
            &["--select"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        std::fs::remove_file(&path).ok();
    }
}
